//! `runtime_images` — image-quality detector.
//!
//! Walks every `<img>` and emits 4 offender lists:
//!
//! * `empty_src`    — `src=""` or missing
//! * `broken`       — `complete=true` but `naturalWidth=0`
//! * `missing_alt`  — `alt` attribute absent (decorative needs `alt=""`)
//! * `cls_risk`     — visible, no explicit width+height + no
//!                    `aspect-ratio` CSS — guaranteed CLS

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const RUNTIME_IMAGES_JS: &str = r##"(() => {
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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      return true;
    };

    const broken = [];
    const emptySrc = [];
    const missingAlt = [];
    const clsRisk = [];
    let totalImages = 0;

    const imgs = document.querySelectorAll('img');
    for (let i = 0; i < imgs.length; i++) {
      const el = imgs[i];
      totalImages += 1;
      const visible = isVisible(el);
      const cs = window.getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      const src = el.getAttribute('src') || '';
      const alt = el.getAttribute('alt');
      const widthAttr = el.getAttribute('width');
      const heightAttr = el.getAttribute('height');
      const hasExplicitDims = !!(widthAttr && heightAttr);
      const hasAspectRatio = !!(cs.aspectRatio && cs.aspectRatio !== 'auto');
      const isDecorative = alt === '';

      const offender = {
        selector: selectorOf(el),
        src: src,
        alt: alt,
        naturalWidth: el.naturalWidth,
        naturalHeight: el.naturalHeight,
        complete: el.complete,
        width: Math.round(rect.width),
        height: Math.round(rect.height),
        hasExplicitDims: hasExplicitDims,
        hasAspectRatio: hasAspectRatio,
        isVisible: visible,
        isDecorative: isDecorative
      };

      if (!src || src.trim() === '') {
        emptySrc.push(offender);
        continue;
      }
      if (el.complete && el.naturalWidth === 0) {
        broken.push(offender);
      }
      if (alt === null) {
        missingAlt.push(offender);
      }
      if (visible && !hasExplicitDims && !hasAspectRatio) {
        clsRisk.push(offender);
      }
    }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      totalImages: totalImages,
      broken: broken,
      emptySrc: emptySrc,
      missingAlt: missingAlt,
      clsRisk: clsRisk
    };
})()"##;

/// One image-offender row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ImageOffender {
    /// Best-effort CSS selector.
    pub selector: String,
    /// `src` attribute value (may be empty).
    pub src: String,
    /// `alt` attribute value (`null` if attribute missing entirely;
    /// empty string `""` if explicitly decorative).
    pub alt: Option<String>,
    /// `HTMLImageElement.naturalWidth`.
    pub natural_width: u32,
    /// `HTMLImageElement.naturalHeight`.
    pub natural_height: u32,
    /// `HTMLImageElement.complete` — load attempt finished.
    pub complete: bool,
    /// Rendered width.
    pub width: i32,
    /// Rendered height.
    pub height: i32,
    /// Both `width=` and `height=` HTML attributes set.
    pub has_explicit_dims: bool,
    /// CSS `aspect-ratio` is non-`auto`.
    pub has_aspect_ratio: bool,
    /// Currently visible per `isVisible` heuristic.
    pub is_visible: bool,
    /// `alt=""` (decorative — explicit empty alt).
    pub is_decorative: bool,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct RuntimeImagesSnapshot {
    /// `window.innerWidth`.
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight`.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// Total `<img>` count.
    pub total_images: u32,
    /// `complete=true && naturalWidth=0`.
    pub broken: Vec<ImageOffender>,
    /// `src` empty or missing.
    pub empty_src: Vec<ImageOffender>,
    /// `alt` attribute missing entirely.
    pub missing_alt: Vec<ImageOffender>,
    /// Visible, no explicit width+height, no aspect-ratio.
    pub cls_risk: Vec<ImageOffender>,
}

/// Apply detection rules to a runtime-images snapshot. Pure function.
/// Mirrors the TS `detectRuntimeImageIssues`.
#[must_use]
pub fn detect_runtime_image_issues(snap: &RuntimeImagesSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    if !snap.broken.is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "images.broken".to_owned(),
            detail: format!(
                "{} <img> tag(s) failed to load (naturalWidth=0 with complete=true). Visitors see broken-image icons.",
                snap.broken.len()
            ),
        });
    }
    if !snap.empty_src.is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "images.empty-src".to_owned(),
            detail: format!(
                "{} <img> tag(s) have empty or missing src attribute.",
                snap.empty_src.len()
            ),
        });
    }
    if !snap.missing_alt.is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "images.missing-alt-attr".to_owned(),
            detail: format!(
                "{} <img> tag(s) have no alt attribute. Screen readers announce filename instead. (Decorative images need alt=\"\", not missing attribute.)",
                snap.missing_alt.len()
            ),
        });
    }
    if !snap.cls_risk.is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "images.cls-risk".to_owned(),
            detail: format!(
                "{} visible <img> tag(s) lack both explicit width+height attributes AND CSS aspect-ratio. Page will shift as images load (CLS).",
                snap.cls_risk.len()
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_balanced() {
        assert_eq!(
            RUNTIME_IMAGES_JS.matches('(').count(),
            RUNTIME_IMAGES_JS.matches(')').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(RUNTIME_IMAGES_JS.starts_with("(() => {"));
        assert!(RUNTIME_IMAGES_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW",
            "vpH",
            "totalImages",
            "broken",
            "emptySrc",
            "missingAlt",
            "clsRisk",
        ] {
            assert!(RUNTIME_IMAGES_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn snapshot_round_trips() {
        let snap = RuntimeImagesSnapshot {
            vp_w: 1280,
            vp_h: 800,
            total_images: 1,
            broken: vec![],
            empty_src: vec![],
            missing_alt: vec![ImageOffender {
                selector: "body > img".to_owned(),
                src: "x.jpg".to_owned(),
                alt: None,
                natural_width: 100,
                natural_height: 100,
                complete: true,
                width: 100,
                height: 100,
                has_explicit_dims: true,
                has_aspect_ratio: false,
                is_visible: true,
                is_decorative: false,
            }],
            cls_risk: vec![],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: RuntimeImagesSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.missing_alt.len(), 1);
        assert_eq!(back.missing_alt[0].alt, None);
    }
}
