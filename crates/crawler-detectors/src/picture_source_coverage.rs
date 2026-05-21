//! `picture_source_coverage` — audits `<picture>` mis-
//! configurations. Complements [`crate::modern_image_formats`]
//! which audits bare `<img>` elements; this axis stays inside
//! `<picture>` and flags shape mistakes the picker silently
//! tolerates:
//!
//! 1. **Missing `<img>` fallback** (Strict). A `<picture>` MUST
//!    contain exactly one `<img>` child — the browser uses it
//!    when no `<source>` matches AND for accessibility (alt
//!    text lives on `<img>`). Without it the picture renders
//!    nothing on no-source-match paths AND has no accessible
//!    name.
//!
//! 2. **No `<source>` tags** (Warn). `<picture><img></picture>`
//!    with no `<source>` inside is just dead chrome — replace
//!    with a plain `<img>`. Operator probably intended to add
//!    sources later and forgot.
//!
//! 3. **Missing AVIF** (Warn). Modern picker should include
//!    `<source type="image/avif">` before `<source
//!    type="image/webp">` before the `<img>` JPEG fallback.
//!    AVIF saves another 30-50% over WebP on equivalent
//!    quality settings. WebP-only `<picture>` is a stale
//!    pattern.
//!
//! 4. **Missing WebP** (Warn). Even older sites should offer
//!    WebP as a baseline modern format. AVIF without WebP
//!    backup omits coverage for browsers that haven't picked
//!    up AVIF yet (Safari < 16, older Android Chromium forks).
//!
//! Honors `data-picture-allow="true"` opt-out on a `<picture>`
//! for legitimate stylistic-source pictures where the operator
//! has declared the limited shape intentional.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<picture>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PictureSourceCoverageHit {
    /// CSS-ish path of the offending `<picture>`.
    pub selector: String,
    /// `src` of the fallback `<img>` (empty when missing).
    pub img_src: String,
    /// `alt` of the fallback `<img>` (capped 60 chars; empty
    /// when missing).
    pub alt: String,
    /// MIME types captured from `<source type="…">` children
    /// (in document order).
    pub source_types: Vec<String>,
    /// Whether a `<source type="image/avif">` is present.
    pub has_avif: bool,
    /// Whether a `<source type="image/webp">` is present.
    pub has_webp: bool,
    /// Whether the `<picture>` contains a fallback `<img>`.
    pub has_fallback_img: bool,
    /// Defect kind — one of `"no-fallback-img"`, `"no-source"`,
    /// `"missing-avif"`, `"missing-webp"`. Multiple defects on
    /// the same `<picture>` emit multiple hits (one per kind)
    /// so each finding bucket counts independently.
    pub defect_kind: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PictureSourceCoverageSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every defect-tagged `<picture>` hit.
    pub hits: Vec<PictureSourceCoverageHit>,
    /// Total `<picture>` elements walked.
    pub scanned_pictures: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_picture_source_coverage(snap: &PictureSourceCoverageSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut no_fallback: Vec<&PictureSourceCoverageHit> = Vec::new();
    let mut no_source: Vec<&PictureSourceCoverageHit> = Vec::new();
    let mut missing_avif: Vec<&PictureSourceCoverageHit> = Vec::new();
    let mut missing_webp: Vec<&PictureSourceCoverageHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "no-fallback-img" => no_fallback.push(h),
            "no-source" => no_source.push(h),
            "missing-avif" => missing_avif.push(h),
            "missing-webp" => missing_webp.push(h),
            _ => {} // defensive: unknown defect kinds dropped
        }
    }

    let format_example = |h: &PictureSourceCoverageHit| -> String {
        let alt = if h.alt.is_empty() {
            String::new()
        } else {
            format!(" alt=\"{}\"", h.alt)
        };
        let types = if h.source_types.is_empty() {
            "no <source>".to_owned()
        } else {
            format!("[{}]", h.source_types.join(", "))
        };
        let img = if h.img_src.is_empty() {
            "<no fallback img>".to_owned()
        } else {
            format!("img=`{}`", h.img_src)
        };
        format!("{}{} ({}, {})", h.selector, alt, types, img)
    };

    let mut out = Vec::new();
    if !no_fallback.is_empty() {
        let examples: Vec<String> = no_fallback
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "picture-source-coverage.no-fallback-img".to_owned(),
            detail: format!(
                "{} <picture> element(s) missing a fallback <img> — when no <source> matches the browser renders nothing AND assistive tech has no accessible name. Add a baseline `<img alt=\"…\" src=\"…\">` as the last child. Examples: {}",
                no_fallback.len(),
                examples.join("; ")
            ),
        });
    }
    if !no_source.is_empty() {
        let examples: Vec<String> = no_source
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "picture-source-coverage.no-source".to_owned(),
            detail: format!(
                "{} <picture> element(s) wrap an <img> without any <source> — dead chrome that should be replaced with a plain <img>. Operator probably intended to add modern-format sources and forgot. Examples: {}",
                no_source.len(),
                examples.join("; ")
            ),
        });
    }
    if !missing_avif.is_empty() {
        let examples: Vec<String> = missing_avif
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "picture-source-coverage.missing-avif".to_owned(),
            detail: format!(
                "{} <picture> element(s) omit `<source type=\"image/avif\">` — AVIF saves another 30-50% over WebP. Add an AVIF source above the WebP one. Examples: {}",
                missing_avif.len(),
                examples.join("; ")
            ),
        });
    }
    if !missing_webp.is_empty() {
        let examples: Vec<String> = missing_webp
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "picture-source-coverage.missing-webp".to_owned(),
            detail: format!(
                "{} <picture> element(s) omit `<source type=\"image/webp\">` — AVIF without WebP backup misses browsers that haven't picked up AVIF (Safari < 16, older Android Chromium). Examples: {}",
                missing_webp.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<picture>`
/// element, collects its `<source type="…">` MIME values + the
/// fallback `<img>` shape, emits one hit per detected defect
/// kind.
///
/// Mirror any change in this file's `PictureSourceCoverageHit`
/// + snapshot fields.
pub const PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
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

    const hits = [];
    let scanned = 0;
    const pictures = document.querySelectorAll('picture');
    for (const pic of pictures) {
      // Opt-out for operator-intentional limited-shape pictures.
      if (pic.getAttribute && pic.getAttribute('data-picture-allow') === 'true') continue;
      scanned += 1;
      const sources = Array.from(pic.querySelectorAll(':scope > source'));
      const sourceTypes = sources
        .map(function(s) { return (s.getAttribute('type') || '').trim().toLowerCase(); })
        .filter(function(t) { return t !== ''; });
      const hasAvif = sourceTypes.indexOf('image/avif') !== -1;
      const hasWebp = sourceTypes.indexOf('image/webp') !== -1;
      const img = pic.querySelector(':scope > img');
      const hasFallbackImg = img != null;
      const imgSrc = img ? (img.getAttribute('src') || '').trim() : '';
      const alt = img ? ((img.getAttribute('alt') || '').substring(0, 60)) : '';

      const baseHit = {
        selector: selectorOf(pic),
        imgSrc: imgSrc,
        alt: alt,
        sourceTypes: sourceTypes,
        hasAvif: hasAvif,
        hasWebp: hasWebp,
        hasFallbackImg: hasFallbackImg
      };

      if (!hasFallbackImg) {
        hits.push(Object.assign({}, baseHit, { defectKind: 'no-fallback-img' }));
      }
      if (sources.length === 0 && hasFallbackImg) {
        // No-source AND has fallback — dead wrapping. (If both
        // no-source AND no-fallback, the no-fallback finding
        // already captured the worse defect; this avoids
        // double-counting an obviously broken picture.)
        hits.push(Object.assign({}, baseHit, { defectKind: 'no-source' }));
      }
      if (sources.length > 0) {
        if (!hasAvif) {
          hits.push(Object.assign({}, baseHit, { defectKind: 'missing-avif' }));
        }
        if (!hasWebp) {
          hits.push(Object.assign({}, baseHit, { defectKind: 'missing-webp' }));
        }
      }
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedPictures: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        source_types: Vec<&str>,
        has_avif: bool,
        has_webp: bool,
        has_fallback_img: bool,
        img_src: &str,
        defect_kind: &str,
    ) -> PictureSourceCoverageHit {
        PictureSourceCoverageHit {
            selector: selector.into(),
            img_src: img_src.into(),
            alt: String::new(),
            source_types: source_types.iter().map(|s| (*s).into()).collect(),
            has_avif,
            has_webp,
            has_fallback_img,
            defect_kind: defect_kind.into(),
        }
    }

    fn snap(hits: Vec<PictureSourceCoverageHit>) -> PictureSourceCoverageSnapshot {
        PictureSourceCoverageSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_pictures: 10,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_picture_source_coverage(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_fallback_img_is_strict() {
        let s = snap(vec![hit(
            ".hero",
            vec!["image/avif", "image/webp"],
            true,
            true,
            false,
            "",
            "no-fallback-img",
        )]);
        let findings = detect_picture_source_coverage(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(
            findings[0].kind,
            "picture-source-coverage.no-fallback-img"
        );
        assert!(findings[0].detail.contains(".hero"));
        assert!(findings[0].detail.contains("no fallback"));
    }

    #[test]
    fn no_source_is_warn() {
        let s = snap(vec![hit(
            ".plain",
            vec![],
            false,
            false,
            true,
            "/a.jpg",
            "no-source",
        )]);
        let findings = detect_picture_source_coverage(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "picture-source-coverage.no-source");
        assert!(findings[0].detail.contains("dead chrome"));
    }

    #[test]
    fn missing_avif_is_warn() {
        let s = snap(vec![hit(
            ".webp-only",
            vec!["image/webp"],
            false,
            true,
            true,
            "/a.jpg",
            "missing-avif",
        )]);
        let findings = detect_picture_source_coverage(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "picture-source-coverage.missing-avif");
        assert!(findings[0].detail.contains("AVIF"));
    }

    #[test]
    fn missing_webp_is_warn() {
        let s = snap(vec![hit(
            ".avif-only",
            vec!["image/avif"],
            true,
            false,
            true,
            "/a.jpg",
            "missing-webp",
        )]);
        let findings = detect_picture_source_coverage(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "picture-source-coverage.missing-webp");
        assert!(findings[0].detail.contains("WebP"));
    }

    #[test]
    fn multiple_defects_emit_multiple_findings() {
        let s = snap(vec![
            hit(".a", vec![], false, false, false, "", "no-fallback-img"),
            hit(".b", vec!["image/webp"], false, true, true, "/y.jpg", "missing-avif"),
            hit(".c", vec!["image/avif"], true, false, true, "/z.jpg", "missing-webp"),
        ]);
        let findings = detect_picture_source_coverage(&s);
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"picture-source-coverage.no-fallback-img"));
        assert!(kinds.contains(&"picture-source-coverage.missing-avif"));
        assert!(kinds.contains(&"picture-source-coverage.missing-webp"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(
            ".x",
            vec!["image/webp"],
            false,
            true,
            true,
            "/y.jpg",
            "future-defect",
        )]);
        let findings = detect_picture_source_coverage(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".avif-only-{i}"),
                vec!["image/webp"],
                false,
                true,
                true,
                "/x.jpg",
                "missing-avif",
            ));
        }
        let s = snap(hits);
        let findings = detect_picture_source_coverage(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <picture> element(s)"));
        // 5 examples → 4 "; " separators between them. The body
        // message itself does NOT contain "; ", so this is a
        // clean separator count.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("hits"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("scannedPictures"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("sourceTypes"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("hasAvif"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("hasWebp"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("hasFallbackImg"));
        // All four defect-kind strings present.
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'no-fallback-img'"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'no-source'"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'missing-avif'"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'missing-webp'"));
        // MIME comparisons against image/avif + image/webp.
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'image/avif'"));
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'image/webp'"));
        // Opt-out contract.
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("data-picture-allow"));
        // Selector contract.
        assert!(PICTURE_SOURCE_COVERAGE_DOM_CAPTURE_JS.contains("'picture'"));
    }
}
