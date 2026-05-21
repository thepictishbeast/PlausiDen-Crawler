//! `anchor_href_defects` — flags two common `<a>` href bugs
//! that the existing axes don't cover:
//!
//! 1. **Empty href** (Strict). `<a href="">` — the empty string
//!    resolves to the current page URL, so clicking the anchor
//!    reloads the page. Always a bug; usually a leftover from
//!    "I'll fill this in later" dev iteration.
//!
//! 2. **JavaScript href** (Strict). `<a href="javascript:…">` —
//!    semantic misuse: `<a>` is for navigation; JS-driven
//!    interaction should use `<button type="button">`. Also a
//!    safe-link gate concern: any interpolation into the href
//!    is a direct XSS vector.
//!
//! Complements [`crate::fragment_anchor`] which already
//! catches `<a href="#">` (lone-hash) and `<a href="#x">` with
//! missing target. This axis stays focused on the two empty /
//! pseudo-scheme cases fragment_anchor doesn't see.
//!
//! Honors `data-anchor-allow="true"` opt-out on the `<a>` for
//! legitimate edge cases (e.g. a stateful component that
//! genuinely needs `<a href="javascript:…">` for back-compat
//! with a vendor analytics shim).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending anchor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnchorHrefDefectHit {
    /// CSS-ish path of the offending `<a>`.
    pub selector: String,
    /// Visible text content (capped 60 chars).
    pub text: String,
    /// `href` attribute verbatim (capped 120 chars).
    pub href: String,
    /// Best-effort accessible name — `aria-label`, falling back
    /// to text or the `<img alt>` of a child image. Capped at
    /// 60 chars. Empty when none resolved.
    pub label: String,
    /// Defect kind — one of `"empty-href"`, `"javascript-href"`.
    /// Unknown kinds are dropped defensively in the detector.
    pub defect_kind: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnchorHrefDefectSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every defective anchor.
    pub hits: Vec<AnchorHrefDefectHit>,
    /// Total `<a>` elements walked (with `href` attribute).
    pub scanned_anchors: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_anchor_href_defects(snap: &AnchorHrefDefectSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut empty: Vec<&AnchorHrefDefectHit> = Vec::new();
    let mut javascript: Vec<&AnchorHrefDefectHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "empty-href" => empty.push(h),
            "javascript-href" => javascript.push(h),
            _ => {} // defensive: unknown defect kinds dropped
        }
    }

    let format_example = |h: &AnchorHrefDefectHit| -> String {
        let label = if h.label.is_empty() {
            String::new()
        } else {
            format!(" [{}]", h.label)
        };
        format!("{}{} href=`{}`", h.selector, label, h.href)
    };

    let mut out = Vec::new();
    if !empty.is_empty() {
        let examples: Vec<String> = empty
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "anchor-href.empty".to_owned(),
            detail: format!(
                "{} <a href=\"\"> element(s) — empty href resolves to current page URL, so clicking reloads the page. Replace with `<button type=\"button\">` if JS-driven, or fill in the real target. Examples: {}",
                empty.len(),
                examples.join("; ")
            ),
        });
    }
    if !javascript.is_empty() {
        let examples: Vec<String> = javascript
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "anchor-href.javascript-scheme".to_owned(),
            detail: format!(
                "{} <a href=\"javascript:…\"> element(s) — semantic misuse + XSS risk. Use `<button type=\"button\">` for JS-driven interaction; reserve <a> for navigation. Examples: {}",
                javascript.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<a href>` and
/// emits a hit for the two documented defect kinds.
///
/// Mirror any change in this file's `AnchorHrefDefectHit` /
/// `AnchorHrefDefectSnapshot` field set.
pub const ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS: &str = r#"
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

    const labelOf = function(a) {
      const aria = a.getAttribute && a.getAttribute('aria-label');
      if (aria) return aria.trim().substring(0, 60);
      const t = (a.textContent || '').trim();
      if (t) return t.substring(0, 60);
      const img = a.querySelector ? a.querySelector('img[alt]') : null;
      if (img) return (img.getAttribute('alt') || '').trim().substring(0, 60);
      return '';
    };

    const hits = [];
    let scanned = 0;
    const anchors = document.querySelectorAll('a[href]');
    for (const a of anchors) {
      // Opt-out for operator-intentional weird hrefs.
      if (a.getAttribute && a.getAttribute('data-anchor-allow') === 'true') continue;
      scanned += 1;
      const hrefAttr = (a.getAttribute('href') || '').trim();

      // `#` (lone hash) + `#anchor-id` cases belong to
      // fragment_anchor; this detector stays focused on the
      // two cases that detector skips.
      let defectKind = null;
      if (hrefAttr === '') {
        defectKind = 'empty-href';
      } else if (/^javascript:/i.test(hrefAttr)) {
        defectKind = 'javascript-href';
      }
      if (defectKind === null) continue;

      const text = (a.textContent || '').trim().substring(0, 60);
      hits.push({
        selector: selectorOf(a),
        text: text,
        href: hrefAttr.substring(0, 120),
        label: labelOf(a),
        defectKind: defectKind
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedAnchors: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, href: &str, defect_kind: &str) -> AnchorHrefDefectHit {
        AnchorHrefDefectHit {
            selector: selector.into(),
            text: String::new(),
            href: href.into(),
            label: String::new(),
            defect_kind: defect_kind.into(),
        }
    }

    fn snap(hits: Vec<AnchorHrefDefectHit>) -> AnchorHrefDefectSnapshot {
        AnchorHrefDefectSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_anchors: 50,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_anchor_href_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn empty_href_is_strict() {
        let s = snap(vec![hit(".cta", "", "empty-href")]);
        let findings = detect_anchor_href_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "anchor-href.empty");
        assert!(findings[0].detail.contains(".cta"));
        assert!(findings[0].detail.contains("reloads the page"));
    }

    #[test]
    fn javascript_href_is_strict() {
        let s = snap(vec![hit(".btn", "javascript:doStuff()", "javascript-href")]);
        let findings = detect_anchor_href_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "anchor-href.javascript-scheme");
        assert!(findings[0].detail.contains(".btn"));
        assert!(findings[0].detail.contains("XSS risk"));
    }

    #[test]
    fn both_defects_emit_two_findings() {
        let s = snap(vec![
            hit(".a", "", "empty-href"),
            hit(".b", "javascript:x()", "javascript-href"),
        ]);
        let findings = detect_anchor_href_defects(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"anchor-href.empty"));
        assert!(kinds.contains(&"anchor-href.javascript-scheme"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit(".cta", "", "empty-href");
        h.label = "Join now".into();
        let s = snap(vec![h]);
        let findings = detect_anchor_href_defects(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Join now]"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(".x", "?", "future-defect")]);
        let findings = detect_anchor_href_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".a-{i}"), "", "empty-href"));
        }
        let s = snap(hits);
        let findings = detect_anchor_href_defects(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <a href=\"\"> element(s)"));
        // 5 examples joined by "; " → 4 "; " separators.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + skip-list contract.
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("hits"));
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("scannedAnchors"));
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("defectKind"));
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("'empty-href'"));
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("'javascript-href'"));
        // javascript: scheme detection regex.
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("/^javascript:/i"));
        // Opt-out contract.
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("data-anchor-allow"));
        // Selector contract — a[href] (skips bare <a name=...>).
        assert!(ANCHOR_HREF_DEFECTS_DOM_CAPTURE_JS.contains("'a[href]'"));
    }
}
