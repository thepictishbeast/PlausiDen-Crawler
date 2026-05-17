//! `image_dimensions` — Cumulative Layout Shift prevention via
//! explicit `<img>` dimensions.
//!
//! Per the WebPlatform CLS guidance: `<img>` (and `<video>`,
//! `<picture>`) elements without `width` + `height` attributes
//! force the browser to reflow once each image loads, which is
//! the largest contributor to CLS scores in real-user metrics.
//! Explicit width + height (or an `aspect-ratio` CSS rule) lets
//! the browser reserve correct space before the bytes arrive.
//!
//! This detector ALSO flags declared-aspect vs intrinsic-aspect
//! mismatches that exceed a small tolerance — distortion bugs
//! that ship looking "almost right" but degrade content
//! presentation on every page load.
//!
//! Findings:
//!   * `image-dimensions.missing-both` strict   no width AND no height
//!                                                AND no CSS aspect-ratio
//!   * `image-dimensions.missing-one`  warn     exactly one of
//!                                                width/height set
//!   * `image-dimensions.aspect-mismatch` warn  declared aspect differs
//!                                                from intrinsic by > 5%
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Aspect-ratio tolerance — declared vs intrinsic deltas inside
/// this window don't fire. 5% accounts for sub-pixel rounding +
/// retina-doubling artifacts without missing real distortion.
pub const ASPECT_TOLERANCE: f64 = 0.05;

/// One captured image element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ImageDimensionEntry {
    /// CSS selector pointing at the `<img>` (or media element).
    pub selector: String,
    /// `src` value as-authored.
    pub src: String,
    /// Declared HTML `width` attribute (None if absent).
    pub html_width: Option<u32>,
    /// Declared HTML `height` attribute (None if absent).
    pub html_height: Option<u32>,
    /// Whether the element has a CSS `aspect-ratio` rule active.
    pub has_css_aspect_ratio: bool,
    /// Intrinsic image width (from loaded asset). 0 if not loaded
    /// at capture time.
    pub natural_width: u32,
    /// Intrinsic image height (from loaded asset).
    pub natural_height: u32,
}

/// Captured set of image elements on the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ImageDimensionsSnapshot {
    /// Page URL.
    pub page_url: String,
    /// All `<img>` / `<picture>` / `<video poster>` elements
    /// the crawler captured.
    pub images: Vec<ImageDimensionEntry>,
}

/// Page-side eval. Collects images + their declared dimensions +
/// computed `aspect-ratio` presence.
pub const IMAGE_DIMENSIONS_JS: &str = r##"(() => {
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

    const out = [];
    const imgs = document.querySelectorAll('img');
    for (let i = 0; i < imgs.length; i++) {
        const el = imgs[i];
        const cs = window.getComputedStyle(el);
        const widthAttr = el.getAttribute('width');
        const heightAttr = el.getAttribute('height');
        const parseIntOrNull = function(s) {
            if (s == null) return null;
            const n = parseInt(s, 10);
            return isNaN(n) ? null : n;
        };
        out.push({
            selector: selectorOf(el),
            src: el.getAttribute('src') || '',
            htmlWidth: parseIntOrNull(widthAttr),
            htmlHeight: parseIntOrNull(heightAttr),
            hasCssAspectRatio: cs.aspectRatio !== 'auto' && cs.aspectRatio !== '',
            naturalWidth: el.naturalWidth || 0,
            naturalHeight: el.naturalHeight || 0
        });
    }
    return { pageUrl: window.location.href, images: out };
})()"##;

fn aspect_mismatch(entry: &ImageDimensionEntry) -> Option<(f64, f64)> {
    let (w, h) = (entry.html_width?, entry.html_height?);
    if w == 0 || h == 0 || entry.natural_width == 0 || entry.natural_height == 0 {
        return None;
    }
    let declared = w as f64 / h as f64;
    let intrinsic = entry.natural_width as f64 / entry.natural_height as f64;
    let delta = (declared - intrinsic).abs() / intrinsic;
    if delta > ASPECT_TOLERANCE {
        Some((declared, intrinsic))
    } else {
        None
    }
}

