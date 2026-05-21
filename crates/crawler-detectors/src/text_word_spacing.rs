//! `text_word_spacing` — WCAG 1.4.12 word-spacing audit.
//!
//! Fourth and final axis in the WCAG 1.4.12 Text Spacing
//! quartet:
//!
//! 1. `text_line_height` — line height ≥ 1.5× font-size
//! 2. `text_letter_spacing` — letter spacing ≥ 0.12× font-size
//! 3. `text_paragraph_spacing` — paragraph spacing ≥ 2×
//! 4. `text_word_spacing` — word spacing ≥ 0.16× font-size  ← THIS
//!
//! Per WCAG 2.1 SC 1.4.12 (Level AA), the page MUST support a
//! user-agent / extension override that sets `word-spacing` to
//! at least 0.16× the font-size, without content loss.
//!
//! Operators ship NEGATIVE `word-spacing` on body text to
//! tighten justified columns — fine for short marketing
//! headlines, hostile for paragraphs. The user's WCAG override
//! can only ADD to word-spacing; if the authored value is
//! negative, the override starts from a deficit and the body
//! text remains compressed.
//!
//! Negative word-spacing also harms readability for dyslexic
//! readers (research literature recommends slightly INCREASED
//! word-spacing as a dyslexia-friendly default).
//!
//! ## Findings
//!
//! * `text-spacing.negative-word-spacing` strict — computed
//!   word-spacing < 0 on a body-text element (>= 80 chars).
//! * `text-spacing.tight-word-spacing` warn — computed
//!   word-spacing < 0 on a short text node (>= 20 and < 80
//!   chars). Some short headlines / display text intentionally
//!   tighten word-spacing; surface but don't gate.
//!
//! Out of scope:
//!
//! * Positive word-spacing values that exceed reasonable
//!   bounds — separate axis.
//! * Line-height, letter-spacing, paragraph-spacing — sibling
//!   axes.
//! * Heading word-spacing tightening (titles often use design-
//!   intent tight word-spacing; SC 1.4.12 targets body copy).
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

/// Short-text threshold in characters. Lines below this are
/// downgraded to warn.
const SHORT_TEXT_THRESHOLD: usize = 80;

/// Floor character count below which the detector ignores the
/// element entirely.
const MIN_TEXT_LENGTH: usize = 20;

/// One captured text-bearing element with computed-style
/// word-spacing + text-content length.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct WordSpacingEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Tag name (lowercased).
    pub tag: String,
    /// Computed `word-spacing` in CSS pixels (can be negative).
    /// `None` when the runner could not compute (`normal`
    /// keyword without numeric resolution; bail rather than
    /// guess).
    pub word_spacing_px: Option<f64>,
    /// Computed `font-size` in CSS pixels.
    pub font_size_px: f64,
    /// Length of trimmed text content in characters.
    pub text_length: usize,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextWordSpacingSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every text-bearing element observed on the page.
    pub entries: Vec<WordSpacingEntry>,
}

