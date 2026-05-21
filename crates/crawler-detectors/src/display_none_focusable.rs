//! `display_none_focusable` — focusable elements inside hidden
//! subtrees that browsers (correctly) skip but assistive tech may
//! still announce, or that scripts subsequently un-hide leaving
//! the original tabindex semantics intact.
//!
//! Common failure modes:
//!
//! * JS toggles `display: none` to hide a dialog / panel without
//!   also clearing `tabindex` from its contents. When the panel
//!   unhides via a different code path (mismatched state machine),
//!   the focusables are tabbable again — but the focus order
//!   surprises users because the panel wasn't visible the last
//!   time they tabbed through.
//! * Visually-hidden \"skip link\" + screen-reader-only content
//!   uses `display: none` instead of the visually-hidden
//!   technique (`clip-path: inset(50%)` + width/height 1px). The
//!   `display: none` removes the element from the accessibility
//!   tree too, defeating the purpose.
//! * Dead JS state where a modal was \"closed\" by setting
//!   `display: none` but the underlying focusables still hold
//!   `aria-modal=\"true\"` + their tab order — confusing screen
//!   readers that re-enter the page.
//!
//! HEURISTIC
//! ---------
//! 1. Walk every element matching the focusable selector list:
//!    `a[href]`, `button`, `input` (not type=hidden), `select`,
//!    `textarea`, `[tabindex]`, `[contenteditable=true]`.
//! 2. For each, walk the parent chain; if any ancestor has
//!    `display: none` or `visibility: hidden`, record the element
//!    along with which hiding mechanism applied + which ancestor.
//! 3. Skip elements inside `<template>` / `<script>` (inert).
//! 4. Cap surfaced offenders at 50; full count surfaces in
//!    `hiddenFocusableCount` regardless.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = \"deny\"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const DISPLAY_NONE_FOCUSABLE_JS: &str = r##"(() => {
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

    const inInertContext = function(el) {
      let node = el.parentElement;
      while (node) {
        if (node.tagName === 'TEMPLATE') return true;
        if (node.tagName === 'SCRIPT') return true;
        node = node.parentElement;
      }
      return false;
    };

    // Walk the parent chain returning [hideMode, ancestorTag] for
    // the first ancestor that hides this subtree. Returns null if
    // no hidden ancestor is found.
    const findHiddenAncestor = function(el) {
      let node = el;
      while (node && node !== document.body) {
        const cs = window.getComputedStyle(node);
        if (cs) {
          if (cs.display === 'none') return ['display-none', node.tagName.toLowerCase()];
          if (cs.visibility === 'hidden') return ['visibility-hidden', node.tagName.toLowerCase()];
        }
        node = node.parentElement;
      }
      return null;
    };

    const focusableSelector =
      'a[href], button, input:not([type=hidden]), select, textarea, [tabindex], [contenteditable=true]';
    const all = document.querySelectorAll(focusableSelector);
    let scanned = 0;
    const offenders = [];
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (inInertContext(el)) continue;
      scanned += 1;
      const found = findHiddenAncestor(el);
      if (!found) continue;
      offenders.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        hideMode: found[0],
        hidingAncestor: found[1],
        tabindex: el.getAttribute('tabindex') || ''
      });
    }

    return {
      totalFocusable: scanned,
      offenders: offenders.slice(0, 50),
      hiddenFocusableCount: offenders.length
    };
})()"##;

