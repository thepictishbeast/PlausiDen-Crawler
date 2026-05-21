//! `aria_haspopup_attribute` — `aria-haspopup` value + role
//! consistency audit.
//!
//! Sibling to the four IDREF-resolution detectors
//! (`aria_labelledby_resolution`, `aria_describedby_resolution`,
//! `aria_controls_resolution`, `aria_owns_resolution`) and to
//! `aria_expanded_state`. Audits the `aria-haspopup` attribute
//! used on widgets that open a secondary surface (menu,
//! listbox, dialog, grid, tree).
//!
//! ## The contract
//!
//! Per ARIA 1.2, `aria-haspopup` indicates that activating the
//! element will open a popup of the specified type. Allowed
//! values:
//!
//! * `false` (default; no popup)
//! * `true` (equivalent to `menu`)
//! * `menu`
//! * `listbox`
//! * `tree`
//! * `grid`
//! * `dialog`
//!
//! Operators routinely:
//!
//! 1. Use invented values (`"popover"`, `"tooltip"`, `"yes"`)
//!    that AT silently ignores.
//! 2. Pair `aria-haspopup` with `aria-expanded="false"` BUT
//!    never set up the popup at all (the attribute is
//!    declarative; without the actual popup it's
//!    misinformation).
//! 3. Set `aria-haspopup` on a non-button non-link role —
//!    the attribute is meaningless without an activation
//!    affordance.
//!
//! ## Findings
//!
//! * `aria-haspopup.invalid-value` strict — value not in the
//!   allowed set.
//! * `aria-haspopup.on-non-activator-role` warn — host's role
//!   is not button / link / menuitem / treeitem / tab AND has
//!   no `aria-controls` pointing at a popup target.
//! * `aria-haspopup.without-aria-controls` warn — host carries
//!   `aria-haspopup` but no `aria-controls`. The popup target
//!   relationship is implicit (the AT can't navigate to it
//!   programmatically).
//!
//! Out of scope:
//!
//! * Whether the controlled popup target actually has a role
//!   matching the declared `aria-haspopup` value — separate
//!   future axis (`aria_haspopup_target_role_match`).
//! * `aria-expanded` state — covered by `aria_expanded_state`.
//! * IDREF resolution of `aria-controls` — covered by
//!   `aria_controls_resolution`.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const MAX_EXAMPLES: usize = 5;

/// Allowed `aria-haspopup` values per ARIA 1.2.
const ALLOWED_VALUES: &[&str] =
    &["false", "true", "menu", "listbox", "tree", "grid", "dialog"];

/// Roles where `aria-haspopup` is meaningful (the host can
/// open a popup when activated).
const ACTIVATOR_ROLES: &[&str] = &[
    "button", "link", "menuitem", "treeitem", "tab", "combobox", "switch",
];

