//! `aria_controls_resolution` — id-reference resolution check
//! for `aria-controls`.
//!
//! Sibling axis to `aria_labelledby_resolution` (accessible name)
//! and `aria_describedby_resolution` (accessible description).
//! `aria-controls` has the same IDREF shape but different
//! semantics: it declares that the host element controls the
//! element(s) named — common for tabs (`<button aria-controls="
//! panel-1">`), disclosure buttons that toggle a panel, and
//! combobox listboxes.
//!
//! ## The bug class
//!
//! Three patterns recur:
//!
//! 1. **Dangling**: the panel was renamed or removed but the
//!    button still claims to control it. Click does nothing
//!    detectable to AT.
//! 2. **Stale conditional rendering**: SPA frameworks remove the
//!    controlled panel from DOM when collapsed but leave the
//!    `aria-controls` reference on the button. ARIA spec
//!    explicitly permits dynamic insertion — but linting tools
//!    can't tell if a runtime snapshot caught the panel
//!    detached. Marked warn rather than strict because the
//!    runner can't distinguish a real bug from collapsed-panel
//!    state.
//! 3. **Wrong direction**: operators write
//!    `aria-controls="trigger-id"` on the controlled element
//!    rather than the controlling element. Resolution succeeds
//!    but the semantic relationship is inverted. Detector
//!    cannot prove inversion in general; it can only surface the
//!    presence of an `aria-controls` on a typically-controlled
//!    role (panel / region / dialog) for the runner to audit.
//!
//! ## Findings
//!
//! * `aria-controls.dangling-ref` strict — NO id-ref in the
//!   attribute resolves to any element on the page.
//! * `aria-controls.partial-resolution` warn — multi-id attribute
//!   where SOME ids resolve and others don't. Less strict than
//!   labelledby because aria-controls is a semantic-relationship
//!   hint, not the basis of the accessible name.
//! * `aria-controls.empty-attribute` strict — attribute is
//!   present with empty / whitespace-only value.
//! * `aria-controls.inverted-direction-suspect` warn —
//!   `aria-controls` declared on a role that is typically the
//!   CONTROLLED party (`tabpanel`, `region`, `dialog`,
//!   `alertdialog`, `group`); operator likely meant the inverse
//!   reference on the controlling button.
//!
//! Out of scope:
//!
//! * `aria-labelledby` — covered by sibling axis.
//! * `aria-describedby` — covered by sibling axis.
//! * `aria-owns`, `aria-flowto` — same IDREF shape, different
//!   semantics; each gets its own axis.
//! * Whether the controlled panel's `aria-labelledby` actually
//!   points back at the button — bidirectional tab-panel
//!   relationships are checked by a future
//!   `tabpanel_reciprocal_labelledby` axis.
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

/// Roles that are typically the controlled side of an
/// aria-controls relationship. `aria-controls` declared on these
/// roles is suspect and gets surfaced as a warn.
const TYPICALLY_CONTROLLED_ROLES: &[&str] = &[
    "tabpanel",
    "region",
    "dialog",
    "alertdialog",
    "group",
    "listbox",
];

/// One element on the page carrying an `aria-controls` attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaControlsEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// `role=` attribute on the host element (lowercased; empty
    /// when no explicit role).
    pub host_role: String,
    /// Raw attribute value (trimmed). Empty string means the
    /// attribute was present but its value was whitespace-only.
    pub attribute_value: String,
    /// Per-id resolution. Empty when `attribute_value` was empty.
    pub resolutions: Vec<ControlsResolution>,
}

