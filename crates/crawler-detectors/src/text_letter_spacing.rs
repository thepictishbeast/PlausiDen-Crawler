//! `text_letter_spacing` — WCAG 1.4.12 letter-spacing audit.
//!
//! Sibling axis to `text_line_height` (line-height dimension of
//! WCAG 1.4.12) and to `runtime_contrast`. WCAG 2.1 SC 1.4.12
//! Text Spacing (Level AA) requires that the page support user-
//! agent / extension overrides setting:
//!
//! * Line height ≥ 1.5× font-size (covered by `text_line_height`)
//! * Paragraph spacing ≥ 2× font-size (future axis)
//! * **Letter spacing ≥ 0.12× font-size**
//! * Word spacing ≥ 0.16× font-size (future axis)
//!
//! This detector audits the LETTER-SPACING dimension. The SC is
//! about "supporting" the override, not about whether the
//! authored letter-spacing is high enough — but operators who
//! ship NEGATIVE letter-spacing (`letter-spacing: -0.05em`) on
//! body text break the override path: the user's accessibility
//! preference can't push the spacing below where the author put
//! it without content loss.
//!
//! Negative letter-spacing on body text also damages
//! readability for low-vision users, dyslexic readers, and
//! anyone reading at low pixel density. The detector flags it
//! aggressively.
//!
//! ## Findings
//!
//! * `text-spacing.negative-letter-spacing` strict — computed
//!   letter-spacing < 0 on a body-text element (>= 80 chars).
//! * `text-spacing.tight-letter-spacing` warn — computed
//!   letter-spacing < 0 on a short text node (>= 20 and < 80
//!   chars). Some short headlines or display text intentionally
//!   tighten; surface but don't gate.
//!
//! Out of scope:
//!
//! * Word spacing, paragraph spacing — separate future axes.
//! * Heading line-height tightening (text_line_height covers
//!   line-height; this axis covers a different SC 1.4.12
//!   dimension).
//! * Computing the threshold when font-size is not pixels — the
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

/// Short-text threshold in characters. Lines below this are
/// downgraded to warn.
const SHORT_TEXT_THRESHOLD: usize = 80;

/// Floor character count below which the detector ignores the
/// element entirely.
const MIN_TEXT_LENGTH: usize = 20;

/// One captured text-bearing element with computed-style
/// letter-spacing + text-content length.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LetterSpacingEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Tag name (lowercased).
    pub tag: String,
    /// Computed `letter-spacing` in CSS pixels (can be negative).
    /// `None` when the runner could not compute (e.g. `normal`
    /// keyword without numeric resolution; bail rather than
    /// guess).
    pub letter_spacing_px: Option<f64>,
    /// Computed `font-size` in CSS pixels.
    pub font_size_px: f64,
    /// Length of trimmed text content in characters.
    pub text_length: usize,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextLetterSpacingSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every text-bearing element observed on the page.
    pub entries: Vec<LetterSpacingEntry>,
}