/// One hidden-focusable row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct HiddenFocusable {
    /// CSS selector to the offender.
    pub selector: String,
    /// Element tag (lowercased).
    pub tag: String,
    /// `display-none` or `visibility-hidden` — which CSS property
    /// hid the subtree.
    pub hide_mode: String,
    /// Tag of the nearest hiding ancestor.
    pub hiding_ancestor: String,
    /// Value of the `tabindex` attribute, if any.
    pub tabindex: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct DisplayNoneFocusableSnapshot {
    /// Total focusable elements walked (excluding inert-context).
    pub total_focusable: u32,
    /// Top 50 hidden-focusable offenders.
    pub offenders: Vec<HiddenFocusable>,
    /// Total hidden-focusable count (may exceed `offenders.len()`
    /// if truncated).
    pub hidden_focusable_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits one **warn** finding when at least one focusable lives
/// inside a `display: none` or `visibility: hidden` subtree.
/// Severity is warn rather than strict because the modern browser
/// behavior (skip the element when tabbing) makes this mostly a
/// hygiene issue rather than a functional defect — but the
/// stale state often points at a dead JS code path that produces
/// real bugs later.
#[must_use]
pub fn detect_display_none_focusable_issues(
    snap: &DisplayNoneFocusableSnapshot,
) -> Vec<crate::AxisFinding> {
    if snap.offenders.is_empty() {
        return Vec::new();
    }
    let first = &snap.offenders[0];
    let tabindex_note = if first.tabindex.is_empty() {
        String::new()
    } else {
        format!(", tabindex=\"{}\"", first.tabindex)
    };
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Warn,
        kind: "display-none-focusable.hidden-subtree".to_owned(),
        detail: format!(
            "{} focusable element(s) inside hidden subtree(s). Total focusables scanned: {}. First offender: <{}> @ {} (hidden by {} on <{}>{})",
            snap.hidden_focusable_count,
            snap.total_focusable,
            first.tag,
            first.selector,
            first.hide_mode,
            first.hiding_ancestor,
            tabindex_note,
        ),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            DISPLAY_NONE_FOCUSABLE_JS.matches('(').count(),
            DISPLAY_NONE_FOCUSABLE_JS.matches(')').count()
        );
        assert_eq!(
            DISPLAY_NONE_FOCUSABLE_JS.matches('{').count(),
            DISPLAY_NONE_FOCUSABLE_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(DISPLAY_NONE_FOCUSABLE_JS.starts_with("(() => {"));
        assert!(DISPLAY_NONE_FOCUSABLE_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "totalFocusable",
            "offenders",
            "hiddenFocusableCount",
            "selector",
            "tag",
            "hideMode",
            "hidingAncestor",
            "tabindex",
        ] {
            assert!(DISPLAY_NONE_FOCUSABLE_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_focusable_selector_covers_canonical_set() {
        // The selector must include the canonical focusable
        // shapes: a[href], button, input, select, textarea,
        // [tabindex], [contenteditable=true]. input[type=hidden]
        // must be excluded.
        let s = DISPLAY_NONE_FOCUSABLE_JS;
        assert!(s.contains("a[href]"));
        assert!(s.contains("button"));
        assert!(s.contains("input:not([type=hidden])"));
        assert!(s.contains("select"));
        assert!(s.contains("textarea"));
        assert!(s.contains("[tabindex]"));
        assert!(s.contains("[contenteditable=true]"));
    }

    #[test]
    fn js_checks_both_display_none_and_visibility_hidden() {
        let s = DISPLAY_NONE_FOCUSABLE_JS;
        assert!(s.contains("'none'"));
        assert!(s.contains("'hidden'"));
        assert!(s.contains("display-none"));
        assert!(s.contains("visibility-hidden"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 12,
            offenders: vec![],
            hidden_focusable_count: 0,
        };
        let findings = detect_display_none_focusable_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn hidden_focusable_emits_warn() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 4,
            offenders: vec![HiddenFocusable {
                selector: "body > div.modal > button".to_owned(),
                tag: "button".to_owned(),
                hide_mode: "display-none".to_owned(),
                hiding_ancestor: "div".to_owned(),
                tabindex: "0".to_owned(),
            }],
            hidden_focusable_count: 1,
        };
        let findings = detect_display_none_focusable_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert_eq!(findings[0].kind, "display-none-focusable.hidden-subtree");
        assert!(findings[0].detail.contains("hidden subtree"));
        assert!(findings[0].detail.contains("<button>"));
        assert!(findings[0].detail.contains("hidden by display-none"));
        assert!(findings[0].detail.contains("on <div>"));
        assert!(findings[0].detail.contains("tabindex=\"0\""));
    }

    #[test]
    fn visibility_hidden_mode_surfaces() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 1,
            offenders: vec![HiddenFocusable {
                selector: "x".to_owned(),
                tag: "a".to_owned(),
                hide_mode: "visibility-hidden".to_owned(),
                hiding_ancestor: "section".to_owned(),
                tabindex: String::new(),
            }],
            hidden_focusable_count: 1,
        };
        let findings = detect_display_none_focusable_issues(&snap);
        assert!(findings[0].detail.contains("hidden by visibility-hidden"));
        assert!(findings[0].detail.contains("on <section>"));
    }

    #[test]
    fn empty_tabindex_omits_attribute_clause() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 1,
            offenders: vec![HiddenFocusable {
                selector: "x".to_owned(),
                tag: "input".to_owned(),
                hide_mode: "display-none".to_owned(),
                hiding_ancestor: "form".to_owned(),
                tabindex: String::new(),
            }],
            hidden_focusable_count: 1,
        };
        let findings = detect_display_none_focusable_issues(&snap);
        assert!(!findings[0].detail.contains("tabindex="));
    }

    #[test]
    fn truncated_count_surfaces_higher_than_array_len() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 200,
            offenders: vec![HiddenFocusable {
                selector: "x".to_owned(),
                tag: "button".to_owned(),
                hide_mode: "display-none".to_owned(),
                hiding_ancestor: "div".to_owned(),
                tabindex: "0".to_owned(),
            }],
            hidden_focusable_count: 73,
        };
        let findings = detect_display_none_focusable_issues(&snap);
        assert!(findings[0].detail.contains("73 focusable"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 5,
            offenders: vec![HiddenFocusable {
                selector: "body > button".to_owned(),
                tag: "button".to_owned(),
                hide_mode: "display-none".to_owned(),
                hiding_ancestor: "section".to_owned(),
                tabindex: "0".to_owned(),
            }],
            hidden_focusable_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"totalFocusable\":5"));
        assert!(json.contains("\"hiddenFocusableCount\":1"));
        assert!(json.contains("\"hideMode\":\"display-none\""));
        let back: DisplayNoneFocusableSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].hide_mode, "display-none");
    }

    #[test]
    fn detail_includes_total_focusable_count() {
        let snap = DisplayNoneFocusableSnapshot {
            total_focusable: 42,
            offenders: vec![HiddenFocusable {
                selector: "x".to_owned(),
                tag: "button".to_owned(),
                hide_mode: "display-none".to_owned(),
                hiding_ancestor: "div".to_owned(),
                tabindex: "0".to_owned(),
            }],
            hidden_focusable_count: 1,
        };
        let findings = detect_display_none_focusable_issues(&snap);
        assert!(findings[0].detail.contains("Total focusables scanned: 42"));
    }
}
