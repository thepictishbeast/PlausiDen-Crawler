//! `text_line_height` — WCAG 1.4.12 line-height ratio audit.
//!
//! Sibling axis to `runtime_contrast` (color contrast),
//! `resize_text_200pct` (text resize behaviour), and
//! `text_wrap_collapse` (text-wrap fidelity). This detector
//! audits computed line-height against WCAG 1.4.12 Text Spacing.
//!
//! ## WCAG 1.4.12 Text Spacing
//!
//! Per WCAG 2.1 Success Criterion 1.4.12 (Level AA), the
//! following text-spacing values MUST be supported without
//! content loss:
//!
//! * Line height (line spacing) ≥ **1.5×** the font size.
//! * Paragraph spacing ≥ 2× the font size.
//! * Letter spacing ≥ 0.12× the font size.
//! * Word spacing ≥ 0.16× the font size.
//!
//! This detector covers the LINE-HEIGHT requirement on body-text
//! elements. Heading elements (`h1`-`h6`) and short-text elements
//! (button labels, single-line UI text) are exempt by the
//! pragmatic interpretation that the SC targets body copy where
//! tight line-height causes the readability harm.
//!
//! Operators routinely ship `line-height: 1` or `line-height:
//! 1.2` on body text — fine for short labels, hostile for
//! paragraphs.
//!
//! ## Findings
//!
//! * `text-spacing.line-height-too-short` strict —
//!   computed line-height < 1.5× font-size on a body-text
//!   element (`p`, `li`, `div` carrying text, etc.) AND the
//!   text exceeds a short-text threshold (>= 80 chars).
//! * `text-spacing.line-height-very-short` warn —
//!   line-height < 1.5× on a SHORT text node (>= 20 but < 80
//!   chars). Surface but don't gate; many designs intentionally
//!   tighten labels.
//!
//! Out of scope:
//!
//! * Paragraph spacing, letter spacing, word spacing — separate
//!   future axes.
//! * Heading line-height — headings often use tighter line-
//!   height by design (titles); SC 1.4.12 targets body copy.
//! * Computing the ratio when font-size is not pixels — the
//!   runner is expected to normalise to px before snapshot.
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

/// WCAG 1.4.12 line-height minimum (× font-size).
const WCAG_MIN_RATIO: f64 = 1.5;

/// Short-text threshold in characters. Lines below this are
/// downgraded to warn (designer intent often legitimately
/// tightens short labels).
const SHORT_TEXT_THRESHOLD: usize = 80;

/// Floor character count below which the detector ignores the
/// element entirely — single-char nodes, formatting fragments,
/// icon-only text are not body copy.
const MIN_TEXT_LENGTH: usize = 20;

/// One captured text-bearing element with computed-style
/// font-size + line-height + text-content length.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextLineHeightEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Tag name (lowercased: `p`, `li`, `div`, etc.) so the
    /// detector can exempt heading tags.
    pub tag: String,
    /// Computed `font-size` in CSS pixels.
    pub font_size_px: f64,
    /// Computed `line-height` in CSS pixels. `None` when the
    /// runner could not compute (`normal` keyword without a
    /// numeric resolution; bail rather than guess).
    pub line_height_px: Option<f64>,
    /// Length of trimmed text content in characters.
    pub text_length: usize,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextLineHeightSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every text-bearing element observed on the page.
    pub entries: Vec<TextLineHeightEntry>,
}

