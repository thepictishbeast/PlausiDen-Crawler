//! `lazy_above_fold` — flag `loading="lazy"` on above-the-fold images.
//!
//! Forward step on rolling task #117 (Crawler axes). Companion to
//! `image_dimensions`, `font_loading`, `fouc_risk` — all of which
//! touch the first-paint / LCP performance dimension.
//!
//! ## The bug class
//!
//! `<img loading="lazy">` defers the image until the browser
//! computes it might enter the viewport. For images BELOW the
//! fold, this is the correct optimization — it saves bandwidth +
//! gets the first paint up faster.
//!
//! For images ABOVE the fold (hero image, banner logo, first
//! card art), `loading="lazy"` is actively harmful: the image
//! that should arrive AS the page renders gets queued behind
//! the JS / CSS / fold-discovery work the browser has to do
//! first. LCP regresses by 100-800ms in real-world measurements.
//!
//! Web.dev's LCP guide is explicit: the LCP element should
//! NEVER have `loading="lazy"`. Use `loading="eager"` (default
//! for images without the attribute) or, for the known LCP
//! image, add `fetchpriority="high"` to bump it up the queue
//! over other resources.
//!
//! ## Findings
//!
//! * `lazy-above-fold.lcp-image` strict — `<img loading="lazy">`
//!   whose bounding box top is within the viewport at capture
//!   time. This is the LCP candidate; lazy on it ships a
//!   measurable LCP regression.
//! * `lazy-above-fold.lcp-image-with-priority` warn — same shape
//!   but the operator added `fetchpriority="high"` partially
//!   mitigating. Still surface — web.dev says don't do that.
//! * `lazy-above-fold.partially-visible` warn — `<img
//!   loading="lazy">` whose top is BELOW viewport but whose
//!   declared width indicates it likely intrudes into smaller
//!   breakpoints. Warn only because cross-viewport walk is the
//!   proper detector (future work).
//!
//! ## Heuristic
//!
//! Single visit at the captured viewport. The detector consumes
//! a snapshot of `<img>` elements with their bounding boxes +
//! loading attribute. Strict-fires when `loading == "lazy"` AND
//! `bounding_box_top < viewport_height`.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured `<img>` element with its bounding-box geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LazyAboveFoldEntry {
    /// CSS-ish selector pointing at the image element.
    pub selector: String,
    /// `src` value as-authored (capped at 200 chars for context).
    pub src: String,
    /// Loading attribute value as-authored. None means absent
    /// (browser default = eager); present-and-lazy is the bug.
    pub loading: Option<String>,
    /// Optional fetchpriority value as-authored. `Some("high")`
    /// partially excuses lazy on a known-LCP image (operator
    /// signaled intent) — phase downgrades severity in that case.
    pub fetchpriority: Option<String>,
    /// Bounding-box top in CSS pixels at the captured viewport.
    pub bbox_top: i32,
    /// Bounding-box bottom in CSS pixels at the captured viewport.
    pub bbox_bottom: i32,
    /// Element width (rendered, in CSS px).
    pub width: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LazyAboveFoldSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Viewport height at capture time (CSS px).
    pub viewport_height: u32,
    /// Captured offenders (images with bounding-box geometry).
    pub entries: Vec<LazyAboveFoldEntry>,
}

/// Page-side eval. Walks every `<img>` element, captures its
/// bounding box + loading + fetchpriority attributes.
pub const LAZY_ABOVE_FOLD_JS: &str = r##"(() => {
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
      return 'body > ' + parts.join(' > ');
    };
    const truncate = function(s) { return (s || '').slice(0, 200); };
    const entries = [];
    const imgs = document.querySelectorAll('img');
    for (let i = 0; i < imgs.length; i++) {
      const el = imgs[i];
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      const loading = el.getAttribute('loading');
      const fp = el.getAttribute('fetchpriority');
      entries.push({
        selector: selectorOf(el),
        src: truncate(el.getAttribute('src') || ''),
        loading: loading,
        fetchpriority: fp,
        bboxTop: Math.round(rect.top),
        bboxBottom: Math.round(rect.bottom),
        width: Math.round(rect.width)
      });
    }
    return {
      pageUrl: location.href,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      entries: entries
    };
  })()"##;