/// Detector.
#[must_use]
pub fn detect_text_letter_spacing(snap: &TextLetterSpacingSnapshot) -> Vec<AxisFinding> {
    let mut negative_body: Vec<String> = Vec::new();
    let mut tight_short: Vec<String> = Vec::new();

    for entry in &snap.entries {
        // Skip heading tags — tight display letter-spacing on
        // headings is common design intent (titles, marketing
        // headlines). SC 1.4.12 targets body copy.
        if is_heading_tag(&entry.tag) {
            continue;
        }
        if entry.text_length < MIN_TEXT_LENGTH {
            continue;
        }
        let Some(ls) = entry.letter_spacing_px else {
            continue;
        };
        if ls >= 0.0 {
            continue;
        }
        let row = format!(
            "{} (tag={}, letter_spacing={:.2}px, font_size={:.1}px, text_len={})",
            entry.selector, entry.tag, ls, entry.font_size_px, entry.text_length
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
            kind: "text-spacing.negative-letter-spacing".to_owned(),
            detail: format!(
                "{} of {} body-text element(s) carry negative letter-spacing (breaks WCAG 1.4.12 override path + harms low-vision / dyslexic readers). Examples: {}",
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
            kind: "text-spacing.tight-letter-spacing".to_owned(),
            detail: format!(
                "{} of {} short text node(s) carry negative letter-spacing; surface but don't gate (some display text intentionally tightens). Examples: {}",
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
/// computed letter-spacing + font-size + text length.
pub const TEXT_LETTER_SPACING_JS: &str = r##"(() => {
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
      // `letter-spacing: normal` returns the keyword as `normal`.
      // We bail (null) rather than guess.
      const lsRaw = cs.letterSpacing;
      const ls = lsRaw === 'normal' ? null : parsePx(lsRaw);
      const text = (el.textContent || '').trim();
      entries.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        letterSpacingPx: ls,
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
        ls: Option<f64>,
        fs: f64,
        text_len: usize,
    ) -> LetterSpacingEntry {
        LetterSpacingEntry {
            selector: sel.to_owned(),
            tag: tag.to_owned(),
            letter_spacing_px: ls,
            font_size_px: fs,
            text_length: text_len,
        }
    }

    fn snap(entries: Vec<LetterSpacingEntry>) -> TextLetterSpacingSnapshot {
        TextLetterSpacingSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_text_letter_spacing(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn zero_letter_spacing_is_clean() {
        let f = detect_text_letter_spacing(&snap(vec![entry(
            "p", "p", Some(0.0), 16.0, 200,
        )]));
        assert!(f.is_empty(), "zero letter-spacing should pass: {f:?}");
    }

    #[test]
    fn positive_letter_spacing_is_clean() {
        let f = detect_text_letter_spacing(&snap(vec![entry(
            "p", "p", Some(0.5), 16.0, 200,
        )]));
        assert!(f.is_empty(), "positive letter-spacing should pass: {f:?}");
    }

    #[test]
    fn negative_body_letter_spacing_is_strict() {
        let f = detect_text_letter_spacing(&snap(vec![entry(
            "article > p:nth-of-type(1)",
            "p",
            Some(-0.5),
            16.0,
            250,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.negative-letter-spacing")
            .expect("negative-body expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("letter_spacing=-0.50px"));
    }

    #[test]
    fn negative_short_text_is_warn() {
        let f = detect_text_letter_spacing(&snap(vec![entry(
            "li", "li", Some(-0.3), 16.0, 30,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.tight-letter-spacing")
            .expect("tight-letter-spacing expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn headings_exempt_from_audit() {
        for h in ["h1", "h2", "h3", "h4", "h5", "h6"] {
            let f = detect_text_letter_spacing(&snap(vec![entry(
                h, h, Some(-0.5), 32.0, 200,
            )]));
            assert!(
                f.is_empty(),
                "{h} should be exempt; got {f:?}"
            );
        }
    }

    #[test]
    fn below_min_text_length_is_ignored() {
        let f = detect_text_letter_spacing(&snap(vec![entry(
            "span", "span", Some(-0.5), 16.0, 10,
        )]));
        assert!(f.is_empty(), "below MIN_TEXT_LENGTH should ignore: {f:?}");
    }

    #[test]
    fn missing_letter_spacing_px_skips_entry() {
        let f = detect_text_letter_spacing(&snap(vec![entry(
            "p", "p", None, 16.0, 200,
        )]));
        assert!(f.is_empty(), "None letter_spacing should skip: {f:?}");
    }

    #[test]
    fn multiple_violations_share_bucket() {
        let f = detect_text_letter_spacing(&snap(vec![
            entry("p#a", "p", Some(-0.5), 16.0, 200),
            entry("p#b", "p", Some(-0.5), 16.0, 200),
            entry("p#c", "p", Some(-0.5), 16.0, 200),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.negative-letter-spacing")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("p#x{i}"), "p", Some(-0.5), 16.0, 200))
            .collect();
        let f = detect_text_letter_spacing(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-spacing.negative-letter-spacing")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_text_bearing_tags() {
        assert!(TEXT_LETTER_SPACING_JS.starts_with("(() => {"));
        assert!(TEXT_LETTER_SPACING_JS.ends_with(")()"));
        assert!(TEXT_LETTER_SPACING_JS.contains("getComputedStyle"));
        assert!(TEXT_LETTER_SPACING_JS.contains("letterSpacing"));
        assert!(TEXT_LETTER_SPACING_JS.contains("p, li, div"));
    }
}
