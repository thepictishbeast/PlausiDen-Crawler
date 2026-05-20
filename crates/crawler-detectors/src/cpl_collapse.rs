//! `cpl_collapse` — flags text elements whose computed bounding box
//! is too narrow to fit a readable number of characters per line.
//!
//! Defect class: text in a narrow column (sidebar, mobile breakpoint,
//! flex-shrinkable card) gets squeezed to 1-2 chars per visible line,
//! producing unreadable rivers. Common causes:
//!
//! 1. `flex: 1` on a child whose `min-width: auto` is computed from
//!    its longest inline-block descendant, then the flex container
//!    shrinks below that floor — descendants overflow horizontally
//!    and the parent forces `word-break: break-word`-shaped wrapping.
//!
//! 2. `word-break: break-all` accidentally inherited from an
//!    ancestor (e.g. a `pre` element ancestor whose styles cascade
//!    into a misnested text block).
//!
//! 3. Sidebar / aside columns sized in `vw` units that collapse at
//!    narrow viewports — the column survives but its text becomes
//!    1-2 chars wide.
//!
//! 4. `column-count: N` on a too-narrow container — multi-column
//!    layouts splinter when the container can't accommodate them.
//!
//! Mirrors `gradient_text_clip`'s shape — typed result struct,
//! pure detector function, paired DOM-capture JS for the
//! chromiumoxide runner.
//!
//! ## Heuristic
//!
//! For each text-bearing element with `text_len > 20` chars:
//!
//! ```text
//! estimated_cpl = rect_width / (font_size * 0.55)
//! ```
//!
//! Where 0.55 is the empirical average character-width ratio for
//! proportional Latin display fonts (same constant
//! `gradient_text_clip` uses).
//!
//! Severity tiers:
//!
//! * `estimated_cpl < 3` on text-bearing element ⇒ **Strict**
//!   (severe collapse, unreadable rivers).
//! * `estimated_cpl < 8` on text-bearing element ⇒ **Warn**
//!   (cramped, poor reading experience).
//! * `white-space: nowrap` on text > 40 chars in container < 200px
//!   ⇒ **Warn** (will overflow but not vertically collapse).
//!
//! Skip pure-decorative elements (`aria-hidden="true"`,
//! `role="presentation"`) and pre/code blocks (preserved-whitespace
//! is intentional).
//!
//! ## Severity
//!
//! * `strict` — `estimated_cpl < 3` (text is unreadable rivers).
//! * `warn` — `estimated_cpl < 8` OR nowrap-overflow case.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a text element whose computed bounding
/// box is too narrow to fit a readable line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CplCollapseHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped at 80 chars) for context.
    pub text: String,
    /// Character count of the visible text.
    pub char_count: u32,
    /// Bounding-box width in CSS px.
    pub rect_width: u32,
    /// Computed font-size in CSS px.
    pub font_size: u32,
    /// Estimated characters per line:
    /// `rect_width / (font_size * 0.55)`. Lower = worse collapse.
    pub estimated_cpl: f32,
    /// True iff the element's computed `white-space` is `nowrap`
    /// (text-overflow case).
    pub nowrap: bool,
    /// True iff `word-break: break-all` is in effect.
    pub break_all: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CplCollapseSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every element with text > 20 chars whose computed
    /// estimated_cpl < 8 OR nowrap-overflow shape applies.
    pub hits: Vec<CplCollapseHit>,
    /// Total text-bearing elements walked.
    pub scanned_elements: u32,
}

/// Empirical character-width ratio (relative to font-size) for
/// proportional Latin display fonts. Matches the constant
/// `gradient_text_clip` uses for consistency.
pub const CHAR_WIDTH_RATIO: f32 = 0.55;

/// CPL below this triggers a Strict finding (severe collapse,
/// reads as unreadable rivers).
pub const STRICT_CPL_THRESHOLD: f32 = 3.0;

/// CPL below this triggers a Warn finding (cramped, poor reading
/// experience).
pub const WARN_CPL_THRESHOLD: f32 = 8.0;

/// Minimum text length we consider for CPL analysis. Short labels
/// (button text, single-word eyebrows) legitimately appear in
/// narrow containers and aren't the defect class this detects.
pub const MIN_TEXT_LEN_FOR_CPL: u32 = 20;

/// Estimate characters per line from rect width + font size.
/// Pure function; exposed so callers can replay the math
/// without round-tripping through the snapshot.
#[must_use]
pub fn estimate_cpl(rect_width: u32, font_size: u32) -> f32 {
    if font_size == 0 {
        return 0.0;
    }
    rect_width as f32 / (font_size as f32 * CHAR_WIDTH_RATIO)
}

