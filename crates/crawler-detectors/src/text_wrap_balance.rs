//! `text_wrap_balance` — `text-wrap: balance / pretty` usage
//! audit on headings + key text containers.
//!
//! Sibling to `text_line_height`, `text_letter_spacing`,
//! `text_paragraph_spacing`, `text_word_spacing` (all WCAG
//! 1.4.12), and `text_wrap_collapse` (which audits actual
//! collapsed-text bugs in the wild). This detector covers a
//! quality-of-text axis: whether HEADINGS use modern
//! `text-wrap` CSS for visually balanced line breaks.
//!
//! ## The contract
//!
//! Per CSS Text Module Level 4 / CSS Working Group consensus:
//!
//! * `text-wrap: wrap` (default) — greedy line-break; the last
//!   line of a wrap is whatever's left over.
//! * `text-wrap: balance` — distributes characters across lines
//!   so the rag is even (last line not orphaned with one word).
//!   Browser support: Chromium 114+, Safari 17.4+, Firefox 121+
//!   (now baseline-ish).
//! * `text-wrap: pretty` — minimizes orphans + improves
//!   hyphenation. Chromium 117+, Safari 17.4+, Firefox 124+.
//!
//! For HEADINGS specifically, `balance` is a near-universal
//! UX win — the orphan-word problem (single word on the last
//! line) is endemic to greedy wrap and the only practical
//! mitigation pre-`text-wrap` was forced `<br>` tags, which
//! break responsive layouts.
//!
//! ## Findings
//!
//! * `text-wrap.heading-default-wrap` warn — `<h1>`-`<h6>`
//!   computed `text-wrap` is `wrap` (default) AND the heading
//!   text wraps to 2+ lines at the snapshot viewport. Best
//!   practice is `balance` for headings.
//! * `text-wrap.invalid-value` strict — `text-wrap` value not
//!   in `{wrap, nowrap, balance, pretty, stable}`.
//!
//! Out of scope:
//!
//! * Body-text `text-wrap: pretty` recommendation — separate
//!   axis (balance vs pretty per-element trade-offs).
//! * Browser-compatibility checks — assumed up-to-date by the
//!   runner; old browsers fall through to default `wrap`.
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

/// Valid `text-wrap` values per CSS Text Level 4.
const VALID_VALUES: &[&str] = &["wrap", "nowrap", "balance", "pretty", "stable"];

/// One captured heading element with text-wrap signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HeadingEntry {
    /// CSS-ish selector pointing at the heading.
    pub selector: String,
    /// Tag name (h1..h6, lowercased).
    pub tag: String,
    /// Computed `text-wrap` value.
    pub text_wrap_value: String,
    /// Whether the heading text wraps to 2+ lines at the
    /// snapshot viewport.
    pub wraps_multiline: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextWrapBalanceSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every heading element on the page.
    pub headings: Vec<HeadingEntry>,
}

