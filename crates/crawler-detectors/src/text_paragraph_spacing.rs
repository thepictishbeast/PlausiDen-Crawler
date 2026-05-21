//! `text_paragraph_spacing` — WCAG 1.4.12 paragraph-spacing
//! audit.
//!
//! Sibling axis to `text_line_height` (line-height dimension)
//! and `text_letter_spacing` (letter-spacing dimension) of
//! WCAG 1.4.12 Text Spacing. SC 1.4.12 has four dimensions;
//! this axis covers paragraph-spacing.
//!
//! ## WCAG 1.4.12 paragraph-spacing
//!
//! Per WCAG 2.1 SC 1.4.12 (Level AA), paragraph spacing MUST
//! support an override to ≥ 2× the font-size of the paragraph.
//! "Paragraph spacing" here means the visual gap between two
//! consecutive paragraphs — typically `margin-block-end` on the
//! first paragraph plus `margin-block-start` on the second
//! (collapsing margins).
//!
//! Operators ship `margin-top: 0` / `margin-bottom: 0` on `<p>`
//! to satisfy a designer's wireframe — then body copy turns
//! into a wall of indistinguishable text. Low-vision readers,
//! dyslexic readers, and AT users all lose the visual paragraph
//! boundary signal.
//!
//! The override-support test is hard to run statically. This
//! detector covers the more practical observable: paragraphs
//! that ALREADY render with paragraph-spacing < 2× font-size in
//! the authored design are flagged as suspect — the user's
//! WCAG override can only ADD margin, not subtract, so if the
//! authored design is already tight, the override has to fight
//! collapsing margins and rarely wins.
//!
//! ## Findings
//!
//! * `text-spacing.paragraph-spacing-too-tight` strict — a body
//!   `<p>` element's effective vertical gap to its next sibling
//!   `<p>` is < 2× the font-size of the smaller paragraph.
//!   Only flagged when both paragraphs hold body-text length
//!   (>= 80 chars combined).
//! * `text-spacing.paragraph-spacing-zero` strict — a body
//!   `<p>` has 0 margin-bottom AND its next sibling `<p>` has 0
//!   margin-top, collapsing entirely. Surfaced separately
//!   because it's the more egregious case (zero gap forces the
//!   reader to use line-break inference).
//!
//! Out of scope:
//!
//! * Line-height, letter-spacing, word-spacing — separate axes.
//! * Non-`<p>` paragraph constructs (multi-line `<div>` /
//!   `<span>` text). Future axis can extend.
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

/// WCAG 1.4.12 paragraph spacing minimum (× smaller font-size).
const WCAG_MIN_RATIO: f64 = 2.0;

/// Minimum combined character length of a paragraph pair to be
/// considered body copy.
const MIN_PAIR_TEXT_LENGTH: usize = 80;

/// One observed consecutive paragraph pair (current `<p>` and
/// its next-sibling `<p>`) with the computed margins between
/// them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ParagraphPairEntry {
    /// CSS-ish selector pointing at the first paragraph of
    /// the pair.
    pub selector: String,
    /// `margin-block-end` of the first paragraph in CSS pixels.
    pub end_margin_px: f64,
    /// `margin-block-start` of the next paragraph in CSS
    /// pixels.
    pub start_margin_px: f64,
    /// Smaller of the two font sizes (CSS pixels). Detector
    /// uses the smaller to compute the ratio conservatively.
    pub smaller_font_size_px: f64,
    /// Combined character length of both paragraphs.
    pub combined_text_length: usize,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextParagraphSpacingSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every observed consecutive `<p>` pair on the page.
    pub pairs: Vec<ParagraphPairEntry>,
}

