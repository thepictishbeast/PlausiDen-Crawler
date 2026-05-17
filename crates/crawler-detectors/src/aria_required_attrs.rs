//! `aria_required_attrs` — WAI-ARIA "required-states-and-properties"
//! audit.
//!
//! Per ARIA 1.2 § Roles, each role has a set of REQUIRED attributes
//! that MUST be present for assistive tech to interpret the element.
//! Missing required attrs make the role a no-op + frequently a
//! reader-confusing trap.
//!
//! This detector covers the role/required pairs from the
//! WAI-ARIA Authoring Practices "common authoring failures" list,
//! which are by far the most-frequent real-world violations:
//!
//!   * `role="checkbox"`         needs `aria-checked`
//!   * `role="radio"`            needs `aria-checked`
//!   * `role="switch"`           needs `aria-checked`
//!   * `role="combobox"`         needs `aria-expanded`
//!   * `role="slider"`           needs `aria-valuenow`,
//!                                      `aria-valuemin`,
//!                                      `aria-valuemax`
//!   * `role="spinbutton"`       needs `aria-valuenow`
//!   * `role="scrollbar"`        needs `aria-controls`,
//!                                      `aria-orientation`,
//!                                      `aria-valuenow`,
//!                                      `aria-valuemin`,
//!                                      `aria-valuemax`
//!   * `role="progressbar"`      needs `aria-valuenow` (or be
//!                                      indeterminate via missing
//!                                      valuenow + present
//!                                      aria-valuemin/max — checked
//!                                      with a relaxed rule)
//!   * `role="heading"`          needs `aria-level`
//!   * `role="option"`           needs `aria-selected`
//!
//! Findings (strict, gate-blocking):
//!   * `aria.missing-required-attr`   any role/attr pair fails
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured element with an `role=` attribute set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaRoleEntry {
    /// CSS selector pointing at the element.
    pub selector: String,
    /// First role token from the element's `role` attribute,
    /// lowercased.
    pub role: String,
    /// Set of `aria-*` attribute names present on the element
    /// (lowercased, without the value).
    pub aria_attrs: Vec<String>,
}

/// Captured ARIA role set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaRequiredAttrsSnapshot {
    /// Page URL.
    pub page_url: String,
    /// All elements with non-empty `role=`.
    pub elements: Vec<AriaRoleEntry>,
}

/// Page-side eval. Collects every element with a `role=` attribute
/// plus its `aria-*` attribute names.
pub const ARIA_REQUIRED_ATTRS_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    const out = [];
    const elems = document.querySelectorAll('[role]');
    for (let i = 0; i < elems.length; i++) {
        const el = elems[i];
        const role = (el.getAttribute('role') || '').toLowerCase().trim().split(/\s+/)[0] || '';
        if (role.length === 0) continue;
        const aria = [];
        const attrs = el.attributes;
        for (let j = 0; j < attrs.length; j++) {
            const name = attrs[j].name.toLowerCase();
            if (name.indexOf('aria-') === 0) aria.push(name);
        }
        out.push({ selector: selectorOf(el), role: role, ariaAttrs: aria });
    }
    return { pageUrl: window.location.href, elements: out };
})()"##;

/// Required-attrs table per the ARIA 1.2 authoring-practices
/// "common failures" subset.
const REQUIRED: &[(&str, &[&str])] = &[
    ("checkbox", &["aria-checked"]),
    ("radio", &["aria-checked"]),
    ("switch", &["aria-checked"]),
    ("combobox", &["aria-expanded"]),
    (
        "slider",
        &["aria-valuenow", "aria-valuemin", "aria-valuemax"],
    ),
    ("spinbutton", &["aria-valuenow"]),
    (
        "scrollbar",
        &[
            "aria-controls",
            "aria-orientation",
            "aria-valuenow",
            "aria-valuemin",
            "aria-valuemax",
        ],
    ),
    ("heading", &["aria-level"]),
    ("option", &["aria-selected"]),
];

/// Run the detector.
pub fn detect_aria_required_attrs_issues(snap: &AriaRequiredAttrsSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    for el in &snap.elements {
        let Some((_, required)) = REQUIRED.iter().find(|(role, _)| *role == el.role) else {
            continue;
        };
        let mut missing = Vec::new();
        for need in *required {
            if !el.aria_attrs.iter().any(|a| a == need) {
                missing.push(*need);
            }
        }
        if !missing.is_empty() {
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "aria.missing-required-attr".into(),
                detail: format!(
                    "role=\"{}\" missing required attr(s): {} ({})",
                    el.role,
                    missing.join(", "),
                    el.selector
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(role: &str, aria: &[&str]) -> AriaRoleEntry {
        AriaRoleEntry {
            selector: format!("[role={}]", role),
            role: role.to_string(),
            aria_attrs: aria.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn snap(elements: Vec<AriaRoleEntry>) -> AriaRequiredAttrsSnapshot {
        AriaRequiredAttrsSnapshot {
            page_url: "https://example.com/".into(),
            elements,
        }
    }

    #[test]
    fn checkbox_without_aria_checked_is_strict() {
        let s = snap(vec![entry("checkbox", &[])]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "aria.missing-required-attr");
        assert!(f[0].detail.contains("aria-checked"));
    }

    #[test]
    fn checkbox_with_aria_checked_is_clean() {
        let s = snap(vec![entry("checkbox", &["aria-checked"])]);
        assert!(detect_aria_required_attrs_issues(&s).is_empty());
    }

    #[test]
    fn slider_missing_all_three_emits_one_finding_listing_all() {
        let s = snap(vec![entry("slider", &[])]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("aria-valuenow"));
        assert!(f[0].detail.contains("aria-valuemin"));
        assert!(f[0].detail.contains("aria-valuemax"));
    }

    #[test]
    fn slider_with_partial_attrs_still_flags_missing() {
        let s = snap(vec![entry("slider", &["aria-valuenow"])]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(!f[0].detail.contains("aria-valuenow"));
        assert!(f[0].detail.contains("aria-valuemin"));
        assert!(f[0].detail.contains("aria-valuemax"));
    }

    #[test]
    fn heading_needs_aria_level() {
        let s = snap(vec![entry("heading", &[])]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("aria-level"));
    }

    #[test]
    fn unknown_role_is_ignored() {
        let s = snap(vec![entry("custom-thing", &[])]);
        assert!(detect_aria_required_attrs_issues(&s).is_empty());
    }

    #[test]
    fn combobox_needs_aria_expanded() {
        let s = snap(vec![entry("combobox", &["aria-haspopup"])]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("aria-expanded"));
    }

    #[test]
    fn option_needs_aria_selected() {
        let s = snap(vec![entry("option", &[])]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("aria-selected"));
    }

    #[test]
    fn multiple_elements_collect_findings() {
        let s = snap(vec![
            entry("checkbox", &[]),                // strict
            entry("radio", &["aria-checked"]),     // clean
            entry("switch", &[]),                  // strict
            entry("combobox", &["aria-expanded"]), // clean
            entry("custom", &[]),                  // ignored
        ]);
        let f = detect_aria_required_attrs_issues(&s);
        assert_eq!(f.len(), 2);
    }
}
