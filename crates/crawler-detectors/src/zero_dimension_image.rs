//! `zero_dimension_image` — `<img>` elements with 0×0 rendered box.
//!
//! Common failure modes that produce zero-dimension images:
//!
//! * Lazy-load misconfiguration: intersection observer never fires
//!   (e.g., the image is inside an `overflow: hidden` ancestor with
//!   no scroll, or its viewport-root never matches). The `loading="lazy"`
//!   image downloads but stays at 0×0 until forced.
//! * Aspect-ratio collision: `width=0` or `height=0` attribute combined
//!   with `aspect-ratio: auto` zeroes the box.
//! * CSS reset that wipes `img { width: 0 }` without re-establishing.
//! * Broken `srcset` where every candidate is unreachable.
//!
//! Why it matters:
//!
//! * The bytes are downloaded (HTTP cost) without the user ever
//!   seeing the image — pure waste.
//! * `<img alt=\"…\">` invisible to sighted users may still be
//!   announced by screen readers, creating a content/visual
//!   mismatch where the AT-user gets information the sighted
//!   user does not.
//! * Layout-shift surprises if the image later resolves to its
//!   natural dimensions.
//!
//! HEURISTIC
//! ---------
//! 1. Walk every `<img>` in the document.
//! 2. Skip images inside `<template>` / `<script>` (inert).
//! 3. Compute the rendered box: `getBoundingClientRect()`.
//! 4. Flag when EITHER `width === 0` OR `height === 0` AND the
//!    image is not `display: none` (because `display: none` is
//!    an explicit hide, not a render bug).
//! 5. Report `src`, `loading` attr, `naturalWidth/Height` so the
//!    operator can tell why the box collapsed.
//! 6. Cap surfaced offenders at 50; full count surfaces in
//!    `zeroDimCount` regardless.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = \"deny\"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const ZERO_DIMENSION_IMAGE_JS: &str = r##"(() => {
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

    const inInertContext = function(el) {
      let node = el.parentElement;
      while (node) {
        if (node.tagName === 'TEMPLATE') return true;
        if (node.tagName === 'SCRIPT') return true;
        node = node.parentElement;
      }
      return false;
    };

    const all = document.querySelectorAll('img');
    let scanned = 0;
    const offenders = [];
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (inInertContext(el)) continue;
      scanned += 1;
      const cs = window.getComputedStyle(el);
      if (cs && cs.display === 'none') continue;
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) continue;
      const src = el.getAttribute('src') || '';
      offenders.push({
        selector: selectorOf(el),
        srcPreview: src.length > 80 ? src.slice(0, 77) + '...' : src,
        loadingAttr: el.getAttribute('loading') || '',
        naturalWidth: el.naturalWidth || 0,
        naturalHeight: el.naturalHeight || 0,
        renderedWidth: Math.round(rect.width),
        renderedHeight: Math.round(rect.height)
      });
    }

    return {
      totalImg: scanned,
      offenders: offenders.slice(0, 50),
      zeroDimCount: offenders.length
    };
})()"##;

