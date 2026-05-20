//! `text_wrap_collapse` — flags text elements wrapping at one or two
//! characters per visual line.
//!
//! Detects the rendering pathology where a column is too narrow for
//! its content, so every character (or short pair) breaks to its
//! own line. Looks like:
//!
//! ```text
//! S
//! o
//! f
//! t
//! w
//! a
//! r
//! e
//! ```
//!
//! Causes are usually CSS layout failures: a grid that didn't
//! collapse columns at a mobile breakpoint, `word-break: break-all`
//! applied too aggressively, a font-size shrinking column that
//! never reached its `min-content` width, or content being placed
//! into a flex item whose `min-width` defaulted to `auto` and
//! grew narrower than a single grapheme. The viewer can technically
//! still read the text — but the rendering is broken in a way the
//! existing `ui_overflow` detector doesn't catch (the text isn't
//! overflowing its parent; the parent is just way too narrow).
//!
//! ## Heuristic
//!
//! For each visible block-like element carrying text:
//!
//! * `chars_per_line = char_count / approx_lines` where
//!   `approx_lines = round(bounding_rect.height / line_height_px)`.
//! * Flag when `chars_per_line < 3.0` AND `char_count > 8` AND
//!   `char_count / approx_lines < 0.5` (i.e. way more lines than
//!   characters-per-line would naturally produce).
//!
//! Excludes:
//!
//! * `<pre>` / `<code>` (intentional vertical formatting).
//! * Elements with explicit `white-space: pre-wrap` / `pre`.
//! * Marquees / animations / scroll-snap rails.
//! * Already-hidden ancestors.
//!
//! ## Severity
//!
//! All hits are `strict` — character-per-line wrap is never the
//! intended rendering. If it happens at a specific viewport, the
//! layout there is broken.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a text element that collapsed to ~one
/// character per line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextWrapCollapseHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped at 80 chars) for context.
    pub text: String,
    /// Total character count of the visible text.
    pub char_count: u32,
    /// Approximate visual line count (rect.height / line-height).
    pub approx_lines: u32,
    /// `char_count / approx_lines`. Lower = more pathological.
    pub chars_per_line: f32,
    /// Element bounding-box width in CSS px.
    pub rect_width: u32,
    /// Element bounding-box height in CSS px.
    pub rect_height: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TextWrapCollapseSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every text element that fired the heuristic.
    pub hits: Vec<TextWrapCollapseHit>,
    /// Total visible text elements walked — noise-floor signal.
    pub scanned_elements: u32,
}

/// Pure detector: snapshot → findings. Aggregates all hits into one
/// finding (matching the placeholder_text pattern); examples are
/// capped at 5.
#[must_use]
pub fn detect_text_wrap_collapse(snap: &TextWrapCollapseSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(5)
        .map(|h| {
            format!(
                "{} (\"{}\", {}c × {}L = {:.1}c/L, {}×{}px) ",
                h.selector,
                h.text,
                h.char_count,
                h.approx_lines,
                h.chars_per_line,
                h.rect_width,
                h.rect_height
            )
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Strict,
        kind: "text-wrap.collapse".to_owned(),
        detail: format!(
            "{} element(s) wrapping at <3 chars per line at viewport width {}px — column too narrow for content; check responsive grid / flex / column breakpoints. Examples: {}",
            snap.hits.len(),
            snap.viewport_width,
            examples.join("; ")
        ),
    }]
}

