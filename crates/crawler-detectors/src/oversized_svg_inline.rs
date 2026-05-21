//! `oversized_svg_inline` — flags inline `<svg>` elements whose
//! source weight is large enough to be costing first-paint /
//! LCP without offering the cache + compression benefits an
//! external `.svg` file would.
//!
//! Defect class: a designer drops a Figma-exported SVG straight
//! into the markup. The SVG is 50-500 KB (massive path data,
//! embedded data-URI bitmaps, or a `<image href="…">` wrapping
//! a raster). All of that ships in the HTML payload — no cache
//! reuse across pages, no separate compression budget, parser
//! has to read the entire thing before paint.
//!
//! Typical thresholds for real production sites:
//!
//! * < 10 KB inline — fine; SVG that small genuinely saves a
//!   request and ships before the round-trip.
//! * 10-50 KB inline — questionable; warn so the operator can
//!   audit and decide.
//! * > 50 KB inline — almost always wrong; strict.
//!
//! Additional signals that almost always indicate the SVG
//! should have been an `<img>` instead:
//!
//! * `<image href="…">` / `<image xlink:href="…">` child — the
//!   SVG is wrapping a raster. Use `<img>` directly.
//! * Embedded data-URI image (`href="data:image/…"` /
//!   `xlink:href="data:image/…"`) — same shape, slightly worse
//!   because the raster is also base64-bloated.
//!
//! Honors `data-svg-allow="true"` opt-out for legitimate large
//! SVGs (interactive diagrams, hero illustrations the operator
//! has measured and chosen to inline).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending inline `<svg>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OversizedSvgHit {
    /// CSS-ish path of the offending `<svg>`.
    pub selector: String,
    /// `outerHTML` byte length at capture time.
    pub size_bytes: u32,
    /// True iff the SVG contains a child `<image href="data:…">`
    /// or `<image xlink:href="data:…">`.
    pub has_data_uri: bool,
    /// True iff the SVG contains any `<image>` child (raster
    /// wrapper signal, independent of data-URI).
    pub has_embedded_image: bool,
    /// Number of `<path>` children — high counts (>50) on a
    /// single SVG suggest a Figma export that didn't simplify.
    pub path_count: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OversizedSvgSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every offending inline `<svg>`.
    pub hits: Vec<OversizedSvgHit>,
    /// Total inline `<svg>` elements walked.
    pub scanned_svgs: u32,
}

/// Size threshold for Strict.
pub const STRICT_BYTES: u32 = 50 * 1024;

/// Size threshold for Warn.
pub const WARN_BYTES: u32 = 10 * 1024;

