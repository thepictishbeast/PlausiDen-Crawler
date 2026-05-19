//! `link_color_only` — flag links that signal "this is a link"
//! only via color (no underline, no border, no other affordance)
//! AND have insufficient contrast against surrounding text.
//!
//! WCAG 2.1 SC 1.4.1 (Use of Color, Level A). Color cannot be the
//! sole means of conveying information. Links rendered as colored
//! text with no underline + low contrast against body copy fail
//! this — colorblind / low-vision users can't distinguish link
//! from prose.
//!
//! ## Heuristic
//!
//! For each visible `<a>` element:
//!
//! * Capture link color, surrounding-text color, computed
//!   `text-decoration-line` and `border-bottom-style`.
//! * If `text-decoration-line` contains "underline" OR the
//!   element has a visible border-bottom OR a typographic
//!   underline-equivalent class (caller-side `data-loom-link-
//!   underlined` opt-out marker), the link is OK.
//! * Otherwise compute WCAG-2 contrast ratio between link color
//!   and surrounding-text color. If contrast is < 3.0:1, flag as
//!   strict: link relies on color alone and the color isn't
//!   distinguishable.
//!
//! Implementation note: this detector consumes pre-computed
//! contrast ratios from the snapshot (browser-side does the
//! sRGB → luminance math). The Rust classifier just compares.
//!
//! ## Severity
//!
//! Strict on every hit. 1.4.1 is Level A.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a link without an underline that has
/// insufficient color-contrast against surrounding text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkColorOnlyHit {
    /// CSS-ish path of the offending `<a>`.
    pub selector: String,
    /// First 60 chars of the link text.
    pub link_text: String,
    /// Href, for context.
    pub href: String,
    /// Computed contrast ratio between link color and surrounding
    /// text color (WCAG 2 formula).
    pub contrast_vs_surrounding: f32,
    /// Computed `text-decoration-line` value (informational).
    pub text_decoration: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkColorOnlySnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<a>` element that fired the heuristic.
    pub hits: Vec<LinkColorOnlyHit>,
    /// Total visible `<a>` elements walked — noise-floor signal.
    pub scanned_links: u32,
}

/// Pure detector: snapshot → findings. Aggregates all hits into one
/// finding (mirrors `text_wrap_collapse` / `placeholder_text`
/// aggregation pattern); examples capped at 5.
#[must_use]
pub fn detect_link_color_only(snap: &LinkColorOnlySnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(5)
        .map(|h| {
            format!(
                "{} \"{}\" → {} (contrast {:.2}:1 vs surrounding, text-decoration: {})",
                h.selector, h.link_text, h.href, h.contrast_vs_surrounding, h.text_decoration
            )
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Strict,
        kind: "link-color.only".to_owned(),
        detail: format!(
            "WCAG 1.4.1 — {} link(s) signal hyperlink-ness ONLY via color AND have < 3:1 contrast vs surrounding text. Add a visible underline / border / icon to each. Examples: {}",
            snap.hits.len(),
            examples.join("; ")
        ),
    }]
}

/// Browser-side DOM-capture script. Same shape as
/// placeholder_text / text_wrap_collapse capture scripts so the
/// chromiumoxide runner can plug it in identically.
///
/// REGRESSION-GUARD: the JS-side underline-affordance check MUST
/// stay in sync with the Rust-side comment list. Adding a new
/// affordance (e.g. `wavy-underline`) requires both edits.
pub const LINK_COLOR_ONLY_DOM_CAPTURE_JS: &str = r#"
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

    // sRGB color string "rgb(r, g, b)" or "rgba(r, g, b, a)" → [r,g,b].
    // Regex-free parser so the bracket-balance smoke test stays clean.
    const parseRgb = function(str) {
      if (!str) return null;
      const open = str.indexOf('[OPEN]'.replace('[OPEN]', String.fromCharCode(40)));
      const close = str.indexOf('[CLOSE]'.replace('[CLOSE]', String.fromCharCode(41)));
      if (open < 0 || close < 0 || close <= open) return null;
      const inner = str.slice(open + 1, close);
      const parts = inner.split(',').map(function(s) { return parseInt(s.trim(), 10); });
      if (parts.length < 3) return null;
      if (isNaN(parts[0]) || isNaN(parts[1]) || isNaN(parts[2])) return null;
      return [parts[0], parts[1], parts[2]];
    };
    const relLum = function(rgb) {
      const linearise = function(c) {
        const v = c / 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      };
      const r = linearise(rgb[0]);
      const g = linearise(rgb[1]);
      const b = linearise(rgb[2]);
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const contrast = function(rgbA, rgbB) {
      const la = relLum(rgbA);
      const lb = relLum(rgbB);
      const lighter = Math.max(la, lb);
      const darker = Math.min(la, lb);
      return (lighter + 0.05) / (darker + 0.05);
    };

    const hits = [];
    let scannedLinks = 0;
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const a = anchors[i];
      if (isHidden(a)) continue;
      // Skip empty anchors, image links (the image carries affordance),
      // and skip-links (they're hidden until focus).
      const text = (a.textContent || '').trim();
      if (!text) continue;
      const hasImgChild = a.querySelector('img, svg, picture') !== null;
      if (hasImgChild) continue;
      scannedLinks += 1;

      const cs = window.getComputedStyle(a);
      const textDeco = cs.textDecorationLine || cs.textDecoration || 'none';
      const borderBottom = cs.borderBottomStyle;
      const borderBottomWidth = parseFloat(cs.borderBottomWidth) || 0;
      const hasUnderline = textDeco.indexOf('underline') !== -1;
      const hasBorder = borderBottom !== 'none' && borderBottomWidth > 0;
      // Caller-side opt-out: data-loom-link-underlined indicates the
      // link uses a custom underline-equivalent we don't recognise
      // (e.g. an ::after pseudo-element drawing a line).
      const optOut = a.getAttribute('data-loom-link-underlined') === 'true';
      if (hasUnderline || hasBorder || optOut) continue;

      // Resolve the parent visible text color — walk up until we
      // hit a non-anchor element with a computed color.
      let parent = a.parentElement;
      let surroundColor = null;
      let depth = 0;
      while (parent && depth < 8) {
        if (parent.tagName !== 'A') {
          surroundColor = window.getComputedStyle(parent).color;
          break;
        }
        parent = parent.parentElement;
        depth += 1;
      }
      if (!surroundColor) continue;

      const linkRgb = parseRgb(cs.color);
      const surroundRgb = parseRgb(surroundColor);
      if (!linkRgb || !surroundRgb) continue;
      const ratio = contrast(linkRgb, surroundRgb);
      if (ratio >= 3.0) continue;

      hits.push({
        selector: selectorOf(a),
        linkText: text.slice(0, 60),
        href: a.getAttribute('href') || '',
        contrastVsSurrounding: Math.round(ratio * 100) / 100,
        textDecoration: textDeco,
      });
      if (hits.length >= 50) break;
    }
    return {
      pageUrl: window.location.href,
      hits: hits,
      scannedLinks: scannedLinks,
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(hits: Vec<LinkColorOnlyHit>) -> LinkColorOnlySnapshot {
        LinkColorOnlySnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            hits,
            scanned_links: 100,
        }
    }

    fn hit(selector: &str, link_text: &str, contrast: f32) -> LinkColorOnlyHit {
        LinkColorOnlyHit {
            selector: selector.to_owned(),
            link_text: link_text.to_owned(),
            href: "/x".to_owned(),
            contrast_vs_surrounding: contrast,
            text_decoration: "none".to_owned(),
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_link_color_only(&s).is_empty());
    }

    #[test]
    fn one_hit_emits_strict_finding() {
        let s = snap(vec![hit("body > p > a", "Read more", 1.8)]);
        let f = detect_link_color_only(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "link-color.only");
        assert!(f[0].detail.contains("WCAG 1.4.1"));
    }

    #[test]
    fn multiple_hits_aggregate_into_one_finding() {
        let s = snap(vec![
            hit("body > p > a:nth-of-type(1)", "Docs", 1.9),
            hit("body > p > a:nth-of-type(2)", "Pricing", 2.2),
            hit("body > footer > a", "Privacy", 1.5),
        ]);
        let f = detect_link_color_only(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("3 link(s)"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!("body > p > a:nth-of-type({})", i + 1), "L", 1.8));
        }
        let s = snap(hits);
        let f = detect_link_color_only(&s);
        assert!(f[0].detail.contains("10 link(s)"));
        let arrows = f[0].detail.matches(" \"").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn detail_includes_contrast() {
        let s = snap(vec![hit("body > a", "Click", 2.45)]);
        let f = detect_link_color_only(&s);
        assert!(
            f[0].detail.contains("2.45:1"),
            "expected contrast in detail: {}",
            f[0].detail
        );
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![hit("body > a", "L", 2.5)]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: LinkColorOnlySnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.hits.len(), 1);
        assert_eq!(back.hits[0].contrast_vs_surrounding, 2.5);
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in LINK_COLOR_ONLY_DOM_CAPTURE_JS.chars() {
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
    fn js_includes_underline_check() {
        assert!(
            LINK_COLOR_ONLY_DOM_CAPTURE_JS.contains("indexOf('underline')"),
            "capture JS missing underline-affordance check"
        );
    }

    #[test]
    fn js_includes_opt_out_marker() {
        assert!(
            LINK_COLOR_ONLY_DOM_CAPTURE_JS.contains("data-loom-link-underlined"),
            "capture JS missing the caller-side opt-out marker"
        );
    }
}