/// Detector.
#[must_use]
pub fn detect_text_wrap_balance(snap: &TextWrapBalanceSnapshot) -> Vec<AxisFinding> {
    let mut invalid: Vec<String> = Vec::new();
    let mut default_wrap_multiline: Vec<String> = Vec::new();

    for entry in &snap.headings {
        let raw = entry.text_wrap_value.trim();
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        let is_valid = VALID_VALUES
            .iter()
            .any(|v| v.eq_ignore_ascii_case(&lower));
        if !is_valid {
            invalid.push(format!("{} (value=\"{}\")", entry.selector, raw));
            continue;
        }
        // Default `wrap` on a multi-line heading is the
        // common warn case.
        if lower == "wrap" && entry.wraps_multiline {
            default_wrap_multiline.push(format!(
                "{} (tag={})",
                entry.selector, entry.tag
            ));
        }
    }

    let mut findings = Vec::new();
    let total = snap.headings.len();

    if !invalid.is_empty() {
        let preview =
            preview_examples(&invalid.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "text-wrap.invalid-value".to_owned(),
            detail: format!(
                "{} of {} heading text-wrap value(s) are not in {{wrap, nowrap, balance, pretty, stable}}. Examples: {}",
                invalid.len(),
                total,
                preview
            ),
        });
    }

    if !default_wrap_multiline.is_empty() {
        let preview = preview_examples(
            &default_wrap_multiline
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "text-wrap.heading-default-wrap".to_owned(),
            detail: format!(
                "{} of {} multi-line heading(s) use default `text-wrap: wrap`; consider `balance` to avoid orphan words. Examples: {}",
                default_wrap_multiline.len(),
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

/// Page-side eval. Walks all heading elements, captures
/// computed text-wrap + whether the heading wraps to 2+ lines
/// (via getClientRects().length).
pub const TEXT_WRAP_BALANCE_JS: &str = r##"(() => {
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

    const headings = Array.from(document.querySelectorAll('h1, h2, h3, h4, h5, h6'));
    const entries = headings.map(function(h) {
      const cs = getComputedStyle(h);
      // getClientRects() returns one rect per line box; >= 2
      // means the heading wraps.
      const rects = h.getClientRects();
      return {
        selector: selectorOf(h),
        tag: h.tagName.toLowerCase(),
        textWrapValue: cs.textWrap || '',
        wrapsMultiline: rects.length >= 2
      };
    });

    return {
      pageUrl: location.href,
      headings: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        sel: &str,
        tag: &str,
        wrap: &str,
        multiline: bool,
    ) -> HeadingEntry {
        HeadingEntry {
            selector: sel.to_owned(),
            tag: tag.to_owned(),
            text_wrap_value: wrap.to_owned(),
            wraps_multiline: multiline,
        }
    }

    fn snap(headings: Vec<HeadingEntry>) -> TextWrapBalanceSnapshot {
        TextWrapBalanceSnapshot {
            page_url: "https://example.test/".to_owned(),
            headings,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_text_wrap_balance(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn balance_on_multiline_heading_is_clean() {
        let f = detect_text_wrap_balance(&snap(vec![entry(
            "h1", "h1", "balance", true,
        )]));
        assert!(f.is_empty(), "balance should pass: {f:?}");
    }

    #[test]
    fn pretty_on_multiline_heading_is_clean() {
        let f = detect_text_wrap_balance(&snap(vec![entry(
            "h1", "h1", "pretty", true,
        )]));
        assert!(f.is_empty(), "pretty should pass: {f:?}");
    }

    #[test]
    fn single_line_heading_with_default_wrap_is_clean() {
        let f = detect_text_wrap_balance(&snap(vec![entry(
            "h1", "h1", "wrap", false,
        )]));
        assert!(
            f.is_empty(),
            "single-line heading with default wrap should pass: {f:?}"
        );
    }

    #[test]
    fn multiline_heading_with_default_wrap_is_warn() {
        let f = detect_text_wrap_balance(&snap(vec![entry(
            "h1#hero", "h1", "wrap", true,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-wrap.heading-default-wrap")
            .expect("default-wrap expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("h1#hero"));
    }

    #[test]
    fn invalid_value_is_strict() {
        for v in ["auto", "smart", "compact", "yes"] {
            let f = detect_text_wrap_balance(&snap(vec![entry(
                "h1", "h1", v, true,
            )]));
            assert!(
                f.iter().any(|x| x.kind == "text-wrap.invalid-value"),
                "value {v} should flag invalid"
            );
        }
    }

    #[test]
    fn case_insensitive_value_match() {
        let f = detect_text_wrap_balance(&snap(vec![entry(
            "h1", "h1", "BALANCE", true,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "text-wrap.invalid-value"),
            "BALANCE should be valid: {f:?}"
        );
    }

    #[test]
    fn invalid_value_suppresses_default_wrap_finding() {
        let f = detect_text_wrap_balance(&snap(vec![entry(
            "h1", "h1", "smart", true,
        )]));
        assert!(f.iter().any(|x| x.kind == "text-wrap.invalid-value"));
        assert!(!f
            .iter()
            .any(|x| x.kind == "text-wrap.heading-default-wrap"));
    }

    #[test]
    fn each_heading_level_treated_equivalently() {
        for h in ["h1", "h2", "h3", "h4", "h5", "h6"] {
            let f = detect_text_wrap_balance(&snap(vec![entry(
                h, h, "wrap", true,
            )]));
            assert!(
                f.iter().any(|x| x.kind == "text-wrap.heading-default-wrap"),
                "{h} should flag default-wrap on multi-line"
            );
        }
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("h1#x{i}"), "h1", "wrap", true))
            .collect();
        let f = detect_text_wrap_balance(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "text-wrap.heading-default-wrap")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_headings() {
        assert!(TEXT_WRAP_BALANCE_JS.starts_with("(() => {"));
        assert!(TEXT_WRAP_BALANCE_JS.ends_with(")()"));
        assert!(TEXT_WRAP_BALANCE_JS.contains("h1, h2, h3, h4, h5, h6"));
        assert!(TEXT_WRAP_BALANCE_JS.contains("getClientRects"));
        assert!(TEXT_WRAP_BALANCE_JS.contains("textWrap"));
    }
}
