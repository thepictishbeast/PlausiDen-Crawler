//! `srcset_mismatch` — flags responsive-image `srcset` syntax
//! bugs that silently break the browser's image selection.
//!
//! Three defect classes:
//!
//! 1. **Duplicate density descriptor** — two entries claim the
//!    same `Nx` density (`foo.webp 1x, bar.webp 1x`). Only one
//!    wins; the other is dead weight in the source AND can hide
//!    a typo (operator meant `bar.webp 2x`).
//!
//! 2. **Invalid descriptor** — `srcset` entries must use either
//!    `Nx` (density) OR `Nw` (width). Forms like `foo.webp 1.5`
//!    (missing unit), `foo.webp 200dpi` (wrong unit), or
//!    `foo.webp 1500px` (wrong unit) cause the entire `srcset`
//!    to be ignored by the browser — `src` falls back. Easy
//!    to miss because the page still renders.
//!
//! 3. **Width descriptors without `sizes`** — when `srcset`
//!    uses `Nw` descriptors, `sizes` MUST be present so the
//!    browser knows the layout-time width. Without `sizes`,
//!    the browser falls back to `100vw` (`sizes="100vw"`),
//!    which usually picks a much larger image than needed —
//!    wasting bandwidth + LCP budget.
//!
//! Skip `<img>` without `srcset` (no contract to verify).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending image.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SrcsetMismatchHit {
    /// CSS-ish path of the offending `<img>`.
    pub selector: String,
    /// Fallback `src` attribute (empty if absent — `srcset` is
    /// supposed to provide alternatives, not the base).
    pub src: String,
    /// Full `srcset` attribute verbatim.
    pub srcset: String,
    /// `sizes` attribute (empty when absent).
    pub sizes: String,
    /// `alt` attribute for context (capped at 60 chars).
    pub alt: String,
    /// Defect kind — one of
    /// `"duplicate-density"`, `"invalid-descriptor"`,
    /// `"missing-sizes"`. Multiple defects on the same image
    /// emit multiple hits (one per kind).
    pub defect_kind: String,
    /// Free-text detail (which descriptor, what value).
    pub defect_detail: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SrcsetMismatchSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every offending hit.
    pub hits: Vec<SrcsetMismatchHit>,
    /// Total `<img srcset>` walked.
    pub scanned_images: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings. Splits hits by