/// One captured element with `aria-haspopup` signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaHaspopupEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Tag name (lowercased).
    pub tag: String,
    /// `role=` attribute on the host (empty when no explicit
    /// role). Detector uses tag-implicit role for `button` /
    /// `a` when role is empty.
    pub role: String,
    /// Raw `aria-haspopup` value (trimmed).
    pub aria_haspopup_value: String,
    /// Whether the host carries `aria-controls`.
    pub has_aria_controls: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaHaspopupAttributeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-haspopup`.
    pub entries: Vec<AriaHaspopupEntry>,
}

/// Detector.
#[must_use]
pub fn detect_aria_haspopup_attribute(
    snap: &AriaHaspopupAttributeSnapshot,
) -> Vec<AxisFinding> {
    let mut invalid: Vec<String> = Vec::new();
    let mut on_non_activator: Vec<String> = Vec::new();
    let mut without_controls: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        let raw = entry.aria_haspopup_value.trim();
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        let is_valid =
            ALLOWED_VALUES.iter().any(|v| v.eq_ignore_ascii_case(&lower));

        if !is_valid {
            invalid.push(format!("{} (value=\"{}\")", entry.selector, raw));
            continue;
        }

        // "false" is the default — operator explicitly declared
        // "no popup". No further finding for this row.
        if lower == "false" {
            continue;
        }

        let effective_role = if entry.role.is_empty() {
            tag_implicit_role(&entry.tag)
        } else {
            entry.role.clone()
        };
        let is_activator = ACTIVATOR_ROLES
            .iter()
            .any(|r| r.eq_ignore_ascii_case(&effective_role));
        if !is_activator && !entry.has_aria_controls {
            on_non_activator.push(format!(
                "{} (tag={}, role={})",
                entry.selector, entry.tag, effective_role
            ));
        }

        if !entry.has_aria_controls {
            without_controls.push(entry.selector.as_str());
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !invalid.is_empty() {
        let preview =
            preview_examples(&invalid.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-haspopup.invalid-value".to_owned(),
            detail: format!(
                "{} of {} aria-haspopup value(s) are not in {{false, true, menu, listbox, tree, grid, dialog}}. Examples: {}",
                invalid.len(),
                total,
                preview
            ),
        });
    }

    if !on_non_activator.is_empty() {
        let preview = preview_examples(
            &on_non_activator
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-haspopup.on-non-activator-role".to_owned(),
            detail: format!(
                "{} of {} aria-haspopup host(s) carry a non-activator role AND no aria-controls; the popup-relationship hint is meaningless without an activation affordance. Examples: {}",
                on_non_activator.len(),
                total,
                preview
            ),
        });
    }

    if !without_controls.is_empty() {
        let preview = preview_examples(&without_controls);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-haspopup.without-aria-controls".to_owned(),
            detail: format!(
                "{} of {} aria-haspopup host(s) carry no aria-controls; the popup target relationship is implicit and AT can't navigate to it programmatically. Examples: {}",
                without_controls.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn tag_implicit_role(tag: &str) -> String {
    match tag.to_ascii_lowercase().as_str() {
        "button" => "button".to_owned(),
        "a" => "link".to_owned(),
        _ => String::new(),
    }
}

fn preview_examples(examples: &[&str]) -> String {
    let mut buf = String::new();
    let n = examples.len().min(MAX_EXAMPLES);
    for (i, sel) in examples.iter().take(n).enumerate() {
        if i > 0 {
            buf.push_str(" | ");
        }
        buf.push_str(sel);
    }
    if examples.len() > MAX_EXAMPLES {
        buf.push_str(&format!(" (+{} more)", examples.len() - MAX_EXAMPLES));
    }
    buf
}

/// Page-side eval. Walks every element with `aria-haspopup`.
pub const ARIA_HASPOPUP_ATTRIBUTE_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return parts.join(' > ') || 'body';
    };

    const hosts = Array.from(document.querySelectorAll('[aria-haspopup]'));
    const entries = hosts.map(function(el) {
      return {
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        role: (el.getAttribute('role') || '').toLowerCase(),
        ariaHaspopupValue: el.getAttribute('aria-haspopup') || '',
        hasAriaControls: el.hasAttribute('aria-controls')
      };
    });

    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        sel: &str,
        tag: &str,
        role: &str,
        ah: &str,
        has_controls: bool,
    ) -> AriaHaspopupEntry {
        AriaHaspopupEntry {
            selector: sel.to_owned(),
            tag: tag.to_owned(),
            role: role.to_owned(),
            aria_haspopup_value: ah.to_owned(),
            has_aria_controls: has_controls,
        }
    }

    fn snap(entries: Vec<AriaHaspopupEntry>) -> AriaHaspopupAttributeSnapshot {
        AriaHaspopupAttributeSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_haspopup_attribute(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn well_formed_button_with_controls_is_clean() {
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "button#open-menu",
            "button",
            "",
            "menu",
            true,
        )]));
        assert!(f.is_empty(), "well-formed should pass: {f:?}");
    }

    #[test]
    fn each_allowed_value_is_valid() {
        for v in ["false", "true", "menu", "listbox", "tree", "grid", "dialog"] {
            let f = detect_aria_haspopup_attribute(&snap(vec![entry(
                "button", "button", "", v, true,
            )]));
            assert!(
                !f.iter().any(|x| x.kind == "aria-haspopup.invalid-value"),
                "value {v} should be valid"
            );
        }
    }

    #[test]
    fn invented_values_are_strict() {
        for v in ["popover", "tooltip", "yes", "no", "1", "0"] {
            let f = detect_aria_haspopup_attribute(&snap(vec![entry(
                "button", "button", "", v, true,
            )]));
            assert!(
                f.iter().any(|x| x.kind == "aria-haspopup.invalid-value"),
                "invented value {v} should flag"
            );
        }
    }

    #[test]
    fn case_insensitive_value_match() {
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "button", "button", "", "MENU", true,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "aria-haspopup.invalid-value"),
            "MENU should be valid"
        );
    }

    #[test]
    fn false_value_is_clean_with_or_without_controls() {
        // false means "no popup" — no further finding.
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "button", "button", "", "false", false,
        )]));
        assert!(f.is_empty(), "false should pass: {f:?}");
    }

    #[test]
    fn missing_aria_controls_is_warn() {
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "button#trigger",
            "button",
            "",
            "menu",
            false,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-haspopup.without-aria-controls")
            .expect("without-controls expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn on_non_activator_role_is_warn() {
        // <div role="region" aria-haspopup="menu"> — region is
        // not an activator role.
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "div#region",
            "div",
            "region",
            "menu",
            false,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-haspopup.on-non-activator-role")
            .expect("on-non-activator expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn link_implicit_role_is_activator() {
        // <a aria-haspopup="menu" aria-controls="m"> — link
        // implicit role is activator-eligible (some sites use
        // <a> as JS-fallback for menu triggers).
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "a#menu-trigger",
            "a",
            "",
            "menu",
            true,
        )]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "aria-haspopup.on-non-activator-role"),
            "<a> implicit role should be activator: {f:?}"
        );
    }

    #[test]
    fn invalid_value_suppresses_other_findings() {
        let f = detect_aria_haspopup_attribute(&snap(vec![entry(
            "div", "div", "region", "popover", false,
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "aria-haspopup.invalid-value"));
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-haspopup.on-non-activator-role"));
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-haspopup.without-aria-controls"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("button#x{i}"),
                    "button",
                    "",
                    "popover",
                    false,
                )
            })
            .collect();
        let f = detect_aria_haspopup_attribute(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-haspopup.invalid-value")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_aria_hosts() {
        assert!(ARIA_HASPOPUP_ATTRIBUTE_JS.starts_with("(() => {"));
        assert!(ARIA_HASPOPUP_ATTRIBUTE_JS.ends_with(")()"));
        assert!(ARIA_HASPOPUP_ATTRIBUTE_JS.contains("[aria-haspopup]"));
        assert!(ARIA_HASPOPUP_ATTRIBUTE_JS.contains("aria-controls"));
    }
}