/// Detector.
#[must_use]
pub fn detect_text_line_height(snap: &TextLineHeightSnapshot) -> Vec<AxisFinding> {
    let mut too_short: Vec<String> = Vec::new();
    let mut very_short: Vec<String> = Vec::new();

    for entry in &snap.entries {
        // Headings exempt — tight title line-height is design intent.
        if is_heading_tag(&entry.tag) {
            continue;
        }
        if entry.text_length < MIN_TEXT_LENGTH {
            continue;
        }
        let Some(lh) = entry.line_height_px else {
            continue;
        };
        if entry.font_size_px <= 0.0 {
            continue;
        }
        let ratio = lh / entry.font_size_px;
        if ratio >= WCAG_MIN_RATIO {
            continue;
        }
        let row = format!(
            "{} (tag={}, font_size={:.1}px, line_height={:.1}px, ratio={:.2}, text_len={})",
            entry.selector,
            entry.tag,
            entry.font_size_px,
            lh,
            ratio,
            entry.text_length
        );
        if entry.text_length >= SHORT_TEXT_THRESHOLD {
            too_short.push(row);
        } else {
            very_short.push(row);
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !too_short.is_empty() {
        let preview = preview_examples(
            &too_short.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "text-spacing.line-height-too-short".to_owned(),
            detail: format!(
                "{} of {} body-text element(s) carry line-height < 1.5× font-size (WCAG 1.4.12). Examples: {}",
                too_short.len(),
                total,
                preview
            ),
        });
    }

    if !very_short.is_empty() {
        let preview = preview_examples(
            &very_short.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "text-spacing.line-height-very-short".to_owned(),
            detail: format!(
                "{} of {} short text node(s) carry line-height < 1.5× font-size; surface but don't gate. Examples: {}",
                very_short.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn is_heading_tag(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
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

/// Page-side eval. Walks visible text-bearing elements,
/// computes font-size + line-height via getComputedStyle.
pub const TEXT_LINE_HEIGHT_JS: &str = r##"(() => {
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
      const m = String(s).match(/^([\d.]+)\s*px$/);
      if (!m) return null;
      const n = parseFloat(m[1]);
      return Number.isFinite(n) ? n : null;
    };

    const candidates = Array.from(document.querySelectorAll('p, li, div, td, th, blockquote, figcaption, dt, dd'));
    const entries = [];
    candidates.forEach(function(el) {
      const hasOwnText = Array.from(el.childNodes).some(function(n) {
        return n.nodeType === 3 && (n.textContent || '').trim().length > 0;
      });
      if (!hasOwnText) return;
      const cs = getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return;
      const fontSize = parsePx(cs.fontSize);
      if (fontSize === null) return;
      const lineHeightPx = parsePx(cs.lineHeight);
      const text = (el.textContent || '').trim();
      entries.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        fontSizePx: fontSize,
        lineHeightPx: lineHeightPx,
        textLength: text.length
      });
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
        fs: f64,
        lh: Option<f64>,
        text_len: usize,
    ) -> TextLineHeightEntry {
        TextLineHeightEntry {
            selector: sel.to_owned(),
            tag: tag.to_owned(),
            font_size_px: fs,
            line_height_px: lh,
            text_length: text_len,
        }
    }

    fn snap(entries: Vec<TextLineHeightEntry>) -> TextLineHeightSnapshot {
        TextLineHeightSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_text_line_height(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn body_text_with_15_ratio_is_clean() {
        // 16px font, 24px line-height → 1.5
        let f = detect_text_line_height(&snap(vec![entry(
            "p", "p", 16.0, Some(24.0), 200,
        )]));
        assert!(f.is_empty(), "ratio 1.5 should pass: {f:?}");
    }

    #[test]
    fn body_text_below_threshold_is_strict() {
        // 16px font, 18px line-height → 1.125 < 1.5
        let f = detect_text_line_height(&snap(vec![entry(
            "article > p:nth-of-type(1)",
            "p",
            16.0,
            Some(18.0),
            250,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.line-height-too-short")
            .expect("too-short expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("ratio=1.12"));
        assert!(hit.detail.contains("p:nth-of-type(1)"));
    }

    #[test]
    fn short_text_below_threshold_is_warn() {
        // 16px font, 16px line-height (ratio 1.0), 30 chars.
        let f = detect_text_line_height(&snap(vec![entry(
            "li", "li", 16.0, Some(16.0), 30,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.line-height-very-short")
            .expect("very-short expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn headings_exempt_from_audit() {
        for h in ["h1", "h2", "h3", "h4", "h5", "h6"] {
            let f = detect_text_line_height(&snap(vec![entry(
                h, h, 32.0, Some(32.0), 200,
            )]));
            assert!(
                f.is_empty(),
                "{h} should be exempt; got {f:?}"
            );
        }
    }

    #[test]
    fn very_short_text_below_min_length_is_ignored() {
        // 10 chars — below MIN_TEXT_LENGTH (20).
        let f = detect_text_line_height(&snap(vec![entry(
            "span", "span", 16.0, Some(16.0), 10,
        )]));
        assert!(f.is_empty(), "below MIN_TEXT_LENGTH should be ignored: {f:?}");
    }

    #[test]
    fn missing_line_height_px_skips_entry() {
        // Runner couldn't compute lh; skip rather than guess.
        let f = detect_text_line_height(&snap(vec![entry(
            "p", "p", 16.0, None, 200,
        )]));
        assert!(f.is_empty(), "None line_height should skip: {f:?}");
    }

    #[test]
    fn zero_font_size_skipped() {
        let f = detect_text_line_height(&snap(vec![entry(
            "p", "p", 0.0, Some(16.0), 200,
        )]));
        assert!(f.is_empty(), "zero font-size should skip: {f:?}");
    }

    #[test]
    fn multiple_violations_share_bucket() {
        let f = detect_text_line_height(&snap(vec![
            entry("p#a", "p", 16.0, Some(18.0), 200),
            entry("p#b", "p", 16.0, Some(18.0), 200),
            entry("p#c", "p", 16.0, Some(18.0), 200),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.line-height-too-short")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
        assert!(hit.detail.contains("p#a"));
        assert!(hit.detail.contains("p#b"));
        assert!(hit.detail.contains("p#c"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("p#x{i}"), "p", 16.0, Some(18.0), 200))
            .collect();
        let f = detect_text_line_height(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.line-height-too-short")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_text_bearing_tags() {
        assert!(TEXT_LINE_HEIGHT_JS.starts_with("(() => {"));
        assert!(TEXT_LINE_HEIGHT_JS.ends_with(")()"));
        assert!(TEXT_LINE_HEIGHT_JS.contains("getComputedStyle"));
        assert!(TEXT_LINE_HEIGHT_JS.contains("lineHeight"));
        assert!(TEXT_LINE_HEIGHT_JS.contains("p, li, div"));
    }
}
