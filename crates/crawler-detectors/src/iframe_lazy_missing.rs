//! `iframe_lazy_missing` — flags below-fold `<iframe>` without
//! `loading="lazy"`.
//!
//! Companion to [`crate::lazy_above_fold`] (which audits the
//! opposite direction for `<img>`). Below-fold iframes that
//! load eagerly cost serious bytes — a YouTube embed in the
//! footer can pull ~500 KB before the user has any chance of
//! seeing it. Setting `loading="lazy"` defers the load until
//! the iframe enters (or is near) the viewport.
//!
//! Defect classes:
//!
//! 1. **Below-fold iframe with `loading=` other than `lazy`**
//!    (Strict). Explicit performance hit: operator declared
//!    eager loading (or left default) but the iframe isn't
//!    visible at first paint.
//!
//! 2. **Below-fold iframe with no `loading=` attribute**
//!    (Warn). Browser default varies (Chrome lazy-by-default
//!    for cross-origin iframes; Safari + Firefox eager). Be
//!    explicit so the behaviour is consistent.
//!
//! Out of scope:
//!
//! * Above-fold iframes — eager is correct there;
//!   `lazy_above_fold` covers the symmetric mistake.
//! * `<iframe>` with `data-iframe-allow="true"` opt-out.
//! * `<iframe>` with `aria-hidden="true"` (decorative).
//!
//! ## Heuristic
//!
//! Caller emulates a typical viewport (1280 px or 390 px for
//! touch). JS captures each visible `<iframe>` with
//! `getBoundingClientRect()`. An iframe whose top edge is
//! beyond `window.innerHeight` (i.e. below the fold) but
//! whose `loading` attribute is not `"lazy"` is flagged.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<iframe>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IframeLazyMissingHit {
    /// CSS-ish path of the offending `<iframe>`.
    pub selector: String,
    /// `src` URL (capped 120 chars).
    pub src: String,
    /// `title` attribute (capped 60 chars).
    pub title: String,
    /// True iff a `loading=` attribute was present at all.
    pub has_explicit_loading: bool,
    /// `loading` attribute value verbatim (`"eager"`,
    /// `"lazy"`, `"auto"`, or empty if unset).
    pub loading_value: String,
    /// Top-edge offset from the viewport top in CSS px.
    /// Positive when below the fold.
    pub top_offset_px: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IframeLazyMissingSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Viewport height at capture time (CSS px) — fold line.
    pub viewport_height: u32,
    /// Offending iframes (already pre-filtered to below-fold).
    pub hits: Vec<IframeLazyMissingHit>,
    /// Total `<iframe>` elements walked.
    pub scanned_iframes: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_iframe_lazy_missing(snap: &IframeLazyMissingSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut explicit_non_lazy: Vec<&IframeLazyMissingHit> = Vec::new();
    let mut no_loading: Vec<&IframeLazyMissingHit> = Vec::new();
    for h in &snap.hits {
        if h.has_explicit_loading {
            // Operator declared something other than lazy.
            explicit_non_lazy.push(h);
        } else {
            // No attribute — relies on browser default.
            no_loading.push(h);
        }
    }

    let format_example = |h: &IframeLazyMissingHit| -> String {
        let title = if h.title.is_empty() {
            String::new()
        } else {
            format!(" [{}]", h.title)
        };
        let loading = if h.loading_value.is_empty() {
            "no attribute".to_owned()
        } else {
            format!("loading=`{}`", h.loading_value)
        };
        format!(
            "{}{} ({} · top {}px below fold · src=`{}`)",
            h.selector, title, loading, h.top_offset_px, h.src
        )
    };

    let mut out = Vec::new();
    if !explicit_non_lazy.is_empty() {
        let examples: Vec<String> = explicit_non_lazy
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "iframe-lazy.below-fold-non-lazy".to_owned(),
            detail: format!(
                "{} below-fold <iframe> element(s) with explicit non-lazy loading — pulls bytes before the user sees the embed. Set `loading=\"lazy\"`. Opt out per-element with `data-iframe-allow=\"true\"` for measured exceptions. Examples: {}",
                explicit_non_lazy.len(),
                examples.join("; ")
            ),
        });
    }
    if !no_loading.is_empty() {
        let examples: Vec<String> = no_loading
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "iframe-lazy.below-fold-no-attribute".to_owned(),
            detail: format!(
                "{} below-fold <iframe> element(s) with no `loading=` attribute. Browser default varies (Chrome lazy-by-default for cross-origin iframes — Safari and Firefox eager). Be explicit: `loading=\"lazy\"`. Examples: {}",
                no_loading.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<iframe>`,
/// captures only those below the fold without `loading="lazy"`.
///
/// Mirror any change in this file's `IframeLazyMissingHit` /
/// `IframeLazyMissingSnapshot` field set.
pub const IFRAME_LAZY_MISSING_DOM_CAPTURE_JS: &str = r#"
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
    const iframes = document.querySelectorAll('iframe');
    for (const f of iframes) {
      // Skip opt-out + aria-hidden decorative iframes.
      if (f.getAttribute && f.getAttribute('data-iframe-allow') === 'true') continue;
      if (f.getAttribute && f.getAttribute('aria-hidden') === 'true') continue;
      scanned += 1;

      const rect = f.getBoundingClientRect();
      // Skip zero-size iframes (display:none, hidden, etc.).
      if (rect.width <= 0 || rect.height <= 0) continue;

      const top = rect.top;
      // Above-fold iframes (top within viewport) are correctly
      // eager — out of scope.
      if (top < window.innerHeight) continue;

      const loadingAttr = f.getAttribute('loading');
      const loadingValue = loadingAttr == null ? '' : loadingAttr.trim().toLowerCase();
      // Lazy-loaded already → out of scope.
      if (loadingValue === 'lazy') continue;

      hits.push({
        selector: selectorOf(f),
        src: (f.getAttribute('src') || '').trim().substring(0, 120),
        title: (f.getAttribute('title') || '').trim().substring(0, 60),
        hasExplicitLoading: loadingAttr != null,
        loadingValue: loadingValue,
        topOffsetPx: Math.round(top - window.innerHeight)
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      hits: hits,
      scannedIframes: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        has_explicit_loading: bool,
        loading_value: &str,
        top_offset_px: u32,
    ) -> IframeLazyMissingHit {
        IframeLazyMissingHit {
            selector: selector.into(),
            src: "/embed.html".into(),
            title: String::new(),
            has_explicit_loading,
            loading_value: loading_value.into(),
            top_offset_px,
        }
    }

    fn snap(hits: Vec<IframeLazyMissingHit>) -> IframeLazyMissingSnapshot {
        IframeLazyMissingSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            viewport_height: 800,
            hits,
            scanned_iframes: 4,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_iframe_lazy_missing(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn explicit_eager_below_fold_is_strict() {
        let s = snap(vec![hit(".yt", true, "eager", 1200)]);
        let findings = detect_iframe_lazy_missing(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "iframe-lazy.below-fold-non-lazy");
        assert!(findings[0].detail.contains(".yt"));
        assert!(findings[0].detail.contains("loading=`eager`"));
        assert!(findings[0].detail.contains("1200px below fold"));
    }

    #[test]
    fn explicit_auto_below_fold_is_strict() {
        // `loading="auto"` is browser-default-equivalent and
        // still treated as not-lazy.
        let s = snap(vec![hit(".embed", true, "auto", 500)]);
        let findings = detect_iframe_lazy_missing(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn no_loading_attribute_below_fold_is_warn() {
        let s = snap(vec![hit(".feed", false, "", 800)]);
        let findings = detect_iframe_lazy_missing(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(
            findings[0].kind,
            "iframe-lazy.below-fold-no-attribute"
        );
        assert!(findings[0].detail.contains("no attribute"));
        assert!(findings[0].detail.contains("Browser default varies"));
    }

    #[test]
    fn mixed_emits_two_findings() {
        let s = snap(vec![
            hit(".a", true, "eager", 1200),
            hit(".b", false, "", 800),
        ]);
        let findings = detect_iframe_lazy_missing(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"iframe-lazy.below-fold-non-lazy"));
        assert!(kinds.contains(&"iframe-lazy.below-fold-no-attribute"));
    }

    #[test]
    fn title_appears_in_examples_when_present() {
        let mut h = hit(".yt", true, "eager", 1200);
        h.title = "YouTube tutorial".into();
        let s = snap(vec![h]);
        let findings = detect_iframe_lazy_missing(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[YouTube tutorial]"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".f-{i}"), true, "eager", 500));
        }
        let s = snap(hits);
        let findings = detect_iframe_lazy_missing(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 below-fold <iframe> element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("viewportHeight"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("hits"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("scannedIframes"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("hasExplicitLoading"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("loadingValue"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("topOffsetPx"));
        // Selector contract.
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("'iframe'"));
        // Opt-out + aria-hidden contracts.
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("data-iframe-allow"));
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("aria-hidden"));
        // Above-fold skip contract.
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("window.innerHeight"));
        // Lazy-already skip contract.
        assert!(IFRAME_LAZY_MISSING_DOM_CAPTURE_JS.contains("'lazy'"));
    }
}