/// Path-count above which a Warn finding mentions the count
/// in the example detail.
pub const PATH_COUNT_NOISE_FLOOR: u32 = 50;

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_oversized_svg(snap: &OversizedSvgSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut strict: Vec<&OversizedSvgHit> = Vec::new();
    let mut warn: Vec<&OversizedSvgHit> = Vec::new();
    for h in &snap.hits {
        if h.size_bytes >= STRICT_BYTES {
            strict.push(h);
        } else if h.size_bytes >= WARN_BYTES {
            warn.push(h);
        }
    }

    let format_example = |h: &OversizedSvgHit| -> String {
        let kb = (h.size_bytes as f64) / 1024.0;
        let raster = if h.has_data_uri {
            " · embeds data-URI raster"
        } else if h.has_embedded_image {
            " · contains <image> child (raster wrapper)"
        } else {
            ""
        };
        let paths = if h.path_count > PATH_COUNT_NOISE_FLOOR {
            format!(" · {} <path> children", h.path_count)
        } else {
            String::new()
        };
        format!("{} ({:.1} KB{}{})", h.selector, kb, raster, paths)
    };

    let mut out = Vec::new();
    if !strict.is_empty() {
        let examples: Vec<String> = strict
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "oversized-svg.strict".to_owned(),
            detail: format!(
                "{} inline <svg> element(s) over {} KB — ships in the HTML payload with no cache reuse across pages. Externalize to `<img src=\"foo.svg\">` or `<picture>` so the asset can compress + cache separately. Opt out with `data-svg-allow=\"true\"` for measured-and-accepted inlining. Examples: {}",
                strict.len(),
                STRICT_BYTES / 1024,
                examples.join("; ")
            ),
        });
    }
    if !warn.is_empty() {
        let examples: Vec<String> = warn
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "oversized-svg.warn".to_owned(),
            detail: format!(
                "{} inline <svg> element(s) between {} KB and {} KB — measure whether external `<img>` would be cheaper given the cache + compression trade-off. Examples: {}",
                warn.len(),
                WARN_BYTES / 1024,
                STRICT_BYTES / 1024,
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every inline `<svg>`,
/// measures `outerHTML` byte length, captures embedded-raster
/// signals.
///
/// Mirror any change in this file's `OversizedSvgHit` /
/// `OversizedSvgSnapshot` field set.
pub const OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS: &str = r#"
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

    const WARN_BYTES = 10 * 1024;

    const hits = [];
    let scanned = 0;
    const svgs = document.querySelectorAll('svg');
    for (const svg of svgs) {
      // Opt-out: operator measured + accepted the inline cost.
      if (svg.getAttribute && svg.getAttribute('data-svg-allow') === 'true') continue;
      scanned += 1;
      const html = svg.outerHTML || '';
      const size = html.length;
      // Skip small SVGs — most inline SVGs are icons + that's
      // the correct usage. Only collect ones at/above the warn
      // threshold so the snapshot stays small.
      if (size < WARN_BYTES) continue;
      // Embedded-raster signals.
      const images = svg.querySelectorAll('image');
      const hasEmbeddedImage = images.length > 0;
      let hasDataUri = false;
      for (const img of images) {
        const href = img.getAttribute('href') || img.getAttributeNS('http://www.w3.org/1999/xlink', 'href') || '';
        if (href.startsWith('data:')) {
          hasDataUri = true;
          break;
        }
      }
      const pathCount = svg.querySelectorAll('path').length;
      hits.push({
        selector: selectorOf(svg),
        sizeBytes: size,
        hasDataUri: hasDataUri,
        hasEmbeddedImage: hasEmbeddedImage,
        pathCount: pathCount
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedSvgs: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, size_bytes: u32, has_data_uri: bool, has_embedded_image: bool, path_count: u32) -> OversizedSvgHit {
        OversizedSvgHit {
            selector: selector.into(),
            size_bytes,
            has_data_uri,
            has_embedded_image,
            path_count,
        }
    }

    fn snap(hits: Vec<OversizedSvgHit>) -> OversizedSvgSnapshot {
        OversizedSvgSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_svgs: 30,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_oversized_svg(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn over_fifty_kb_is_strict() {
        let s = snap(vec![hit(".hero svg", 60_000, false, false, 12)]);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "oversized-svg.strict");
        assert!(findings[0].detail.contains(".hero svg"));
        // kb formatted with one decimal.
        assert!(findings[0].detail.contains("58.6 KB"));
    }

    #[test]
    fn between_ten_and_fifty_kb_is_warn() {
        let s = snap(vec![hit(".chart", 20_000, false, false, 30)]);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "oversized-svg.warn");
    }

    #[test]
    fn below_ten_kb_skipped() {
        let s = snap(vec![hit(".icon", 5_000, false, false, 1)]);
        let findings = detect_oversized_svg(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn data_uri_raster_appears_in_example_detail() {
        let s = snap(vec![hit(".photo", 80_000, true, true, 1)]);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("embeds data-URI raster"));
    }

    #[test]
    fn embedded_image_without_data_uri_appears_in_example_detail() {
        let s = snap(vec![hit(".wrapper", 80_000, false, true, 0)]);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("contains <image> child"));
        // Should NOT mention data-URI.
        assert!(!findings[0].detail.contains("data-URI"));
    }

    #[test]
    fn high_path_count_appears_in_example_detail() {
        let s = snap(vec![hit(".figma", 70_000, false, false, 200)]);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("200 <path> children"));
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(vec![
            hit(".huge", 80_000, false, false, 12),
            hit(".medium", 20_000, false, false, 12),
        ]);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"oversized-svg.strict"));
        assert!(kinds.contains(&"oversized-svg.warn"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".svg-{i}"), 70_000, false, false, 12));
        }
        let s = snap(hits);
        let findings = detect_oversized_svg(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 inline <svg> element(s)"));
        // 5 examples → 4 "; " separators between them.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape.
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("hits"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("scannedSvgs"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("sizeBytes"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("hasDataUri"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("hasEmbeddedImage"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("pathCount"));
        // Threshold constant present.
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("WARN_BYTES = 10 * 1024"));
        // Selector + xlink href handling contract.
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("'svg'"));
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("xlink"));
        // Opt-out contract.
        assert!(OVERSIZED_SVG_INLINE_DOM_CAPTURE_JS.contains("data-svg-allow"));
    }
}