/// Browser-side DOM-capture script. Pinned for the future
/// chromiumoxide path; mirror any change in this file's hits +
/// snapshot fields.
///
/// REGRESSION-GUARD: the JS-side `excludeTagSet` MUST stay in sync
/// with the Rust-side comment list. Adding a new excluded tag
/// requires a docs update in this file's module comment.
pub const TEXT_WRAP_COLLAPSE_DOM_CAPTURE_JS: &str = r#"
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

    const isHidden = function(el) {
      if (!el || el.nodeType !== 1) return false;
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return true;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return true;
      return false;
    };

    const excludeTagSet = new Set([
      'SCRIPT','STYLE','NOSCRIPT','TEMPLATE','PRE','CODE','SVG','PATH',
      'IMG','VIDEO','AUDIO','SOURCE','TRACK','OPTION','META','LINK','HEAD','HTML','BODY'
    ]);

    const hits = [];
    let scannedElements = 0;

    const candidates = document.querySelectorAll('body h1, body h2, body h3, body h4, body h5, body h6, body p, body span, body li, body dt, body dd, body button, body a, body div');
    for (let i = 0; i < candidates.length; i++) {
      const el = candidates[i];
      if (excludeTagSet.has(el.tagName)) continue;
      if (isHidden(el)) continue;
      const cs = window.getComputedStyle(el);
      const ws = cs.whiteSpace;
      if (ws === 'pre' || ws === 'pre-wrap' || ws === 'pre-line') continue;
      const text = (el.textContent || '').trim();
      if (text.length <= 8) continue;
      const charCount = text.length;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      const fontSize = parseFloat(cs.fontSize) || 16;
      let lineHeightRaw = cs.lineHeight;
      let lineHeightPx = parseFloat(lineHeightRaw);
      if (!isFinite(lineHeightPx) || lineHeightPx <= 0) {
        lineHeightPx = fontSize * 1.2;
      }
      const approxLines = Math.max(1, Math.round(rect.height / lineHeightPx));
      if (approxLines < 4) continue;
      const charsPerLine = charCount / approxLines;
      if (charsPerLine >= 3.0) continue;
      // Final check: very wide elements with short text shouldn't fire.
      if (rect.width > fontSize * 4) continue;
      scannedElements += 1;
      hits.push({
        selector: selectorOf(el),
        text: text.slice(0, 80),
        charCount: charCount,
        approxLines: approxLines,
        charsPerLine: Math.round(charsPerLine * 10) / 10,
        rectWidth: Math.round(rect.width),
        rectHeight: Math.round(rect.height),
      });
      if (hits.length >= 50) break;
    }
    return {
      pageUrl: window.location.href,
      viewportWidth: Math.round(window.innerWidth),
      hits: hits,
      scannedElements: scannedElements,
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(hits: Vec<TextWrapCollapseHit>) -> TextWrapCollapseSnapshot {
        TextWrapCollapseSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            viewport_width: 390,
            hits,
            scanned_elements: 100,
        }
    }

    fn hit(selector: &str, text: &str, char_count: u32, approx_lines: u32) -> TextWrapCollapseHit {
        TextWrapCollapseHit {
            selector: selector.to_owned(),
            text: text.to_owned(),
            char_count,
            approx_lines,
            chars_per_line: char_count as f32 / approx_lines as f32,
            rect_width: 40,
            rect_height: approx_lines * 20,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_text_wrap_collapse(&s).is_empty());
    }

    #[test]
    fn one_hit_emits_strict_finding() {
        let s = snap(vec![hit(
            "body > section > h2",
            "A build platform that outlives its dependencies.",
            48,
            48,
        )]);
        let f = detect_text_wrap_collapse(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "text-wrap.collapse");
        assert!(f[0].detail.contains("1 element(s)"));
        assert!(f[0].detail.contains("390px"));
    }

    #[test]
    fn multiple_hits_aggregate_into_one_finding() {
        let s = snap(vec![
            hit("body > h2:nth-of-type(1)", "First broken title", 18, 18),
            hit("body > h2:nth-of-type(2)", "Second broken title", 19, 19),
            hit("body > span", "Eyebrow chip text", 17, 17),
        ]);
        let f = detect_text_wrap_collapse(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("3 element(s)"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!("body > p:nth-of-type({})", i + 1),
                "Pathological text wrap",
                22,
                22,
            ));
        }
        let s = snap(hits);
        let f = detect_text_wrap_collapse(&s);
        assert!(f[0].detail.contains("10 element(s)"));
        let arrows = f[0].detail.matches(" (\"").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![hit("body > h1", "Broken hero", 11, 11)]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: TextWrapCollapseSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.hits.len(), 1);
        assert_eq!(back.viewport_width, 390);
        assert_eq!(back.hits[0].char_count, 11);
    }

    #[test]
    fn detail_includes_chars_per_line() {
        let s = snap(vec![hit("body > h2", "Hello world this is text", 24, 24)]);
        let f = detect_text_wrap_collapse(&s);
        assert!(
            f[0].detail.contains("1.0c/L"),
            "expected chars-per-line in detail: {}",
            f[0].detail
        );
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in TEXT_WRAP_COLLAPSE_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }

    #[test]
    fn js_includes_pre_whitespace_exclusion() {
        // Catches accidental removal of the white-space: pre / pre-wrap exclusion.
        assert!(
            TEXT_WRAP_COLLAPSE_DOM_CAPTURE_JS.contains("'pre'")
                && TEXT_WRAP_COLLAPSE_DOM_CAPTURE_JS.contains("'pre-wrap'"),
            "capture JS missing pre/pre-wrap exclusion"
        );
    }
}