/// Pure detector: snapshot → findings.
///
/// Strict fires for any `loading="lazy"` image whose bounding-
/// box top is within the viewport at capture time. Warn fires
/// when the image is partially visible (top close to fold AND
/// width large enough that smaller breakpoints likely intrude).
///
/// Fetchpriority="high" on an above-fold lazy image DOWNGRADES
/// strict → warn — operator signaled intent.
#[must_use]
pub fn detect_lazy_above_fold(snap: &LazyAboveFoldSnapshot) -> Vec<AxisFinding> {
    let mut findings = Vec::new();
    let vh = snap.viewport_height as i32;
    for entry in &snap.entries {
        let is_lazy = entry.loading.as_deref() == Some("lazy");
        if !is_lazy {
            continue;
        }
        let above_fold = entry.bbox_top < vh;
        if above_fold {
            let priority_signal = entry.fetchpriority.as_deref() == Some("high");
            let severity = if priority_signal {
                AxisSeverity::Warn
            } else {
                AxisSeverity::Strict
            };
            let kind = if priority_signal {
                "lazy-above-fold.lcp-image-with-priority"
            } else {
                "lazy-above-fold.lcp-image"
            };
            let priority_hint = if priority_signal {
                " (fetchpriority=\"high\" partially mitigates, but lazy on the LCP element still regresses)"
            } else {
                ""
            };
            findings.push(AxisFinding {
                severity,
                kind: kind.to_owned(),
                detail: format!(
                    "{} — `<img loading=\"lazy\">` above the fold (bbox top={}px, viewport height={}px); LCP regression. Remove `loading=\"lazy\"` or add `fetchpriority=\"high\"`{}. src=\"{}\"",
                    entry.selector, entry.bbox_top, vh, priority_hint, entry.src
                ),
            });
            continue;
        }
        let vw = snap.viewport_width;
        let third_of_viewport = vw / 3;
        if entry.width >= third_of_viewport && entry.bbox_top < vh + 200 {
            findings.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "lazy-above-fold.partially-visible".to_owned(),
                detail: format!(
                    "{} — `<img loading=\"lazy\">` close to fold (bbox top={}px, viewport height={}px); larger than 33% of viewport width — may intrude on smaller breakpoints. Verify across 390/768/1280 viewports. src=\"{}\"",
                    entry.selector, entry.bbox_top, vh, entry.src
                ),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(loading: Option<&str>, bbox_top: i32, width: u32) -> LazyAboveFoldEntry {
        LazyAboveFoldEntry {
            selector: "body > img".to_owned(),
            src: "/hero.jpg".to_owned(),
            loading: loading.map(str::to_owned),
            fetchpriority: None,
            bbox_top,
            bbox_bottom: bbox_top + 400,
            width,
        }
    }

    fn snap(entries: Vec<LazyAboveFoldEntry>) -> LazyAboveFoldSnapshot {
        LazyAboveFoldSnapshot {
            page_url: "https://example.test/".to_owned(),
            viewport_width: 1280,
            viewport_height: 800,
            entries,
        }
    }

    #[test]
    fn empty_entries_emit_no_findings() {
        assert!(detect_lazy_above_fold(&snap(Vec::new())).is_empty());
    }

    #[test]
    fn lazy_above_fold_image_is_strict() {
        let findings =
            detect_lazy_above_fold(&snap(vec![entry(Some("lazy"), 100, 1200)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "lazy-above-fold.lcp-image");
        assert!(findings[0].detail.contains("LCP regression"));
    }

    #[test]
    fn lazy_below_fold_image_silent() {
        let findings =
            detect_lazy_above_fold(&snap(vec![entry(Some("lazy"), 1200, 200)]));
        assert!(findings.is_empty());
    }

    #[test]
    fn non_lazy_image_silent_regardless_of_position() {
        assert!(detect_lazy_above_fold(&snap(vec![entry(None, 0, 1200)])).is_empty());
        assert!(detect_lazy_above_fold(&snap(vec![entry(Some("eager"), 0, 1200)])).is_empty());
    }

    #[test]
    fn lazy_at_zero_top_is_above_fold_strict() {
        let findings =
            detect_lazy_above_fold(&snap(vec![entry(Some("lazy"), 0, 800)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn lazy_just_below_viewport_with_wide_image_warns_partially_visible() {
        let findings = detect_lazy_above_fold(&snap(vec![entry(
            Some("lazy"),
            850,
            800,
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "lazy-above-fold.partially-visible");
    }

    #[test]
    fn lazy_far_below_fold_silent_even_when_wide() {
        let findings = detect_lazy_above_fold(&snap(vec![entry(
            Some("lazy"),
            1500,
            800,
        )]));
        assert!(findings.is_empty());
    }

    #[test]
    fn fetchpriority_high_downgrades_above_fold_lazy_to_warn() {
        let mut e = entry(Some("lazy"), 100, 1200);
        e.fetchpriority = Some("high".to_owned());
        let findings = detect_lazy_above_fold(&snap(vec![e]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "lazy-above-fold.lcp-image-with-priority");
        assert!(findings[0].detail.contains("fetchpriority"));
    }

    #[test]
    fn multiple_lazy_above_fold_images_emit_one_finding_each() {
        let findings = detect_lazy_above_fold(&snap(vec![
            entry(Some("lazy"), 50, 1200),
            entry(Some("lazy"), 100, 1200),
            entry(Some("lazy"), 200, 1200),
        ]));
        assert_eq!(findings.len(), 3);
        for f in &findings {
            assert_eq!(f.kind, "lazy-above-fold.lcp-image");
            assert_eq!(f.severity, AxisSeverity::Strict);
        }
    }

    #[test]
    fn snapshot_serde_camel_case() {
        let s = snap(vec![entry(Some("lazy"), 0, 100)]);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pageUrl\""));
        assert!(j.contains("\"viewportWidth\""));
        assert!(j.contains("\"viewportHeight\""));
        assert!(j.contains("\"bboxTop\""));
        assert!(j.contains("\"bboxBottom\""));
        assert!(j.contains("\"fetchpriority\""));
        let back: LazyAboveFoldSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn js_eval_const_includes_expected_capture_fields() {
        assert!(LAZY_ABOVE_FOLD_JS.contains("querySelectorAll('img')"));
        assert!(LAZY_ABOVE_FOLD_JS.contains("getBoundingClientRect()"));
        assert!(LAZY_ABOVE_FOLD_JS.contains("loading"));
        assert!(LAZY_ABOVE_FOLD_JS.contains("fetchpriority"));
        assert!(LAZY_ABOVE_FOLD_JS.contains("bboxTop"));
    }
}
