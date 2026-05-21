//! `empty_paragraph` — `<p>` elements with no visible content.
//!
//! Common CMS-authoring + content-migration bug: editors paste
//! empty paragraphs for vertical spacing (instead of using CSS
//! margin), or a migration script that strips inline styles
//! leaves `<p>` shells behind. Visually invisible, but they:
//!
//! * Inflate the DOM size + page weight.
//! * Confuse screen readers that announce \"paragraph\" on each
//!   one regardless of whether there's content to read.
//! * Break automated content-extraction (LLM summarization,
//!   search indexers) by introducing zero-information separator
//!   nodes mid-stream.
//! * Create accidental focus traps when paired with `tabindex=0`.
//!
//! HEURISTIC
//! ---------
//! 1. Walk every `<p>` in the document.
//! 2. A `<p>` counts as empty when ALL three are true:
//!    a. `textContent.trim()` is the empty string.
//!    b. No child element has a non-trivial visible footprint —
//!       i.e., no `<img>`, `<svg>`, `<video>`, `<audio>`, `<iframe>`,
//!       `<canvas>`, `<picture>`, `<input>`, `<button>`, or
//!       `<select>` descendant. (Inline media inside a `<p>` is
//!       semantically valid; we only flag truly empty `<p>`.)
//!    c. The element is rendered (not `display: none` / inside
//!       `<template>` / inside `<script type=\"text/template\">`).
//! 3. Cap surfaced offenders at 50; full count surfaces in
//!    `emptyCount` regardless.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = \"deny\"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const EMPTY_PARAGRAPH_JS: &str = r##"(() => {
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

    const hasVisibleMediaChild = function(el) {
      const sel = 'img, svg, video, audio, iframe, canvas, picture, input, button, select';
      return el.querySelector(sel) !== null;
    };

    const all = document.querySelectorAll('p');
    let scanned = 0;
    const empties = [];
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (inInertContext(el)) continue;
      scanned += 1;
      const cs = window.getComputedStyle(el);
      if (cs && cs.display === 'none') continue;
      const text = (el.textContent || '').trim();
      if (text !== '') continue;
      if (hasVisibleMediaChild(el)) continue;
      empties.push({
        selector: selectorOf(el),
        hasTabindex: el.hasAttribute('tabindex')
      });
    }

    return {
      totalP: scanned,
      empties: empties.slice(0, 50),
      emptyCount: empties.length
    };
})()"##;