/// Pure detector: snapshot → findings. Partitions hits into severe
/// collapse (Strict, CPL < 3), cramped (Warn, CPL < 8), and
/// nowrap-overflow (Warn, separate kind for filterability).
/// Examples capped at 5 per finding to keep reports readable.
#[must_use]
pub fn detect_cpl_collapse(snap: &CplCollapseSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let mut severe: Vec<&CplCollapseHit> = Vec::new();
    let mut cramped: Vec<&CplCollapseHit> = Vec::new();
    let mut overflow: Vec<&CplCollapseHit> = Vec::new();

    for hit in &snap.hits {
        if hit.char_count < MIN_TEXT_LEN_FOR_CPL {
            continue;
        }
        if hit.nowrap && hit.rect_width < 200 {
            overflow.push(hit);
            continue;
        }
        if hit.estimated_cpl < STRICT_CPL_THRESHOLD {
            severe.push(hit);
        } else if hit.estimated_cpl < WARN_CPL_THRESHOLD {
            cramped.push(hit);
        }
    }

    let mut out = Vec::new();

    if !severe.is_empty() {
        let examples: Vec<String> = severe
            .iter()
            .take(5)
            .map(|h| {
                format!(
                    "{} (\"{}\", {}c → {:.1} cpl in {}px @ {}px font{}{})",
                    h.selector,
                    h.text,
                    h.char_count,
                    h.estimated_cpl,
                    h.rect_width,
                    h.font_size,
                    if h.break_all { " break-all" } else { "" },
                    if h.nowrap { " nowrap" } else { "" }
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "cpl-collapse.severe".to_owned(),
            detail: format!(
                "{} text element(s) with < {:.0} chars/line at viewport {}px — reads as unreadable rivers. Widen the container, reduce font-size, or remove word-break:break-all. Examples: {}",
                severe.len(),
                STRICT_CPL_THRESHOLD,
                snap.viewport_width,
                examples.join("; ")
            ),
        });
    }

    if !cramped.is_empty() {
        let examples: Vec<String> = cramped
            .iter()
            .take(5)
            .map(|h| {
                format!(
                    "{} (\"{}\", {:.1} cpl, {}px wide)",
                    h.selector, h.text, h.estimated_cpl, h.rect_width
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cpl-collapse.cramped".to_owned(),
            detail: format!(
                "{} text element(s) with {:.0}-{:.0} chars/line at viewport {}px — cramped reading experience. Consider widening the container or reducing font-size. Examples: {}",
                cramped.len(),
                STRICT_CPL_THRESHOLD,
                WARN_CPL_THRESHOLD,
                snap.viewport_width,
                examples.join("; ")
            ),
        });
    }

    if !overflow.is_empty() {
        let examples: Vec<String> = overflow
            .iter()
            .take(5)
            .map(|h| {
                format!(
                    "{} (\"{}\", {}c in {}px nowrap)",
                    h.selector, h.text, h.char_count, h.rect_width
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cpl-collapse.nowrap-overflow".to_owned(),
            detail: format!(
                "{} text element(s) with white-space:nowrap in a container < 200px wide — text will overflow horizontally or get truncated. Either drop the nowrap or widen the container. Examples: {}",
                overflow.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

/// Browser-side DOM-capture script. Pinned for the future
/// chromiumoxide path; mirror any change in this file's
/// `CplCollapseHit` + snapshot fields.
pub const CPL_COLLAPSE_DOM_CAPTURE_JS: &str = r#"
(() => {
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

    const isExcluded = function(el) {
      if (!el) return true;
      if (el.getAttribute && (el.getAttribute('aria-hidden') === 'true' || el.getAttribute('role') === 'presentation')) return true;
      const tag = el.tagName ? el.tagName.toLowerCase() : '';
      if (tag === 'pre' || tag === 'code' || tag === 'kbd' || tag === 'samp') return true;
      let p = el.parentElement;
      while (p) {
        const ptag = p.tagName ? p.tagName.toLowerCase() : '';
        if (ptag === 'pre' || ptag === 'code') return true;
        p = p.parentElement;
      }
      return false;
    };

    const CHAR_WIDTH_RATIO = 0.55;
    const MIN_TEXT_LEN = 20;
    const WARN_CPL = 8.0;
    const hits = [];
    let scanned = 0;
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    let node = walk.currentNode;
    while (node) {
      if (node.nodeType === 1 && !isExcluded(node)) {
        const text = (node.textContent || '').trim();
        if (text.length >= MIN_TEXT_LEN) {
          scanned += 1;
          const rect = node.getBoundingClientRect();
          if (rect.width > 0) {
            const cs = window.getComputedStyle(node);
            const fontSize = parseFloat(cs.fontSize) || 16;
            const cpl = rect.width / (fontSize * CHAR_WIDTH_RATIO);
            const nowrap = cs.whiteSpace === 'nowrap';
            const breakAll = cs.wordBreak === 'break-all';
            const isCollapsed = cpl < WARN_CPL;
            const isOverflow = nowrap && rect.width < 200;
            if (isCollapsed || isOverflow) {
              hits.push({
                selector: selectorOf(node),
                text: text.substring(0, 80),
                charCount: text.length,
                rectWidth: Math.round(rect.width),
                fontSize: Math.round(fontSize),
                estimatedCpl: Math.round(cpl * 100) / 100,
                nowrap: nowrap,
                breakAll: breakAll
              });
            }
          }
        }
      }
      node = walk.nextNode();
    }
    return {
      pageUrl: location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedElements: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(text: &str, width: u32, font: u32, nowrap: bool) -> CplCollapseHit {
        CplCollapseHit {
            selector: "body > div".to_owned(),
            text: text.to_owned(),
            char_count: text.len() as u32,
            rect_width: width,
            font_size: font,
            estimated_cpl: estimate_cpl(width, font),
            nowrap,
            break_all: false,
        }
    }

    #[test]
    fn estimate_cpl_known_values() {
        // 220px / (16 * 0.55) = 220 / 8.8 = 25.0
        assert!((estimate_cpl(220, 16) - 25.0).abs() < 0.01);
        // 32px / (16 * 0.55) = 32 / 8.8 ≈ 3.636 — just above strict
        assert!(estimate_cpl(32, 16) > STRICT_CPL_THRESHOLD);
        // 24px / (16 * 0.55) ≈ 2.73 — below strict
        assert!(estimate_cpl(24, 16) < STRICT_CPL_THRESHOLD);
        // Zero font size yields zero (no panic)
        assert_eq!(estimate_cpl(100, 0), 0.0);
    }

    #[test]
    fn detect_empty_snapshot_yields_no_findings() {
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits: Vec::new(),
            scanned_elements: 0,
        };
        assert!(detect_cpl_collapse(&snap).is_empty());
    }

    #[test]
    fn detect_severe_collapse_emits_strict() {
        // 24px / 8.8 ≈ 2.73 cpl on a 30-char text
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits: vec![hit(
                "This is a sentence that definitely has more than 20 chars",
                24,
                16,
                false,
            )],
            scanned_elements: 1,
        };
        let findings = detect_cpl_collapse(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "cpl-collapse.severe");
    }

    #[test]
    fn detect_cramped_collapse_emits_warn() {
        // 50px / 8.8 ≈ 5.68 cpl on a 30-char text — cramped
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 768,
            hits: vec![hit(
                "This is a sentence that definitely has more than 20 chars",
                50,
                16,
                false,
            )],
            scanned_elements: 1,
        };
        let findings = detect_cpl_collapse(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "cpl-collapse.cramped");
    }

    #[test]
    fn detect_nowrap_overflow_emits_separate_warn() {
        // 150px wide container, nowrap, long text
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 768,
            hits: vec![hit(
                "Some long text that will overflow horizontally",
                150,
                16,
                true,
            )],
            scanned_elements: 1,
        };
        let findings = detect_cpl_collapse(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "cpl-collapse.nowrap-overflow");
    }

    #[test]
    fn detect_skips_text_below_min_length() {
        // 24px / 8.8 ≈ 2.73 cpl but text only 10 chars — skip
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits: vec![hit("Short text", 24, 16, false)],
            scanned_elements: 1,
        };
        assert!(detect_cpl_collapse(&snap).is_empty());
    }

    #[test]
    fn detect_partitions_into_three_kinds() {
        // One severe, one cramped, one nowrap-overflow — three findings
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits: vec![
                hit(
                    "Severe collapse text with more than twenty characters",
                    24,
                    16,
                    false,
                ),
                hit(
                    "Cramped collapse text with more than twenty characters",
                    50,
                    16,
                    false,
                ),
                hit(
                    "Nowrap overflow text with more than twenty characters",
                    150,
                    16,
                    true,
                ),
            ],
            scanned_elements: 3,
        };
        let findings = detect_cpl_collapse(&snap);
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"cpl-collapse.severe"));
        assert!(kinds.contains(&"cpl-collapse.cramped"));
        assert!(kinds.contains(&"cpl-collapse.nowrap-overflow"));
    }

    #[test]
    fn detect_caps_examples_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!("Severe text item number {i} with more than twenty chars"),
                24,
                16,
                false,
            ));
        }
        let snap = CplCollapseSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits,
            scanned_elements: 10,
        };
        let findings = detect_cpl_collapse(&snap);
        assert_eq!(findings.len(), 1);
        // The finding reports the total (10) but the example list
        // caps at 5; both numbers appear in the detail string.
        assert!(findings[0].detail.contains("10 text element(s)"));
        let semicolons = findings[0].detail.matches(';').count();
        assert_eq!(semicolons, 4, "5 examples joined by 4 semicolons");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: the JS body is a self-invoking function returning a
        // shape with the four documented fields. We assert syntactic
        // signatures only — no JS execution in unit tests.
        assert!(CPL_COLLAPSE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(CPL_COLLAPSE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(CPL_COLLAPSE_DOM_CAPTURE_JS.contains("hits"));
        assert!(CPL_COLLAPSE_DOM_CAPTURE_JS.contains("scannedElements"));
        assert!(CPL_COLLAPSE_DOM_CAPTURE_JS.contains("CHAR_WIDTH_RATIO = 0.55"));
        assert!(CPL_COLLAPSE_DOM_CAPTURE_JS.contains("MIN_TEXT_LEN = 20"));
    }
}
