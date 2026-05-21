//! `list_item_outside_list` — `<li>` elements without a list parent.
//!
//! HTML semantics require `<li>` to live inside `<ul>`, `<ol>`, or
//! `<menu>`. CMS authoring commonly produces orphan `<li>` elements
//! when an editor copy-pastes a marker glyph into a paragraph or
//! div — the rendered visual looks list-like (the bullet is just
//! a `::marker` pseudo-element) but screen readers receive no
//! "list of N items" announcement, so the structural intent is
//! lost.
//!
//! WCAG 1.3.1 Info and Relationships (Level A) — the relationship
//! must be programmatically determinable. An orphan `<li>` fails
//! this because the list relationship is absent from the
//! accessibility tree.
//!
//! HEURISTIC
//! ---------
//! 1. Walk every `<li>` in the document.
//! 2. For each, check its parent: if `parentElement.tagName` is
//!    NOT `UL`, `OL`, or `MENU`, flag as orphan.
//! 3. `<template>` content + `<script type="text/template">` are
//!    excluded — inert markup that never reaches a user agent.
//! 4. Cap reported offenders at 50; total count surfaces in the
//!    snapshot regardless.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const LIST_ITEM_OUTSIDE_LIST_JS: &str = r##"(() => {
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

    const all = document.querySelectorAll('li');
    let scanned = 0;
    const orphans = [];
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (inInertContext(el)) continue;
      scanned += 1;
      const parent = el.parentElement;
      const parentTag = parent ? parent.tagName : '';
      if (parentTag !== 'UL' && parentTag !== 'OL' && parentTag !== 'MENU') {
        orphans.push({
          selector: selectorOf(el),
          parentTag: parentTag.toLowerCase() || '(none)',
          textPreview: (el.textContent || '').trim().slice(0, 60)
        });
      }
    }

    return {
      totalLi: scanned,
      orphans: orphans.slice(0, 50),
      orphanCount: orphans.length
    };
})()"##;

/// One orphan-li row: an `<li>` whose parent is not a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct OrphanLi {
    /// CSS selector to the offending `<li>`.
    pub selector: String,
    /// The actual parent tag (lowercased) — `div` / `p` / `span` / etc.
    pub parent_tag: String,
    /// First 60 chars of `textContent` for operator identification.
    pub text_preview: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ListItemOutsideListSnapshot {
    /// Total `<li>` elements walked (excluding inert-context
    /// elements inside `<template>` / `<script>`).
    pub total_li: u32,
    /// Top 50 orphan-li offenders.
    pub orphans: Vec<OrphanLi>,
    /// Total orphan count (may exceed `orphans.len()` if truncated).
    pub orphan_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits one strict finding per snapshot that contained at least one
/// orphan `<li>`. Severity is strict because the structural
/// relationship is lost from the accessibility tree — screen-reader
/// users get no "list of N items" announcement.
#[must_use]
pub fn detect_list_item_outside_list_issues(
    snap: &ListItemOutsideListSnapshot,
) -> Vec<crate::AxisFinding> {
    if snap.orphans.is_empty() {
        return Vec::new();
    }
    let first = &snap.orphans[0];
    let preview = if first.text_preview.is_empty() {
        "(empty)".to_owned()
    } else {
        format!("\"{}\"", first.text_preview)
    };
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "list-item-outside-list.orphan".to_owned(),
        detail: format!(
            "{} orphan <li> element(s) — parent is not <ul>/<ol>/<menu>. Total <li> scanned: {}. First offender: <li> inside <{}> @ {} (text {})",
            snap.orphan_count, snap.total_li, first.parent_tag, first.selector, preview,
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
            LIST_ITEM_OUTSIDE_LIST_JS.matches('(').count(),
            LIST_ITEM_OUTSIDE_LIST_JS.matches(')').count()
        );
        assert_eq!(
            LIST_ITEM_OUTSIDE_LIST_JS.matches('{').count(),
            LIST_ITEM_OUTSIDE_LIST_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(LIST_ITEM_OUTSIDE_LIST_JS.starts_with("(() => {"));
        assert!(LIST_ITEM_OUTSIDE_LIST_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "totalLi",
            "orphans",
            "orphanCount",
            "selector",
            "parentTag",
            "textPreview",
        ] {
            assert!(LIST_ITEM_OUTSIDE_LIST_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_walks_li_tag() {
        assert!(LIST_ITEM_OUTSIDE_LIST_JS.contains("querySelectorAll('li')"));
    }

    #[test]
    fn js_accepts_three_list_parents() {
        // The detector must accept UL, OL, AND MENU as legitimate
        // parents — MENU is the underused third one.
        assert!(LIST_ITEM_OUTSIDE_LIST_JS.contains("'UL'"));
        assert!(LIST_ITEM_OUTSIDE_LIST_JS.contains("'OL'"));
        assert!(LIST_ITEM_OUTSIDE_LIST_JS.contains("'MENU'"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = ListItemOutsideListSnapshot {
            total_li: 8,
            orphans: vec![],
            orphan_count: 0,
        };
        let findings = detect_list_item_outside_list_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn orphan_li_emits_strict() {
        let snap = ListItemOutsideListSnapshot {
            total_li: 4,
            orphans: vec![OrphanLi {
                selector: "body > main > div > li".to_owned(),
                parent_tag: "div".to_owned(),
                text_preview: "Free shipping on all orders".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_list_item_outside_list_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "list-item-outside-list.orphan");
        assert!(findings[0].detail.contains("orphan <li>"));
        assert!(findings[0].detail.contains("inside <div>"));
        assert!(findings[0].detail.contains("Free shipping on all orders"));
    }

    #[test]
    fn empty_text_preview_renders_as_placeholder() {
        let snap = ListItemOutsideListSnapshot {
            total_li: 1,
            orphans: vec![OrphanLi {
                selector: "body > p > li".to_owned(),
                parent_tag: "p".to_owned(),
                text_preview: String::new(),
            }],
            orphan_count: 1,
        };
        let findings = detect_list_item_outside_list_issues(&snap);
        assert!(findings[0].detail.contains("(empty)"));
    }

    #[test]
    fn truncated_count_surfaces_higher_than_array_len() {
        // JS caps the orphans list at 50; the finding reports the
        // full count even when the array was truncated.
        let snap = ListItemOutsideListSnapshot {
            total_li: 200,
            orphans: vec![OrphanLi {
                selector: "x".to_owned(),
                parent_tag: "section".to_owned(),
                text_preview: "First".to_owned(),
            }],
            orphan_count: 87,
        };
        let findings = detect_list_item_outside_list_issues(&snap);
        assert!(findings[0].detail.contains("87 orphan"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = ListItemOutsideListSnapshot {
            total_li: 5,
            orphans: vec![OrphanLi {
                selector: "body > div > li".to_owned(),
                parent_tag: "div".to_owned(),
                text_preview: "Anything".to_owned(),
            }],
            orphan_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"totalLi\":5"));
        assert!(json.contains("\"orphanCount\":1"));
        assert!(json.contains("\"parentTag\":\"div\""));
        let back: ListItemOutsideListSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.orphans.len(), 1);
        assert_eq!(back.orphans[0].parent_tag, "div");
    }

    #[test]
    fn detail_includes_total_li_count() {
        let snap = ListItemOutsideListSnapshot {
            total_li: 42,
            orphans: vec![OrphanLi {
                selector: "x".to_owned(),
                parent_tag: "section".to_owned(),
                text_preview: "First".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_list_item_outside_list_issues(&snap);
        assert!(findings[0].detail.contains("Total <li> scanned: 42"));
    }
}