/// One empty-paragraph row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyParagraph {
    /// CSS selector to the offending `<p>`.
    pub selector: String,
    /// Whether the empty `<p>` also carries a `tabindex` attribute —
    /// elevates to a more serious bug (accidental focus stop with no
    /// readable content for assistive tech).
    pub has_tabindex: bool,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyParagraphSnapshot {
    /// Total `<p>` elements walked (excluding inert-context
    /// elements + `display: none` elements).
    pub total_p: u32,
    /// Top 50 empty-paragraph offenders.
    pub empties: Vec<EmptyParagraph>,
    /// Total empty-paragraph count (may exceed `empties.len()` if
    /// truncated).
    pub empty_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits one finding per snapshot that contained at least one
/// empty paragraph. Severity escalates to **strict** when any
/// empty `<p>` also carries a `tabindex` attribute (focus trap
/// with no readable content); otherwise the finding is **warn**.
#[must_use]
pub fn detect_empty_paragraph_issues(snap: &EmptyParagraphSnapshot) -> Vec<crate::AxisFinding> {
    if snap.empties.is_empty() {
        return Vec::new();
    }
    let with_tabindex = snap.empties.iter().filter(|e| e.has_tabindex).count();
    let severity = if with_tabindex > 0 {
        crate::AxisSeverity::Strict
    } else {
        crate::AxisSeverity::Warn
    };
    let first = &snap.empties[0];
    let tabindex_note = if with_tabindex > 0 {
        format!(
            " — {with_tabindex} of these also carry `tabindex` (focus trap with no readable content)"
        )
    } else {
        String::new()
    };
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity,
        kind: "empty-paragraph.no-content".to_owned(),
        detail: format!(
            "{} empty <p> element(s){tabindex_note}. Total <p> scanned: {}. First offender: {}",
            snap.empty_count, snap.total_p, first.selector,
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
            EMPTY_PARAGRAPH_JS.matches('(').count(),
            EMPTY_PARAGRAPH_JS.matches(')').count()
        );
        assert_eq!(
            EMPTY_PARAGRAPH_JS.matches('{').count(),
            EMPTY_PARAGRAPH_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(EMPTY_PARAGRAPH_JS.starts_with("(() => {"));
        assert!(EMPTY_PARAGRAPH_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in ["totalP", "empties", "emptyCount", "selector", "hasTabindex"] {
            assert!(EMPTY_PARAGRAPH_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_walks_p_tag_only() {
        assert!(EMPTY_PARAGRAPH_JS.contains("querySelectorAll('p')"));
    }

    #[test]
    fn js_treats_media_children_as_non_empty() {
        // `<p><img></p>` is NOT empty — the detector must skip
        // paragraphs whose only content is a media descendant.
        assert!(EMPTY_PARAGRAPH_JS.contains("img"));
        assert!(EMPTY_PARAGRAPH_JS.contains("svg"));
        assert!(EMPTY_PARAGRAPH_JS.contains("iframe"));
    }

    #[test]
    fn js_skips_display_none() {
        assert!(EMPTY_PARAGRAPH_JS.contains("display"));
        assert!(EMPTY_PARAGRAPH_JS.contains("'none'"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = EmptyParagraphSnapshot {
            total_p: 12,
            empties: vec![],
            empty_count: 0,
        };
        let findings = detect_empty_paragraph_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn empty_p_without_tabindex_emits_warn() {
        let snap = EmptyParagraphSnapshot {
            total_p: 4,
            empties: vec![EmptyParagraph {
                selector: "body > main > p:nth-of-type(3)".to_owned(),
                has_tabindex: false,
            }],
            empty_count: 1,
        };
        let findings = detect_empty_paragraph_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert_eq!(findings[0].kind, "empty-paragraph.no-content");
        assert!(findings[0].detail.contains("empty <p>"));
        assert!(!findings[0].detail.contains("tabindex"));
    }

    #[test]
    fn empty_p_with_tabindex_escalates_to_strict() {
        let snap = EmptyParagraphSnapshot {
            total_p: 4,
            empties: vec![EmptyParagraph {
                selector: "body > main > p:nth-of-type(3)".to_owned(),
                has_tabindex: true,
            }],
            empty_count: 1,
        };
        let findings = detect_empty_paragraph_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert!(findings[0].detail.contains("tabindex"));
        assert!(findings[0].detail.contains("focus trap"));
    }

    #[test]
    fn mixed_empties_escalate_only_when_any_tabindex_present() {
        // Two empties, one carrying tabindex — the whole finding
        // escalates to strict because even one focus trap is too
        // many.
        let snap = EmptyParagraphSnapshot {
            total_p: 10,
            empties: vec![
                EmptyParagraph {
                    selector: "x".to_owned(),
                    has_tabindex: false,
                },
                EmptyParagraph {
                    selector: "y".to_owned(),
                    has_tabindex: true,
                },
            ],
            empty_count: 2,
        };
        let findings = detect_empty_paragraph_issues(&snap);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert!(findings[0].detail.contains("1 of these"));
    }

    #[test]
    fn truncated_count_surfaces_higher_than_array_len() {
        let snap = EmptyParagraphSnapshot {
            total_p: 200,
            empties: vec![EmptyParagraph {
                selector: "x".to_owned(),
                has_tabindex: false,
            }],
            empty_count: 87,
        };
        let findings = detect_empty_paragraph_issues(&snap);
        assert!(findings[0].detail.contains("87 empty"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = EmptyParagraphSnapshot {
            total_p: 5,
            empties: vec![EmptyParagraph {
                selector: "body > p".to_owned(),
                has_tabindex: true,
            }],
            empty_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"totalP\":5"));
        assert!(json.contains("\"emptyCount\":1"));
        assert!(json.contains("\"hasTabindex\":true"));
        let back: EmptyParagraphSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.empties.len(), 1);
        assert!(back.empties[0].has_tabindex);
    }

    #[test]
    fn detail_includes_total_p_count() {
        let snap = EmptyParagraphSnapshot {
            total_p: 42,
            empties: vec![EmptyParagraph {
                selector: "x".to_owned(),
                has_tabindex: false,
            }],
            empty_count: 1,
        };
        let findings = detect_empty_paragraph_issues(&snap);
        assert!(findings[0].detail.contains("Total <p> scanned: 42"));
    }
}