/// `defect_kind`. Empty hits → empty findings.
#[must_use]
pub fn detect_srcset_mismatch(snap: &SrcsetMismatchSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut duplicate: Vec<&SrcsetMismatchHit> = Vec::new();
    let mut invalid: Vec<&SrcsetMismatchHit> = Vec::new();
    let mut missing_sizes: Vec<&SrcsetMismatchHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "duplicate-density" => duplicate.push(h),
            "invalid-descriptor" => invalid.push(h),
            "missing-sizes" => missing_sizes.push(h),
            _ => {} // unknown defect kinds are ignored; defensive
        }
    }

    let format_example = |h: &SrcsetMismatchHit| -> String {
        let alt = if h.alt.is_empty() {
            String::new()
        } else {
            format!(" alt=\"{}\"", h.alt)
        };
        format!(
            "{}{} (srcset=`{}`{}): {}",
            h.selector,
            alt,
            h.srcset,
            if h.sizes.is_empty() {
                String::new()
            } else {
                format!(" sizes=`{}`", h.sizes)
            },
            h.defect_detail
        )
    };

    let mut out = Vec::new();
    if !invalid.is_empty() {
        // Invalid descriptors are the worst: the entire srcset
        // is ignored by the browser and `src` falls back.
        let examples: Vec<String> = invalid
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "srcset.invalid-descriptor".to_owned(),
            detail: format!(
                "{} image(s) carry srcset entries with invalid descriptors — browser ignores the entire srcset and falls back to `src`. Use `Nx` (density) or `Nw` (width) only — never `N`, `Npx`, or `Ndpi`. Examples: {}",
                invalid.len(),
                examples.join("; ")
            ),
        });
    }
    if !duplicate.is_empty() {
        let examples: Vec<String> = duplicate
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "srcset.duplicate-density".to_owned(),
            detail: format!(
                "{} image(s) repeat the same density descriptor in srcset — only one entry wins; the other is dead weight that may hide a typo. Each descriptor should appear at most once. Examples: {}",
                duplicate.len(),
                examples.join("; ")
            ),
        });
    }
    if !missing_sizes.is_empty() {
        let examples: Vec<String> = missing_sizes
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "srcset.missing-sizes".to_owned(),
            detail: format!(
                "{} image(s) use width descriptors (`Nw`) in srcset but no `sizes` attribute — browser falls back to `sizes=100vw`, usually picking a much larger image than needed. Add a `sizes` attribute matching the layout (e.g. `sizes=\"(min-width: 768px) 50vw, 100vw\"`). Examples: {}",
                missing_sizes.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<img srcset>`
/// and emits one hit per defect kind found (an image with both
/// duplicate-density AND missing-sizes produces two hits).
///
/// Mirror any change in this file's `SrcsetMismatchHit` /
/// `SrcsetMismatchSnapshot` field set.
pub const SRCSET_MISMATCH_DOM_CAPTURE_JS: &str = r#"
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

    // Returns { url, descriptor }. descriptor is the second
    // whitespace-separated token after the URL (trailing).
    const parseEntry = function(entry) {
      const e = entry.trim();
      if (!e) return null;
      // The descriptor (if any) is the last whitespace-separated
      // token. URLs themselves don't contain spaces (must be
      // %20-encoded), so a simple split is safe.
      const parts = e.split(/\s+/);
      if (parts.length === 1) {
        return { url: parts[0], descriptor: '' };
      }
      return {
        url: parts.slice(0, parts.length - 1).join(' '),
        descriptor: parts[parts.length - 1]
      };
    };

    // Returns 'density' | 'width' | 'invalid'.
    const classifyDescriptor = function(d) {
      if (d === '') return 'density'; // default 1x
      if (/^[0-9]+(?:\.[0-9]+)?x$/.test(d)) return 'density';
      if (/^[0-9]+w$/.test(d)) return 'width';
      return 'invalid';
    };

    const hits = [];
    let scanned = 0;
    const imgs = document.querySelectorAll('img[srcset]');
    for (const img of imgs) {
      scanned += 1;
      const srcset = (img.getAttribute('srcset') || '').trim();
      const sizes = (img.getAttribute('sizes') || '').trim();
      const src = (img.getAttribute('src') || '').trim();
      const alt = (img.getAttribute('alt') || '').substring(0, 60);

      // Split on commas at the descriptor boundary. srcset
      // doesn't allow commas inside URLs without %2C-encoding,
      // so a naive split works.
      const entries = srcset.split(',').map(parseEntry).filter(function(x) { return x !== null; });
      if (entries.length === 0) continue;

      let hasWidth = false;
      let hasInvalid = false;
      const densitySeen = {};
      const seenDuplicate = [];
      const seenInvalid = [];
      for (const e of entries) {
        const kind = classifyDescriptor(e.descriptor);
        if (kind === 'width') hasWidth = true;
        if (kind === 'invalid') {
          hasInvalid = true;
          seenInvalid.push(e.descriptor || '(empty)');
        }
        if (kind === 'density') {
          const k = e.descriptor || '1x';
          if (densitySeen[k]) seenDuplicate.push(k);
          else densitySeen[k] = true;
        }
      }

      const baseHit = {
        selector: selectorOf(img),
        src: src,
        srcset: srcset,
        sizes: sizes,
        alt: alt
      };

      if (hasInvalid) {
        hits.push(Object.assign({}, baseHit, {
          defectKind: 'invalid-descriptor',
          defectDetail: 'invalid descriptors: ' + seenInvalid.join(', ')
        }));
      }
      if (seenDuplicate.length > 0) {
        hits.push(Object.assign({}, baseHit, {
          defectKind: 'duplicate-density',
          defectDetail: 'repeated descriptor(s): ' + seenDuplicate.join(', ')
        }));
      }
      if (hasWidth && sizes === '') {
        hits.push(Object.assign({}, baseHit, {
          defectKind: 'missing-sizes',
          defectDetail: 'srcset uses Nw descriptors without sizes'
        }));
      }
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedImages: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, srcset: &str, sizes: &str, defect_kind: &str, defect_detail: &str) -> SrcsetMismatchHit {
        SrcsetMismatchHit {
            selector: selector.into(),
            src: "/fallback.webp".into(),
            srcset: srcset.into(),
            sizes: sizes.into(),
            alt: String::new(),
            defect_kind: defect_kind.into(),
            defect_detail: defect_detail.into(),
        }
    }

    fn snap(hits: Vec<SrcsetMismatchHit>) -> SrcsetMismatchSnapshot {
        SrcsetMismatchSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_images: 20,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_srcset_mismatch(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn invalid_descriptor_is_strict() {
        let s = snap(vec![hit(
            ".hero img",
            "/a.webp 1.5, /b.webp 2x",
            "",
            "invalid-descriptor",
            "invalid descriptors: 1.5",
        )]);
        let findings = detect_srcset_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "srcset.invalid-descriptor");
        assert!(findings[0].detail.contains(".hero img"));
        assert!(findings[0].detail.contains("1.5"));
    }

    #[test]
    fn duplicate_density_is_strict() {
        let s = snap(vec![hit(
            ".gallery img",
            "/a.webp 1x, /b.webp 1x",
            "",
            "duplicate-density",
            "repeated descriptor(s): 1x",
        )]);
        let findings = detect_srcset_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "srcset.duplicate-density");
        assert!(findings[0].detail.contains("dead weight"));
    }

    #[test]
    fn missing_sizes_is_warn() {
        let s = snap(vec![hit(
            ".thumb img",
            "/a.webp 320w, /b.webp 640w",
            "",
            "missing-sizes",
            "srcset uses Nw descriptors without sizes",
        )]);
        let findings = detect_srcset_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "srcset.missing-sizes");
        assert!(findings[0].detail.contains("100vw"));
    }

    #[test]
    fn all_three_defects_emit_three_findings() {
        let s = snap(vec![
            hit(".a", "/x.webp 1.5", "", "invalid-descriptor", "invalid descriptors: 1.5"),
            hit(".b", "/y.webp 1x, /z.webp 1x", "", "duplicate-density", "repeated descriptor(s): 1x"),
            hit(".c", "/p.webp 320w, /q.webp 640w", "", "missing-sizes", "srcset uses Nw descriptors without sizes"),
        ]);
        let findings = detect_srcset_mismatch(&s);
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"srcset.invalid-descriptor"));
        assert!(kinds.contains(&"srcset.duplicate-density"));
        assert!(kinds.contains(&"srcset.missing-sizes"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        // The detector's wire contract is the three documented
        // kinds; an unknown kind shouldn't crash or leak into
        // findings — it's silently dropped (defensive code path).
        let s = snap(vec![hit(".x", "/a.webp 1x", "", "future-defect", "x")]);
        let findings = detect_srcset_mismatch(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".img-{i}"),
                "/x.webp 1.5",
                "",
                "invalid-descriptor",
                "invalid descriptors: 1.5",
            ));
        }
        let s = snap(hits);
        let findings = detect_srcset_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 image(s)"));
        // 5 examples → 4 "; " separators.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("hits"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("scannedImages"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("defectKind"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("defectDetail"));
        // All three defect-kind strings present.
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("'invalid-descriptor'"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("'duplicate-density'"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("'missing-sizes'"));
        // Descriptor classifier regexes — density (Nx) + width (Nw).
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("[0-9]+(?:\\.[0-9]+)?x"));
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("[0-9]+w"));
        // Selector contract.
        assert!(SRCSET_MISMATCH_DOM_CAPTURE_JS.contains("'img[srcset]'"));
    }
}