/// Resolution result for a single id token inside the attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ControlsResolution {
    /// Id token from the attribute.
    pub id_ref: String,
    /// Whether the page has an element with this id.
    pub resolves: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaControlsResolutionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-controls`.
    pub entries: Vec<AriaControlsEntry>,
}

/// Detector. Buckets findings by defect kind so consumers see at
/// most one finding per kind.
#[must_use]
pub fn detect_aria_controls_resolution(
    snap: &AriaControlsResolutionSnapshot,
) -> Vec<AxisFinding> {
    let mut dangling: Vec<String> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    let mut empty_attr: Vec<&str> = Vec::new();
    let mut inverted_suspect: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        // Inverted-direction check first; applies whether or not
        // resolution succeeded.
        let role = entry.host_role.to_ascii_lowercase();
        if !role.is_empty()
            && TYPICALLY_CONTROLLED_ROLES
                .iter()
                .any(|r| r.eq_ignore_ascii_case(&role))
        {
            inverted_suspect.push(entry.selector.as_str());
        }

        if entry.attribute_value.is_empty() {
            empty_attr.push(entry.selector.as_str());
            continue;
        }
        let total = entry.resolutions.len();
        let resolved = entry.resolutions.iter().filter(|r| r.resolves).count();
        let unresolved: Vec<&str> = entry
            .resolutions
            .iter()
            .filter(|r| !r.resolves)
            .map(|r| r.id_ref.as_str())
            .collect();

        if resolved == 0 && total > 0 {
            dangling.push(format!(
                "{} (ids={})",
                entry.selector,
                unresolved.join(",")
            ));
        } else if !unresolved.is_empty() {
            partial.push(format!(
                "{} (missing={})",
                entry.selector,
                unresolved.join(",")
            ));
        }
    }

    let mut findings = Vec::new();
    let total_entries = snap.entries.len();

    if !dangling.is_empty() {
        let preview =
            preview_examples(&dangling.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-controls.dangling-ref".to_owned(),
            detail: format!(
                "{} of {} aria-controls host(s) have NO id-ref that resolves. Examples: {}",
                dangling.len(),
                total_entries,
                preview
            ),
        });
    }

    if !empty_attr.is_empty() {
        let preview = preview_examples(&empty_attr);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-controls.empty-attribute".to_owned(),
            detail: format!(
                "{} of {} aria-controls host(s) carry an empty / whitespace-only attribute value. Examples: {}",
                empty_attr.len(),
                total_entries,
                preview
            ),
        });
    }

    if !partial.is_empty() {
        let preview =
            preview_examples(&partial.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-controls.partial-resolution".to_owned(),
            detail: format!(
                "{} of {} aria-controls host(s) have SOME ids that resolve and others that don't. Examples: {}",
                partial.len(),
                total_entries,
                preview
            ),
        });
    }

    if !inverted_suspect.is_empty() {
        let preview = preview_examples(&inverted_suspect);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-controls.inverted-direction-suspect".to_owned(),
            detail: format!(
                "{} of {} aria-controls host(s) carry role(s) typically on the CONTROLLED side (tabpanel/region/dialog/group/listbox); operator likely meant the inverse reference on the controlling element. Examples: {}",
                inverted_suspect.len(),
                total_entries,
                preview
            ),
        });
    }

    findings
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

/// Page-side eval. Walks every element with `aria-controls`,
/// resolves each id token, and captures the host element's role
/// so the detector can check inverted-direction patterns.
pub const ARIA_CONTROLS_RESOLUTION_JS: &str = r##"(() => {
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

    const hosts = Array.from(document.querySelectorAll('[aria-controls]'));
    const entries = hosts.map(function(el) {
      const raw = (el.getAttribute('aria-controls') || '').trim();
      const role = (el.getAttribute('role') || '').trim().toLowerCase();
      if (!raw) {
        return {
          selector: selectorOf(el),
          hostRole: role,
          attributeValue: '',
          resolutions: []
        };
      }
      const ids = raw.split(/\s+/).filter(function(s) { return s.length > 0; });
      const resolutions = ids.map(function(id) {
        const target = document.getElementById(id);
        return { idRef: id, resolves: !!target };
      });
      return {
        selector: selectorOf(el),
        hostRole: role,
        attributeValue: raw,
        resolutions: resolutions
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

    fn res(id: &str, resolves: bool) -> ControlsResolution {
        ControlsResolution {
            id_ref: id.to_owned(),
            resolves,
        }
    }

    fn entry(
        sel: &str,
        role: &str,
        attr: &str,
        resolutions: Vec<ControlsResolution>,
    ) -> AriaControlsEntry {
        AriaControlsEntry {
            selector: sel.to_owned(),
            host_role: role.to_owned(),
            attribute_value: attr.to_owned(),
            resolutions,
        }
    }

    fn snap(entries: Vec<AriaControlsEntry>) -> AriaControlsResolutionSnapshot {
        AriaControlsResolutionSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_controls_resolution(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn fully_resolved_tab_button_is_clean() {
        let f = detect_aria_controls_resolution(&snap(vec![entry(
            "button#tab-1",
            "tab",
            "panel-1",
            vec![res("panel-1", true)],
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn no_ids_resolve_is_strict_dangling() {
        let f = detect_aria_controls_resolution(&snap(vec![entry(
            "button#tab-2",
            "tab",
            "missing-panel",
            vec![res("missing-panel", false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-controls.dangling-ref")
            .expect("dangling-ref expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("button#tab-2"));
        assert!(hit.detail.contains("missing-panel"));
    }

    #[test]
    fn partial_resolution_is_warn() {
        let f = detect_aria_controls_resolution(&snap(vec![entry(
            "button#multi",
            "button",
            "p1 p2",
            vec![res("p1", true), res("p2", false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-controls.partial-resolution")
            .expect("partial-resolution expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("button#multi"));
        assert!(hit.detail.contains("missing=p2"));
    }

    #[test]
    fn empty_attribute_is_strict() {
        let f = detect_aria_controls_resolution(&snap(vec![entry(
            "button#empty",
            "button",
            "",
            vec![],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-controls.empty-attribute")
            .expect("empty-attribute expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn inverted_direction_on_tabpanel_is_warn() {
        // aria-controls on a tabpanel is the inverse of the
        // typical pattern (the TAB carries the reference).
        let f = detect_aria_controls_resolution(&snap(vec![entry(
            "section#panel-1",
            "tabpanel",
            "tab-1",
            vec![res("tab-1", true)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-controls.inverted-direction-suspect")
            .expect("inverted-direction expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("section#panel-1"));
    }

    #[test]
    fn inverted_direction_each_controlled_role_flags() {
        let roles = ["region", "dialog", "alertdialog", "group", "listbox"];
        for role in roles {
            let f = detect_aria_controls_resolution(&snap(vec![entry(
                "section#x",
                role,
                "trigger",
                vec![res("trigger", true)],
            )]));
            assert!(
                f.iter()
                    .any(|x| x.kind == "aria-controls.inverted-direction-suspect"),
                "role={role} should trigger inverted-direction"
            );
        }
    }

    #[test]
    fn host_without_role_does_not_trigger_inversion() {
        let f = detect_aria_controls_resolution(&snap(vec![entry(
            "button#trigger",
            "", // no explicit role
            "panel-1",
            vec![res("panel-1", true)],
        )]));
        assert!(
            !f.iter()
                .any(|x| x.kind == "aria-controls.inverted-direction-suspect"),
            "no role should not flag inversion"
        );
    }

    #[test]
    fn multiple_dangling_share_bucket() {
        let f = detect_aria_controls_resolution(&snap(vec![
            entry("button#a", "button", "x", vec![res("x", false)]),
            entry("button#b", "button", "y", vec![res("y", false)]),
            entry("button#c", "button", "z", vec![res("z", false)]),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-controls.dangling-ref")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("button#h{i}"),
                    "button",
                    "x",
                    vec![res("x", false)],
                )
            })
            .collect();
        let f = detect_aria_controls_resolution(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-controls.dangling-ref")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_aria_controls_hosts() {
        assert!(ARIA_CONTROLS_RESOLUTION_JS.starts_with("(() => {"));
        assert!(ARIA_CONTROLS_RESOLUTION_JS.ends_with(")()"));
        assert!(ARIA_CONTROLS_RESOLUTION_JS.contains("aria-controls"));
        assert!(ARIA_CONTROLS_RESOLUTION_JS.contains("getElementById"));
        assert!(ARIA_CONTROLS_RESOLUTION_JS.contains("role"));
    }
}
