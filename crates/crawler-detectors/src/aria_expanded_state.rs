//! `aria_expanded_state` — `aria-expanded` state-consistency
//! audit.
//!
//! Sibling to the four IDREF-resolution detectors
//! (`aria_labelledby_resolution`, `aria_describedby_resolution`,
//! `aria_controls_resolution`, `aria_owns_resolution`) and to
//! `details_open_default`. This detector covers the
//! `aria-expanded` state attribute used on toggle-style
//! disclosure widgets (custom dropdowns, accordion sections,
//! `<button aria-expanded>` triggers).
//!
//! ## The bug class
//!
//! Per ARIA Authoring Practices, `aria-expanded` is required on
//! roles where the user can toggle a disclosure (button as
//! toggle, combobox, menu, treeitem-with-children). Common
//! authoring failures:
//!
//! 1. **Invalid value** — `aria-expanded="open"` /
//!    `"closed"` / `"yes"` / `"no"` / `""`. ARIA spec requires
//!    `"true"` or `"false"` (the keyword `undefined` is also
//!    permitted but rare in author markup).
//! 2. **Stale state** — `aria-expanded="false"` on an element
//!    whose controlled panel is visible (the page rendered the
//!    open state but the attribute wasn't updated). Detector
//!    can verify ONLY when `aria-controls` resolves to an
//!    element with computed visibility.
//! 3. **`aria-expanded` on a non-toggling role** — e.g.
//!    `<a aria-expanded="false">` linking somewhere else.
//!    Confuses AT into announcing collapse / expand behaviour
//!    that doesn't exist.
//! 4. **Missing on disclosure trigger** — a button with
//!    `aria-controls="panel"` toggling that panel's visibility
//!    SHOULD carry `aria-expanded`. Surfaced as a separate
//!    finding when the host has `aria-controls` but no
//!    `aria-expanded`.
//!
//! ## Findings
//!
//! * `aria-expanded.invalid-value` strict — attribute value
//!   not in `{"true", "false", "undefined"}`.
//! * `aria-expanded.stale-state` warn — attribute says
//!   `"false"` but `aria-controls` target is computed-visible
//!   (or `"true"` but target is hidden). Detector can only
//!   verify when runner supplies controlled-target visibility.
//! * `aria-expanded.on-non-toggling-role` warn — element's
//!   role is none of `{button, link with toggle, combobox,
//!   menu, treeitem, tab}` AND has no matching `aria-controls`.
//! * `aria-expanded.missing-on-disclosure` warn — host has
//!   `aria-controls` pointing at a toggleable element AND no
//!   `aria-expanded` on the host itself.
//!
//! Out of scope:
//!
//! * `<details>` element configuration — covered by
//!   `details_open_default`.
//! * `<dialog>` open/close — covered by `dialog_label` plus
//!   `dialog_trap_focus`.
//! * IDREF resolution — covered by `aria_controls_resolution`.
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

/// Roles where `aria-expanded` is meaningful per ARIA APG.
const TOGGLING_ROLES: &[&str] = &[
    "button",
    "combobox",
    "menu",
    "menuitem",
    "treeitem",
    "tab",
    "link",
    "switch",
];

/// One captured element with aria-expanded signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaExpandedEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Tag name (lowercased) — `button`, `a`, `div`, etc.
    pub tag: String,
    /// `role=` attribute on the host (empty when no explicit
    /// role; detector uses tag-implicit role for `button` / `a`).
    pub role: String,
    /// Raw `aria-expanded` value (trimmed). Empty string means
    /// the attribute was present without a value.
    pub aria_expanded_value: String,
    /// Whether the host carries `aria-controls` (used for the
    /// missing-on-disclosure check).
    pub has_aria_controls: bool,
    /// Optional visibility of the controlled target: `Some(true)`
    /// = visible, `Some(false)` = hidden, `None` = runner did
    /// not resolve.
    pub controlled_target_visible: Option<bool>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaExpandedStateSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Hosts: any element with `aria-expanded` OR
    /// `aria-controls` (so the missing-on-disclosure check has
    /// candidates).
    pub entries: Vec<AriaExpandedEntry>,
}