/// Detector.
#[must_use]
pub fn detect_text_paragraph_spacing(
    snap: &TextParagraphSpacingSnapshot,
) -> Vec<AxisFinding> {
    let mut too_tight: Vec<String> = Vec::new();
    let mut zero: Vec<String> = Vec::new();

    for pair in &snap.pairs {
        if pair.combined_text_length < MIN_PAIR_TEXT_LENGTH {
            continue;
        }
        if pair.smaller_font_size_px <= 0.0 {
            continue;
        }
        // Collapsing margins: the effective gap is max(end,
        // start), not the sum.
        let effective_gap = pair.end_margin_px.max(pair.start_margin_px);
        if effective_gap == 0.0 {
            zero.push(format!(
                "{} (end_margin={:.1}px, start_margin={:.1}px, font_size={:.1}px)",
                pair.selector,
                pair.end_margin_px,
                pair.start_margin_px,
                pair.smaller_font_size_px
            ));
            continue;
        }
        let ratio = effective_gap / pair.smaller_font_size_px;
        if ratio >= WCAG_MIN_RATIO {
            continue;
        }
        too_tight.push(format!(
            "{} (gap={:.1}px, font_size={:.1}px, ratio={:.2})",
            pair.selector,
            effective_gap,
            pair.smaller_font_size_px,
            ratio
        ));
    }

    let mut findings = Vec::new();
    let total = snap.pairs.len();

    if !zero.is_empty() {
        let preview =
            preview_examples(&zero.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "text-spacing.paragraph-spacing-zero".to_owned(),
            detail: format!(
                "{} of {} consecutive <p> pair(s) have zero effective vertical gap. Examples: {}",
                zero.len(),
                total,
                preview
            ),
        });
    }

    if !too_tight.is_empty() {
        let preview =
            preview_examples(&too_tight.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "text-spacing.paragraph-spacing-too-tight".to_owned(),
            detail: format!(
                "{} of {} consecutive <p> pair(s) have effective gap < 2× smaller-paragraph font-size (WCAG 1.4.12). Examples: {}",
                too_tight.len(),
                total,
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

/// Page-side eval. Walks every consecutive pair of `<p>`
/// elements that share a parent. Captures the margins between
/// them.
pub const TEXT_PARAGRAPH_SPACING_JS: &str = r##"(() => {
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

    const parsePx = function(s) {
      const m = String(s).match(/^(-?[\d.]+)\s*px$/);
      if (!m) return null;
      const n = parseFloat(m[1]);
      return Number.isFinite(n) ? n : null;
    };

    const paragraphs = Array.from(document.querySelectorAll('p'));
    const pairs = [];
    paragraphs.forEach(function(p) {
      const next = p.nextElementSibling;
      if (!next || next.tagName !== 'P') return;
      // Both must be visible.
      const cs1 = getComputedStyle(p);
      const cs2 = getComputedStyle(next);
      if (cs1.display === 'none' || cs1.visibility === 'hidden') return;
      if (cs2.display === 'none' || cs2.visibility === 'hidden') return;
      const fs1 = parsePx(cs1.fontSize);
      const fs2 = parsePx(cs2.fontSize);
      if (fs1 === null || fs2 === null) return;
      const endM = parsePx(cs1.marginBottom);
      const startM = parsePx(cs2.marginTop);
      if (endM === null || startM === null) return;
      const t1 = (p.textContent || '').trim().length;
      const t2 = (next.textContent || '').trim().length;
      pairs.push({
        selector: selectorOf(p),
        endMarginPx: endM,
        startMarginPx: startM,
        smallerFontSizePx: Math.min(fs1, fs2),
        combinedTextLength: t1 + t2
      });
    });

    return {
      pageUrl: location.href,
      pairs: pairs
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(
        sel: &str,
        end: f64,
        start: f64,
        fs: f64,
        text_len: usize,
    ) -> ParagraphPairEntry {
        ParagraphPairEntry {
            selector: sel.to_owned(),
            end_margin_px: end,
            start_margin_px: start,
            smaller_font_size_px: fs,
            combined_text_length: text_len,
        }
    }

    fn snap(pairs: Vec<ParagraphPairEntry>) -> TextParagraphSpacingSnapshot {
        TextParagraphSpacingSnapshot {
            page_url: "https://example.test/".to_owned(),
            pairs,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_text_paragraph_spacing(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn paragraph_pair_with_2x_gap_is_clean() {
        // 16px font; margin-bottom 32px; collapsing margins keep
        // gap at 32px = 2.0×.
        let f = detect_text_paragraph_spacing(&snap(vec![pair(
            "p:nth-of-type(1)",
            32.0,
            16.0,
            16.0,
            200,
        )]));
        assert!(f.is_empty(), "2.0× should pass: {f:?}");
    }

    #[test]
    fn zero_gap_is_strict() {
        let f = detect_text_paragraph_spacing(&snap(vec![pair(
            "p:nth-of-type(1)",
            0.0,
            0.0,
            16.0,
            200,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.paragraph-spacing-zero")
            .expect("zero gap expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        // Too-tight NOT emitted (zero is a separate finding).
        assert!(!f
            .iter()
            .any(|x| x.kind == "text-spacing.paragraph-spacing-too-tight"));
    }

    #[test]
    fn tight_gap_below_2x_is_strict() {
        // 16px font, 16px gap → 1.0×.
        let f = detect_text_paragraph_spacing(&snap(vec![pair(
            "p:nth-of-type(1)",
            16.0,
            0.0,
            16.0,
            200,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.paragraph-spacing-too-tight")
            .expect("too-tight expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("ratio=1.00"));
    }

    #[test]
    fn collapsing_margins_use_max_not_sum() {
        // end=20px, start=12px → effective gap = max(20, 12) = 20.
        // At 16px font → 20/16 = 1.25 → too tight.
        let f = detect_text_paragraph_spacing(&snap(vec![pair(
            "p#a",
            20.0,
            12.0,
            16.0,
            200,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.paragraph-spacing-too-tight")
            .expect("too-tight expected");
        assert!(hit.detail.contains("gap=20.0px"));
        assert!(hit.detail.contains("ratio=1.25"));
    }

    #[test]
    fn short_pair_below_threshold_ignored() {
        // 40 chars combined — below MIN_PAIR_TEXT_LENGTH.
        let f = detect_text_paragraph_spacing(&snap(vec![pair(
            "p#a", 0.0, 0.0, 16.0, 40,
        )]));
        assert!(f.is_empty(), "below threshold should ignore: {f:?}");
    }

    #[test]
    fn zero_font_size_skipped() {
        let f = detect_text_paragraph_spacing(&snap(vec![pair(
            "p#a", 0.0, 0.0, 0.0, 200,
        )]));
        assert!(f.is_empty(), "zero font-size should skip: {f:?}");
    }

    #[test]
    fn multiple_zero_gaps_share_bucket() {
        let f = detect_text_paragraph_spacing(&snap(vec![
            pair("p#a", 0.0, 0.0, 16.0, 200),
            pair("p#b", 0.0, 0.0, 16.0, 200),
            pair("p#c", 0.0, 0.0, 16.0, 200),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.paragraph-spacing-zero")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let pairs: Vec<_> = (0..8)
            .map(|i| pair(&format!("p#x{i}"), 0.0, 0.0, 16.0, 200))
            .collect();
        let f = detect_text_paragraph_spacing(&snap(pairs));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.paragraph-spacing-zero")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_p_pairs() {
        assert!(TEXT_PARAGRAPH_SPACING_JS.starts_with("(() => {"));
        assert!(TEXT_PARAGRAPH_SPACING_JS.ends_with(")()"));
        assert!(TEXT_PARAGRAPH_SPACING_JS.contains("nextElementSibling"));
        assert!(TEXT_PARAGRAPH_SPACING_JS.contains("marginBottom"));
        assert!(TEXT_PARAGRAPH_SPACING_JS.contains("marginTop"));
    }
}
