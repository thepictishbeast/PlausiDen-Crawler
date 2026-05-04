//! `heading_order` — heading hierarchy detector.
//!
//! WCAG 1.3.1 (Info and Relationships) requires that heading
//! structure be programmatically determinable AND meaningful.
//! Two violations this detector catches:
//!
//! 1. **Wrong h1 count.** A document has exactly ONE `<h1>` (the
//!    main page heading). Zero or multiple confuse screen-reader
//!    nav and SERP indexing.
//! 2. **Level skipping.** Headings should not jump levels (h2
//!    followed by h4 without h3 between). Skip indicates either
//!    missing intermediate structure or a developer using h-tags
//!    for visual size instead of semantic structure.
//!
//! What the detector does NOT enforce (out of scope):
//!   * Heading text quality / uniqueness
//!   * Heading-to-content correspondence
//!   * h1 == document title
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on enums.
//! * Pure functions (`detect_heading_order_issues`) take the typed
//!   snapshot and return findings — no I/O, no globals.

use serde::{Deserialize, Serialize};

/// Page-side eval. Walks every `<h1>` through `<h6>` in DOM
/// order, returns level + text + best-effort selector.
pub const HEADING_ORDER_JS: &str = r##"(() => {
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

    const headings = [];
    const all = document.querySelectorAll('h1, h2, h3, h4, h5, h6');
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      const level = parseInt(el.tagName.substring(1), 10);
      const text = (el.textContent || '').trim().slice(0, 80);
      headings.push({
        level: level,
        text: text,
        selector: selectorOf(el)
      });
    }
    return {
      pageUrl: window.location.href,
      headings: headings,
    };
})()"##;

/// One heading captured from the DOM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedHeading {
    /// 1-6 (the h-level).
    pub level: u8,
    /// First 80 chars of textContent.
    pub text: String,
    /// Best-effort CSS selector.
    pub selector: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadingOrderSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// Headings in DOM order.
    pub headings: Vec<CapturedHeading>,
}

/// Apply detection rules to a heading-order snapshot. Pure
/// function. Returns `AxisFinding`s for every violation.
#[must_use]
pub fn detect_heading_order_issues(
    snap: &HeadingOrderSnapshot,
) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    // Rule 1: exactly one h1.
    let h1_count = snap.headings.iter().filter(|h| h.level == 1).count();
    if h1_count == 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "headings.no-h1".to_owned(),
            detail: "Document has no <h1>. Screen-reader page nav and SERP both rely on a top-level heading; pages without one announce as 'untitled'.".to_owned(),
        });
    } else if h1_count > 1 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "headings.multiple-h1".to_owned(),
            detail: format!(
                "Document has {h1_count} <h1> elements; should have exactly 1. Multiple top-level headings break document outline + screen-reader navigation."
            ),
        });
    }

    // Rule 2: no level skipping.
    let mut prev_level: Option<u8> = None;
    for h in &snap.headings {
        if let Some(prev) = prev_level {
            // Skips down by more than 1 (h2 → h4, h2 → h5, etc.).
            // Going UP any number of levels (h4 → h2) is fine —
            // closing a section is unconstrained.
            if h.level > prev + 1 {
                out.push(crate::AxisFinding {
                    severity: crate::AxisSeverity::Warn,
                    kind: "headings.level-skip".to_owned(),
                    detail: format!(
                        "Heading skips from h{prev} to h{}: '{}'. Insert the intermediate level(s) for accessible document outline.",
                        h.level,
                        h.text,
                    ),
                });
            }
        }
        prev_level = Some(h.level);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_balanced() {
        assert_eq!(
            HEADING_ORDER_JS.matches('(').count(),
            HEADING_ORDER_JS.matches(')').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(HEADING_ORDER_JS.starts_with("(() => {"));
        assert!(HEADING_ORDER_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in ["pageUrl", "headings"] {
            assert!(HEADING_ORDER_JS.contains(k), "missing key: {k}");
        }
    }

    fn h(level: u8, text: &str) -> CapturedHeading {
        CapturedHeading {
            level,
            text: text.to_owned(),
            selector: format!("h{level}"),
        }
    }

    fn snap(headings: Vec<CapturedHeading>) -> HeadingOrderSnapshot {
        HeadingOrderSnapshot {
            page_url: "http://t/".to_owned(),
            headings,
        }
    }

    #[test]
    fn one_h1_with_proper_descent_passes() {
        let s = snap(vec![
            h(1, "Page title"),
            h(2, "Section A"),
            h(3, "Subsection"),
            h(2, "Section B"),
        ]);
        assert!(detect_heading_order_issues(&s).is_empty());
    }

    #[test]
    fn no_h1_fires_strict() {
        let s = snap(vec![h(2, "Floating section"), h(3, "Sub")]);
        let f = detect_heading_order_issues(&s);
        assert!(f.iter().any(|x| x.kind == "headings.no-h1"));
        assert!(f.iter().any(|x| x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn empty_doc_fires_no_h1() {
        let s = snap(vec![]);
        let f = detect_heading_order_issues(&s);
        assert!(f.iter().any(|x| x.kind == "headings.no-h1"));
    }

    #[test]
    fn multiple_h1_fires_strict() {
        let s = snap(vec![
            h(1, "First"),
            h(2, "x"),
            h(1, "Second"),
            h(2, "y"),
        ]);
        let f = detect_heading_order_issues(&s);
        assert!(f.iter().any(|x| x.kind == "headings.multiple-h1"));
    }

    #[test]
    fn h2_to_h4_skip_fires_warn() {
        let s = snap(vec![h(1, "Title"), h(2, "Section"), h(4, "Skip!")]);
        let f = detect_heading_order_issues(&s);
        let skip = f.iter().find(|x| x.kind == "headings.level-skip");
        let skip = skip.expect("should fire skip");
        assert_eq!(skip.severity, crate::AxisSeverity::Warn);
        assert!(skip.detail.contains("h2 to h4"));
    }

    #[test]
    fn h1_to_h3_skip_fires_warn() {
        let s = snap(vec![h(1, "Title"), h(3, "Skip!")]);
        let f = detect_heading_order_issues(&s);
        assert!(f.iter().any(|x| x.kind == "headings.level-skip"));
    }

    #[test]
    fn going_up_levels_is_fine() {
        // h1 → h2 → h3 → h4 then back to h2 is fine.
        let s = snap(vec![
            h(1, "T"),
            h(2, "A"),
            h(3, "A.1"),
            h(4, "A.1.a"),
            h(2, "B"),
        ]);
        assert!(detect_heading_order_issues(&s).is_empty());
    }

    #[test]
    fn consecutive_same_level_is_fine() {
        let s = snap(vec![
            h(1, "T"),
            h(2, "A"),
            h(2, "B"),
            h(2, "C"),
        ]);
        assert!(detect_heading_order_issues(&s).is_empty());
    }

    #[test]
    fn skip_emits_text_in_detail() {
        let s = snap(vec![h(1, "T"), h(3, "Some heading text")]);
        let f = detect_heading_order_issues(&s);
        assert!(f[0].detail.contains("Some heading text"));
    }

    #[test]
    fn snapshot_round_trips() {
        let s = snap(vec![h(1, "A"), h(2, "B")]);
        let json = serde_json::to_string(&s).expect("ser");
        let back: HeadingOrderSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.headings.len(), 2);
        assert_eq!(back.headings[0].level, 1);
    }
}