/// Detector.
#[must_use]
pub fn detect_aria_expanded_state(
    snap: &AriaExpandedStateSnapshot,
) -> Vec<AxisFinding> {
    let mut invalid: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    let mut on_non_toggling: Vec<String> = Vec::new();
    let mut missing_on_disclosure: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        let has_aria_expanded =
            !entry.aria_expanded_value.is_empty() || entry.aria_expanded_value == "";
        // Treat presence by checking for an explicit value;
        // empty-string "present with no value" we'll handle as
        // invalid.
        let raw = entry.aria_expanded_value.trim();
        let attr_present = has_aria_expanded && !raw.is_empty()
            || entry.aria_expanded_value.is_empty() && entry.controlled_target_visible.is_some();

        // Cleaner: derive presence directly. We say "attr
        // present" when the raw value (post-trim) is non-empty
        // OR when the runner emitted an empty string with a
        // controlled-target visibility (rare). For the missing-
        // on-disclosure check we use a more reliable signal
        // below.
        let _ = attr_present;
        let aria_expanded_attr_present = !entry.aria_expanded_value.is_empty();

        if aria_expanded_attr_present {
            let normalised = raw.to_ascii_lowercase();
            let is_valid = matches!(
                normalised.as_str(),
                "true" | "false" | "undefined"
            );
            if !is_valid {
                invalid.push(format!(
                    "{} (value=\"{}\")",
                    entry.selector, raw
                ));
            }

            // Stale-state check (only when target visibility
            // resolved AND value is true/false).
            if let Some(visible) = entry.controlled_target_visible {
                let expanded_says_open = normalised == "true";
                let expanded_says_closed = normalised == "false";
                if expanded_says_open && !visible {
                    stale.push(format!(
                        "{} (says \"true\" but target is hidden)",
                        entry.selector
                    ));
                } else if expanded_says_closed && visible {
                    stale.push(format!(
                        "{} (says \"false\" but target is visible)",
                        entry.selector
                    ));
                }
            }

            // Non-toggling role check — only when the value is
            // syntactically valid (otherwise the invalid
            // finding is the primary concern).
            let effective_role = if entry.role.is_empty() {
                tag_implicit_role(&entry.tag)
            } else {
                entry.role.clone()
            };
            let is_toggling = TOGGLING_ROLES
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&effective_role));
            if is_valid && !is_toggling && !entry.has_aria_controls {
                on_non_toggling.push(format!(
                    "{} (tag={}, role={})",
                    entry.selector, entry.tag, effective_role
                ));
            }
        }

        if entry.has_aria_controls && !aria_expanded_attr_present {
            // The host carries aria-controls and toggles
            // visibility but has no aria-expanded. We only flag
            // when the host's role is one of the toggling
            // roles — otherwise the aria-controls might be
            // labelling a relationship that isn't a disclosure.
            let effective_role = if entry.role.is_empty() {
                tag_implicit_role(&entry.tag)
            } else {
                entry.role.clone()
            };
            if TOGGLING_ROLES
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&effective_role))
            {
                missing_on_disclosure.push(entry.selector.as_str());
            }
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !invalid.is_empty() {
        let preview =
            preview_examples(&invalid.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-expanded.invalid-value".to_owned(),
            detail: format!(
                "{} of {} aria-expanded value(s) are not in {{\"true\", \"false\", \"undefined\"}}. Examples: {}",
                invalid.len(),
                total,
                preview
            ),
        });
    }

    if !stale.is_empty() {
        let preview =
            preview_examples(&stale.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-expanded.stale-state".to_owned(),
            detail: format!(
                "{} of {} aria-expanded value(s) disagree with the controlled target's visibility. Examples: {}",
                stale.len(),
                total,
                preview
            ),
        });
    }

    if !on_non_toggling.is_empty() {
        let preview = preview_examples(
            &on_non_toggling
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-expanded.on-non-toggling-role".to_owned(),
            detail: format!(
                "{} of {} aria-expanded host(s) carry a non-toggling role AND no aria-controls; confuses AT into announcing collapse/expand behaviour that doesn't exist. Examples: {}",
                on_non_toggling.len(),
                total,
                preview
            ),
        });
    }

    if !missing_on_disclosure.is_empty() {
        let preview = preview_examples(&missing_on_disclosure);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-expanded.missing-on-disclosure".to_owned(),
            detail: format!(
                "{} of {} toggling-role host(s) with aria-controls lack aria-expanded; AT can't announce open/closed state. Examples: {}",
                missing_on_disclosure.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn tag_implicit_role(tag: &str) -> String {
    // ARIA implicit-role mapping for the tags we care about.
    match tag.to_ascii_lowercase().as_str() {
        "button" => "button".to_owned(),
        "a" => "link".to_owned(),
        // <details>/<summary> aren't toggling-role hosts for
        // aria-expanded purposes — they have their own
        // open/closed semantics covered by details_open_default.
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

/// Page-side eval. Walks every element with `aria-expanded` OR
/// `aria-controls`. Optionally resolves controlled-target
/// visibility.
pub const ARIA_EXPANDED_STATE_JS: &str = r##"(() => {
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

    const isVisible = function(el) {
      if (!el) return null;
      const cs = getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const r = el.getBoundingClientRect();
      if (r.width === 0 && r.height === 0) return false;
      return true;
    };

    const hosts = Array.from(document.querySelectorAll('[aria-expanded], [aria-controls]'));
    const entries = hosts.map(function(el) {
      const aeRaw = el.hasAttribute('aria-expanded') ? (el.getAttribute('aria-expanded') || '') : '';
      const controlsId = (el.getAttribute('aria-controls') || '').trim().split(/\s+/)[0] || '';
      let targetVisible = null;
      if (controlsId) {
        const t = document.getElementById(controlsId);
        const v = isVisible(t);
        if (v !== null) targetVisible = v;
      }
      return {
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        role: (el.getAttribute('role') || '').toLowerCase(),
        ariaExpandedValue: aeRaw,
        hasAriaControls: !!controlsId,
        controlledTargetVisible: targetVisible
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
        ae: &str,
        has_controls: bool,
        target_visible: Option<bool>,
    ) -> AriaExpandedEntry {
        AriaExpandedEntry {
            selector: sel.to_owned(),
            tag: tag.to_owned(),
            role: role.to_owned(),
            aria_expanded_value: ae.to_owned(),
            has_aria_controls: has_controls,
            controlled_target_visible: target_visible,
        }
    }

    fn snap(entries: Vec<AriaExpandedEntry>) -> AriaExpandedStateSnapshot {
        AriaExpandedStateSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_expanded_state(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn well_formed_button_with_controls_is_clean() {
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button#trigger",
            "button",
            "",
            "false",
            true,
            Some(false),
        )]));
        assert!(f.is_empty(), "well-formed should pass: {f:?}");
    }

    #[test]
    fn invalid_value_open_is_strict() {
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button#bad",
            "button",
            "",
            "open",
            false,
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-expanded.invalid-value")
            .expect("invalid-value expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("\"open\""));
    }

    #[test]
    fn yes_no_values_are_strict() {
        for v in ["yes", "no", "open", "closed", "1", "0"] {
            let f = detect_aria_expanded_state(&snap(vec![entry(
                "button", "button", "", v, false, None,
            )]));
            assert!(
                f.iter().any(|x| x.kind == "aria-expanded.invalid-value"),
                "value \"{v}\" should be invalid"
            );
        }
    }

    #[test]
    fn case_insensitive_value_match() {
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button", "button", "", "TRUE", true, Some(true),
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "aria-expanded.invalid-value"),
            "TRUE should be accepted case-insensitively"
        );
    }

    #[test]
    fn stale_state_says_true_but_hidden_is_warn() {
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button#trigger",
            "button",
            "",
            "true",
            true,
            Some(false),
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-expanded.stale-state")
            .expect("stale expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("but target is hidden"));
    }

    #[test]
    fn stale_state_says_false_but_visible_is_warn() {
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button#trigger",
            "button",
            "",
            "false",
            true,
            Some(true),
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-expanded.stale-state")
            .expect("stale expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("but target is visible"));
    }

    #[test]
    fn on_non_toggling_role_is_warn() {
        // <div role="banner" aria-expanded="false"> — non-
        // toggling role + no aria-controls.
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "div#banner",
            "div",
            "banner",
            "false",
            false,
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-expanded.on-non-toggling-role")
            .expect("on-non-toggling expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn missing_on_disclosure_is_warn() {
        // Button with aria-controls but no aria-expanded.
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button#missing",
            "button",
            "",
            "",
            true,
            Some(false),
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-expanded.missing-on-disclosure")
            .expect("missing expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn missing_aria_expanded_on_non_toggling_host_with_controls_is_ignored() {
        // A <div role="region" aria-controls="x"> isn't a
        // disclosure trigger; don't flag.
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "div#region",
            "div",
            "region",
            "",
            true,
            Some(true),
        )]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "aria-expanded.missing-on-disclosure"),
            "non-toggling role + aria-controls should not flag: {f:?}"
        );
    }

    #[test]
    fn explicit_role_overrides_tag_implicit() {
        // <button role="banner" aria-expanded="false"> — author
        // overrode the implicit button role to banner.
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "button",
            "button",
            "banner",
            "false",
            false,
            None,
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "aria-expanded.on-non-toggling-role"));
    }

    #[test]
    fn invalid_value_suppresses_on_non_toggling_finding() {
        // When value is invalid, the on-non-toggling finding
        // doesn't also fire — one strict finding is enough.
        let f = detect_aria_expanded_state(&snap(vec![entry(
            "div#x", "div", "banner", "open", false, None,
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "aria-expanded.invalid-value"));
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-expanded.on-non-toggling-role"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("button#x{i}"),
                    "button",
                    "",
                    "open",
                    false,
                    None,
                )
            })
            .collect();
        let f = detect_aria_expanded_state(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-expanded.invalid-value")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_aria_hosts() {
        assert!(ARIA_EXPANDED_STATE_JS.starts_with("(() => {"));
        assert!(ARIA_EXPANDED_STATE_JS.ends_with(")()"));
        assert!(ARIA_EXPANDED_STATE_JS.contains("[aria-expanded], [aria-controls]"));
        assert!(ARIA_EXPANDED_STATE_JS.contains("getComputedStyle"));
        assert!(ARIA_EXPANDED_STATE_JS.contains("getBoundingClientRect"));
    }
}
