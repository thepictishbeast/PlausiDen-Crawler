//! `modern_image_formats` — flag `<img>` references to legacy
//! formats (JPEG / PNG / GIF) that don't have an AVIF / WebP
//! `<source>` sibling.
//!
//! Lighthouse "modern-image-formats" + "uses-optimized-images"
//! audits. Marketing pages routinely ship large jpg/png hero
//! images without offering AVIF / WebP alternatives — Lighthouse
//! estimates this typically costs 30-70% of the asset weight.
//!
//! ## Heuristic
//!
//! For each visible `<img>` whose rendered display width is
//! ≥ 200 px AND whose `src` ends in `.jpg`, `.jpeg`, `.png`, or
//! `.gif`:
//!
//! * If the `<img>` is wrapped in a `<picture>` with a
//!   `<source type="image/avif">` OR `<source type="image/webp">`,
//!   the format is offered — pass.
//! * Else flag as warn — site is shipping a legacy raster only.
//!
//! Caller opt-out: `data-loom-image-legacy-ok="true"` on the
//! `<img>` (use when the image is intentionally legacy — e.g. a
//! source-format icon).
//!
//! ## Severity
//!
//! Warn. Lighthouse treats this as opportunity-budget, not
//! correctness — the user-visible payoff varies with image
//! weight. Severity may upgrade to strict in a future pass if
//! a Network event tells us the image is ≥ 50 KB.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — an `<img>` ref to a legacy raster
/// without modern alternatives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LegacyImageHit {
    /// Selector path.
    pub selector: String,
    /// Image src URL.
    pub src: String,
    /// Format detected from URL extension (`jpg` / `png` / `gif`).
    pub format: String,
    /// Rendered display width in CSS px (informational).
    pub display_width: u32,
    /// True if the `<img>` is inside a `<picture>` that ALREADY
    /// has a `<source>` — but the source isn't AVIF/WebP. Hints
    /// at an incomplete picture-element migration.
    pub has_picture_wrapper: bool,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ModernImageFormatsSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every legacy raster hit the walker found.
    pub hits: Vec<LegacyImageHit>,
    /// Total visible `<img>` elements walked.
    pub scanned: u32,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_modern_image_formats(snap: &ModernImageFormatsSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(5)
        .map(|h| {
            let wrapper = if h.has_picture_wrapper {
                " (inside <picture> but no avif/webp source)"
            } else {
                ""
            };
            format!(
                "{} {} ({}, {}px wide){}",
                h.selector, h.src, h.format, h.display_width, wrapper
            )
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Warn,
        kind: "image-format.legacy-only".to_owned(),
        detail: format!(
            "{} legacy raster image(s) shipped without an AVIF / WebP alternative. Wrap in `<picture>` with `<source type=\"image/avif\">` and/or `<source type=\"image/webp\">` to cut transfer 30-70%. Examples: {}",
            snap.hits.len(),
            examples.join("; ")
        ),
    }]
}

/// Browser-side capture.
pub const MODERN_IMAGE_FORMATS_DOM_CAPTURE_JS: &str = r#"
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

    const hits = [];
    let scanned = 0;

    const imgs = document.querySelectorAll('img[src]');
    for (let i = 0; i < imgs.length; i++) {
      const img = imgs[i];
      if (isHidden(img)) continue;
      // Caller opt-out.
      if (img.getAttribute('data-loom-image-legacy-ok') === 'true') continue;
      scanned += 1;
      const rect = img.getBoundingClientRect();
      const width = Math.round(rect.width);
      if (width < 200) continue;

      const src = img.getAttribute('src') || '';
      const lower = src.toLowerCase();
      let format = '';
      if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) format = 'jpg';
      else if (lower.endsWith('.png')) format = 'png';
      else if (lower.endsWith('.gif')) format = 'gif';
      else continue;

      // Check for picture wrapper with AVIF/WebP source.
      const picture = img.closest('picture');
      let coveredByModern = false;
      let hasPictureWrapper = false;
      if (picture) {
        hasPictureWrapper = true;
        const sources = picture.querySelectorAll('source[type]');
        for (let s = 0; s < sources.length; s++) {
          const t = (sources[s].getAttribute('type') || '').toLowerCase();
          if (t === 'image/avif' || t === 'image/webp') {
            coveredByModern = true;
            break;
          }
        }
      }
      if (coveredByModern) continue;

      hits.push({
        selector: selectorOf(img),
        src: src,
        format: format,
        displayWidth: width,
        hasPictureWrapper: hasPictureWrapper,
      });
      if (hits.length >= 50) break;
    }

    return { pageUrl: window.location.href, hits: hits, scanned: scanned };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(hits: Vec<LegacyImageHit>) -> ModernImageFormatsSnapshot {
        ModernImageFormatsSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            hits,
            scanned: 5,
        }
    }

    fn hit(src: &str, format: &str, width: u32, has_picture: bool) -> LegacyImageHit {
        LegacyImageHit {
            selector: "body > img".to_owned(),
            src: src.to_owned(),
            format: format.to_owned(),
            display_width: width,
            has_picture_wrapper: has_picture,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_modern_image_formats(&s).is_empty());
    }

    #[test]
    fn one_hit_warns() {
        let s = snap(vec![hit("/hero.jpg", "jpg", 1200, false)]);
        let f = detect_modern_image_formats(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "image-format.legacy-only");
        assert!(f[0].detail.contains("1 legacy"));
    }

    #[test]
    fn multiple_hits_aggregate() {
        let s = snap(vec![
            hit("/a.jpg", "jpg", 600, false),
            hit("/b.png", "png", 800, false),
            hit("/c.gif", "gif", 400, true),
        ]);
        let f = detect_modern_image_formats(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("3 legacy"));
    }

    #[test]
    fn picture_wrapper_note_in_detail() {
        let s = snap(vec![hit("/x.png", "png", 600, true)]);
        let f = detect_modern_image_formats(&s);
        assert!(
            f[0].detail
                .contains("inside <picture> but no avif/webp source"),
            "expected picture-wrapper note: {}",
            f[0].detail
        );
    }

    #[test]
    fn examples_capped_at_5() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!("/img-{i}.jpg"), "jpg", 800, false));
        }
        let s = snap(hits);
        let f = detect_modern_image_formats(&s);
        assert!(f[0].detail.contains("10 legacy"));
        let arrows = f[0].detail.matches(" (jpg, ").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![hit("/x.png", "png", 400, true)]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: ModernImageFormatsSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.hits[0].format, "png");
        assert!(back.hits[0].has_picture_wrapper);
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in MODERN_IMAGE_FORMATS_DOM_CAPTURE_JS.chars() {
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
    fn js_includes_opt_out_marker() {
        assert!(
            MODERN_IMAGE_FORMATS_DOM_CAPTURE_JS.contains("data-loom-image-legacy-ok"),
            "capture JS missing the caller-side opt-out marker"
        );
    }

    #[test]
    fn js_includes_avif_and_webp_checks() {
        assert!(
            MODERN_IMAGE_FORMATS_DOM_CAPTURE_JS.contains("'image/avif'"),
            "capture JS missing image/avif check"
        );
        assert!(
            MODERN_IMAGE_FORMATS_DOM_CAPTURE_JS.contains("'image/webp'"),
            "capture JS missing image/webp check"
        );
    }
}