/// Detector.
#[must_use]
pub fn detect_text_word_spacing(snap: &TextWordSpacingSnapshot) -> Vec<AxisFinding> {
    let mut negative_body: Vec<String> = Vec::new();
    let mut tight_short: Vec<String> = Vec::new();

    for entry in &snap.entries {
        if is_heading_tag(&entry.tag) {
            continue;
        }
        if entry.text_length < MIN_TEXT_LENGTH {
            continue;
        }
        let Some(ws) = entry.word_spacing_px else {
            continue;
        };
        if ws >= 0.0 {
            continue;
        }
        let row = format!(
            "{} (tag={}, word_spacing={:.2}px, font_size={:.1}px, text_len={})",
            entry.selector, entry.tag, ws, entry.font_size_px, entry.text_length
        );
        if entry.text_length >= SHORT_TEXT_THRESHOLD {
            negative_body.push(row);
        } else {
            tight_short.push(row);
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !negative_body.is_empty() {
        let preview = preview_examples(
            &negative_body
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "text-spacing.negative-word-spacing".to_owned(),
            detail: format!(
                "{} of {} body-text element(s) carry negative word-spacing (breaks WCAG 1.4.12 override path + harms dyslexic readers). Examples: {}",
                negative_body.len(),
                total,
                preview
            ),
        });
    }

    if !tight_short.is_empty() {
        let preview = preview_examples(
            &tight_short.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "text-spacing.tight-word-spacing".to_owned(),
            detail: format!(
                "{} of {} short text node(s) carry negative word-spacing; surface but don't gate (some display text intentionally tightens). Examples: {}",
                tight_short.len(),
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

/// Page-side eval. Walks text-bearing elements, captures
/// computed word-spacing + font-size + text length.
pub const TEXT_WORD_SPACING_JS: &str = r##"(() => {
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
      const wsRaw = cs.wordSpacing;
      const ws = wsRaw === 'normal' ? null : parsePx(wsRaw);
      const text = (el.textContent || '').trim();
      entries.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        wordSpacingPx: ws,
        fontSizePx: fontSize,
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
        ws: Option<f64>,
        fs: f64,
        text_len: usize,
    ) -> WordSpacingEntry {
        WordSpacingEntry {
            selector: sel.to_owned(),
            tag: tag.to_owned(),
            word_spacing_px: ws,
            font_size_px: fs,
            text_length: text_len,
        }
    }

    fn snap(entries: Vec<WordSpacingEntry>) -> TextWordSpacingSnapshot {
        TextWordSpacingSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_text_word_spacing(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn zero_word_spacing_is_clean() {
        let f = detect_text_word_spacing(&snap(vec![entry(
            "p", "p", Some(0.0), 16.0, 200,
        )]));
        assert!(f.is_empty(), "zero word-spacing should pass: {f:?}");
    }

    #[test]
    fn positive_word_spacing_is_clean() {
        let f = detect_text_word_spacing(&snap(vec![entry(
            "p", "p", Some(1.5), 16.0, 200,
        )]));
        assert!(f.is_empty(), "positive word-spacing should pass: {f:?}");
    }

    #[test]
    fn negative_body_word_spacing_is_strict() {
        let f = detect_text_word_spacing(&snap(vec![entry(
            "article > p:nth-of-type(1)",
            "p",
            Some(-1.0),
            16.0,
            250,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.negative-word-spacing")
            .expect("negative-body expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("word_spacing=-1.00px"));
    }

    #[test]
    fn negative_short_text_is_warn() {
        let f = detect_text_word_spacing(&snap(vec![entry(
            "li", "li", Some(-0.5), 16.0, 30,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.tight-word-spacing")
            .expect("tight-word-spacing expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn headings_exempt_from_audit() {
        for h in ["h1", "h2", "h3", "h4", "h5", "h6"] {
            let f = detect_text_word_spacing(&snap(vec![entry(
                h, h, Some(-1.0), 32.0, 200,
            )]));
            assert!(
                f.is_empty(),
                "{h} should be exempt; got {f:?}"
            );
        }
    }

    #[test]
    fn below_min_text_length_is_ignored() {
        let f = detect_text_word_spacing(&snap(vec![entry(
            "span", "span", Some(-1.0), 16.0, 10,
        )]));
        assert!(f.is_empty(), "below MIN_TEXT_LENGTH should ignore: {f:?}");
    }

    #[test]
    fn missing_word_spacing_px_skips_entry() {
        // `word-spacing: normal` cannot be resolved as px;
        // detector bails rather than guess.
        let f = detect_text_word_spacing(&snap(vec![entry(
            "p", "p", None, 16.0, 200,
        )]));
        assert!(f.is_empty(), "None word_spacing should skip: {f:?}");
    }

    #[test]
    fn multiple_violations_share_bucket() {
        let f = detect_text_word_spacing(&snap(vec![
            entry("p#a", "p", Some(-1.0), 16.0, 200),
            entry("p#b", "p", Some(-1.0), 16.0, 200),
            entry("p#c", "p", Some(-1.0), 16.0, 200),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.negative-word-spacing")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("p#x{i}"), "p", Some(-1.0), 16.0, 200))
            .collect();
        let f = detect_text_word_spacing(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.negative-word-spacing")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_text_bearing_tags() {
        assert!(TEXT_WORD_SPACING_JS.starts_with("(() => {"));
        assert!(TEXT_WORD_SPACING_JS.ends_with(")()"));
        assert!(TEXT_WORD_SPACING_JS.contains("getComputedStyle"));
        assert!(TEXT_WORD_SPACING_JS.contains("wordSpacing"));
        assert!(TEXT_WORD_SPACING_JS.contains("p, li, div"));
    }
}