/// One zero-dimension-image row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ZeroDimImage {
    /// CSS selector to the offending `<img>`.
    pub selector: String,
    /// First 80 chars of `src` attribute, ellipsised if longer.
    pub src_preview: String,
    /// Value of `loading` attribute (`""` / `"lazy"` / `"eager"`).
    pub loading_attr: String,
    /// `naturalWidth` — the intrinsic pixel dimension the browser
    /// would render IF the image weren't collapsed.
    pub natural_width: u32,
    /// `naturalHeight` — intrinsic pixel height.
    pub natural_height: u32,
    /// Rendered box width (rounded). Will be 0 in most flag cases.
    pub rendered_width: i32,
    /// Rendered box height (rounded). Will be 0 in most flag cases.
    pub rendered_height: i32,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ZeroDimensionImageSnapshot {
    /// Total `<img>` elements walked (excluding inert-context +
    /// `display: none`).
    pub total_img: u32,
    /// Top 50 zero-dimension-image offenders.
    pub offenders: Vec<ZeroDimImage>,
    /// Total zero-dim count (may exceed `offenders.len()` if
    /// truncated).
    pub zero_dim_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits one **strict** finding when at least one `<img>` rendered
/// to 0×0 despite not being `display: none`. The bytes were
/// downloaded but the user never sees the image; AT-users may still
/// hear the `alt` text, creating a content/visual mismatch.
#[must_use]
pub fn detect_zero_dimension_image_issues(
    snap: &ZeroDimensionImageSnapshot,
) -> Vec<crate::AxisFinding> {
    if snap.offenders.is_empty() {
        return Vec::new();
    }
    let first = &snap.offenders[0];
    let loading_note = if first.loading_attr.is_empty() {
        "no loading attr".to_owned()
    } else {
        format!("loading=\"{}\"", first.loading_attr)
    };
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "zero-dimension-image.collapsed-box".to_owned(),
        detail: format!(
            "{} <img> rendered at 0×0 despite not being display:none. Total <img> scanned: {}. First offender: src=\"{}\" @ {} ({}; natural={}×{}, rendered={}×{})",
            snap.zero_dim_count,
            snap.total_img,
            first.src_preview,
            first.selector,
            loading_note,
            first.natural_width,
            first.natural_height,
            first.rendered_width,
            first.rendered_height,
        ),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            ZERO_DIMENSION_IMAGE_JS.matches('(').count(),
            ZERO_DIMENSION_IMAGE_JS.matches(')').count()
        );
        assert_eq!(
            ZERO_DIMENSION_IMAGE_JS.matches('{').count(),
            ZERO_DIMENSION_IMAGE_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(ZERO_DIMENSION_IMAGE_JS.starts_with("(() => {"));
        assert!(ZERO_DIMENSION_IMAGE_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "totalImg",
            "offenders",
            "zeroDimCount",
            "selector",
            "srcPreview",
            "loadingAttr",
            "naturalWidth",
            "naturalHeight",
            "renderedWidth",
            "renderedHeight",
        ] {
            assert!(ZERO_DIMENSION_IMAGE_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_walks_img_tag() {
        assert!(ZERO_DIMENSION_IMAGE_JS.contains("querySelectorAll('img')"));
    }

    #[test]
    fn js_skips_display_none() {
        // display:none images are intentionally hidden; not a bug.
        assert!(ZERO_DIMENSION_IMAGE_JS.contains("'none'"));
        assert!(ZERO_DIMENSION_IMAGE_JS.contains("getComputedStyle"));
    }

    #[test]
    fn js_uses_bounding_rect_not_attribute() {
        // Heuristic uses rendered geometry, not attribute values —
        // an image with `width=400` attribute but collapsed by CSS
        // still counts as zero-dim.
        assert!(ZERO_DIMENSION_IMAGE_JS.contains("getBoundingClientRect"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = ZeroDimensionImageSnapshot {
            total_img: 12,
            offenders: vec![],
            zero_dim_count: 0,
        };
        let findings = detect_zero_dimension_image_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn zero_dim_image_emits_strict() {
        let snap = ZeroDimensionImageSnapshot {
            total_img: 4,
            offenders: vec![ZeroDimImage {
                selector: "body > main > img:nth-of-type(2)".to_owned(),
                src_preview: "/assets/hero.webp".to_owned(),
                loading_attr: "lazy".to_owned(),
                natural_width: 1920,
                natural_height: 1080,
                rendered_width: 0,
                rendered_height: 0,
            }],
            zero_dim_count: 1,
        };
        let findings = detect_zero_dimension_image_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "zero-dimension-image.collapsed-box");
        assert!(findings[0].detail.contains("0×0"));
        assert!(findings[0].detail.contains("/assets/hero.webp"));
        assert!(findings[0].detail.contains("loading=\"lazy\""));
        assert!(findings[0].detail.contains("natural=1920×1080"));
    }

    #[test]
    fn missing_loading_attr_renders_as_placeholder() {
        let snap = ZeroDimensionImageSnapshot {
            total_img: 1,
            offenders: vec![ZeroDimImage {
                selector: "x".to_owned(),
                src_preview: "/a.png".to_owned(),
                loading_attr: String::new(),
                natural_width: 100,
                natural_height: 100,
                rendered_width: 0,
                rendered_height: 0,
            }],
            zero_dim_count: 1,
        };
        let findings = detect_zero_dimension_image_issues(&snap);
        assert!(findings[0].detail.contains("no loading attr"));
    }

    #[test]
    fn truncated_count_surfaces_higher_than_array_len() {
        let snap = ZeroDimensionImageSnapshot {
            total_img: 200,
            offenders: vec![ZeroDimImage {
                selector: "x".to_owned(),
                src_preview: "/a.png".to_owned(),
                loading_attr: "lazy".to_owned(),
                natural_width: 100,
                natural_height: 100,
                rendered_width: 0,
                rendered_height: 0,
            }],
            zero_dim_count: 73,
        };
        let findings = detect_zero_dimension_image_issues(&snap);
        assert!(findings[0].detail.contains("73 <img>"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = ZeroDimensionImageSnapshot {
            total_img: 5,
            offenders: vec![ZeroDimImage {
                selector: "body > img".to_owned(),
                src_preview: "/x.jpg".to_owned(),
                loading_attr: "eager".to_owned(),
                natural_width: 640,
                natural_height: 480,
                rendered_width: 0,
                rendered_height: 12,
            }],
            zero_dim_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"totalImg\":5"));
        assert!(json.contains("\"zeroDimCount\":1"));
        assert!(json.contains("\"loadingAttr\":\"eager\""));
        let back: ZeroDimensionImageSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].natural_width, 640);
    }

    #[test]
    fn detail_includes_total_img_count() {
        let snap = ZeroDimensionImageSnapshot {
            total_img: 42,
            offenders: vec![ZeroDimImage {
                selector: "x".to_owned(),
                src_preview: "/x.jpg".to_owned(),
                loading_attr: "lazy".to_owned(),
                natural_width: 100,
                natural_height: 100,
                rendered_width: 0,
                rendered_height: 0,
            }],
            zero_dim_count: 1,
        };
        let findings = detect_zero_dimension_image_issues(&snap);
        assert!(findings[0].detail.contains("Total <img> scanned: 42"));
    }

    #[test]
    fn one_dimension_zero_other_nonzero_still_flags() {
        // height==0 with width>0 still counts as collapsed (no
        // visible pixels regardless).
        let snap = ZeroDimensionImageSnapshot {
            total_img: 1,
            offenders: vec![ZeroDimImage {
                selector: "x".to_owned(),
                src_preview: "/x.jpg".to_owned(),
                loading_attr: String::new(),
                natural_width: 200,
                natural_height: 100,
                rendered_width: 320,
                rendered_height: 0,
            }],
            zero_dim_count: 1,
        };
        let findings = detect_zero_dimension_image_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("rendered=320×0"));
    }
}