/// Run the detector.
pub fn detect_image_dimension_issues(snap: &ImageDimensionsSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    for img in &snap.images {
        let has_w = img.html_width.is_some();
        let has_h = img.html_height.is_some();
        let has_aspect = img.has_css_aspect_ratio;

        match (has_w, has_h, has_aspect) {
            (false, false, false) => {
                out.push(AxisFinding {
                    severity: AxisSeverity::Strict,
                    kind: "image-dimensions.missing-both".into(),
                    detail: format!(
                        "<img src=\"{}\"> has neither width/height nor CSS aspect-ratio; will trigger CLS ({})",
                        img.src, img.selector
                    ),
                });
            }
            (true, false, false) | (false, true, false) => {
                out.push(AxisFinding {
                    severity: AxisSeverity::Warn,
                    kind: "image-dimensions.missing-one".into(),
                    detail: format!(
                        "<img src=\"{}\"> sets only one of width/height; CLS prevention requires both ({})",
                        img.src, img.selector
                    ),
                });
            }
            _ => {}
        }
        if let Some((decl, nat)) = aspect_mismatch(img) {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "image-dimensions.aspect-mismatch".into(),
                detail: format!(
                    "<img src=\"{}\"> declared aspect {:.3} differs from intrinsic {:.3} by > {:.0}% ({})",
                    img.src,
                    decl,
                    nat,
                    ASPECT_TOLERANCE * 100.0,
                    img.selector
                ),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(src: &str, w: Option<u32>, h: Option<u32>, aspect: bool) -> ImageDimensionEntry {
        ImageDimensionEntry {
            selector: format!("img[src={}]", src),
            src: src.to_string(),
            html_width: w,
            html_height: h,
            has_css_aspect_ratio: aspect,
            natural_width: 0,
            natural_height: 0,
        }
    }

    fn img_loaded(
        src: &str,
        w: Option<u32>,
        h: Option<u32>,
        nat: (u32, u32),
    ) -> ImageDimensionEntry {
        ImageDimensionEntry {
            selector: format!("img[src={}]", src),
            src: src.to_string(),
            html_width: w,
            html_height: h,
            has_css_aspect_ratio: false,
            natural_width: nat.0,
            natural_height: nat.1,
        }
    }

    fn snap(images: Vec<ImageDimensionEntry>) -> ImageDimensionsSnapshot {
        ImageDimensionsSnapshot {
            page_url: "https://example.com/".into(),
            images,
        }
    }

    #[test]
    fn explicit_dimensions_are_clean() {
        let s = snap(vec![img("a.jpg", Some(100), Some(200), false)]);
        assert!(detect_image_dimension_issues(&s).is_empty());
    }

    #[test]
    fn missing_both_fires_strict() {
        let s = snap(vec![img("a.jpg", None, None, false)]);
        let f = detect_image_dimension_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "image-dimensions.missing-both");
    }

    #[test]
    fn missing_one_fires_warn() {
        let s = snap(vec![img("a.jpg", Some(100), None, false)]);
        let f = detect_image_dimension_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "image-dimensions.missing-one");
    }

    #[test]
    fn css_aspect_ratio_covers_missing_dimensions() {
        let s = snap(vec![img("a.jpg", None, None, true)]);
        assert!(detect_image_dimension_issues(&s).is_empty());
    }

    #[test]
    fn aspect_mismatch_fires_warn() {
        // Declared 200x200 (1:1), intrinsic 200x100 (2:1) — 100% delta.
        let s = snap(vec![img_loaded("a.jpg", Some(200), Some(200), (200, 100))]);
        let f = detect_image_dimension_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "image-dimensions.aspect-mismatch"));
    }

    #[test]
    fn small_aspect_delta_within_tolerance() {
        // Declared 100x100, intrinsic 102x100 — 2% delta, within 5%.
        let s = snap(vec![img_loaded("a.jpg", Some(100), Some(100), (102, 100))]);
        let f = detect_image_dimension_issues(&s);
        assert!(f.is_empty());
    }

    #[test]
    fn multiple_images_collect_findings() {
        let s = snap(vec![
            img("a.jpg", None, None, false),           // strict
            img("b.jpg", Some(100), None, false),      // warn
            img("c.jpg", Some(100), Some(100), false), // clean
        ]);
        let f = detect_image_dimension_issues(&s);
        assert_eq!(f.len(), 2);
    }
}
