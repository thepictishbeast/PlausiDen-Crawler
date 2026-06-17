//! `deprecated_motion_element` — flags the two HTML4-era
//! motion elements browsers still tolerate but the spec has
//! formally deprecated:
//!
//! 1. `<marquee>` — scrolling text. Deprecated since HTML5.
//!    Still rendered by all major browsers, but: (a) ignores
//!    `prefers-reduced-motion`, (b) creates a vestibular
//!    trigger for users with motion sensitivity, (c) confuses
//!    screen readers (announcement timing depends on scroll
//!    cycle), (d) signals "this site hasn't been touched
//!    since 2003." Real defect class on sites migrating from
//!    legacy CMSes.
//!
//! 2. `<blink>` — text that flashes. Deprecated since HTML5,
//!    removed from Firefox in 2013 and Chrome in 2015. Still
//!    appears in copy-pasted HTML4 snippets + some CMS
//!    legacy. WCAG 2.3.1 photosensitive violation candidate
//!    even though most browsers no longer render it.
//!
//! Both classes are Strict because they're documented spec
//! deprecations the operator should clean up regardless of
//! how the browser handles them today.
//!
//! Honors `data-motion-element-allow="true"` opt-out for the
//! rare retro-nostalgia site that wants the effect
//! intentionally.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DeprecatedMotionElementHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Lowercase tag name (`"marquee"` or `"blink"`).
    pub tag: String,
    /// Visible text content (capped 60 chars).
    pub text: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DeprecatedMotionElementSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every offending `<marquee>` / `<blink>` occurrence.
    pub hits: Vec<DeprecatedMotionElementHit>,
    /// Total elements walked. Always ≥ hits.len().
    pub scanned_elements: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_deprecated_motion_elements(
    snap: &DeprecatedMotionElementSnapshot,
) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut marquee: Vec<&DeprecatedMotionElementHit> = Vec::new();
    let mut blink: Vec<&DeprecatedMotionElementHit> = Vec::new();
    for h in &snap.hits {
        match h.tag.as_str() {
            "marquee" => marquee.push(h),
            "blink" => blink.push(h),
            _ => {} // defensive: unknown tags ignored
        }
    }

    let format_example = |h: &DeprecatedMotionElementHit| -> String {
        if h.text.is_empty() {
            h.selector.clone()
        } else {
            format!("{} (\"{}\")", h.selector, h.text)
        }
    };

    let mut out = Vec::new();
    if !marquee.is_empty() {
        let examples: Vec<String> = marquee
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "deprecated-motion-element.marquee".to_owned(),
            detail: format!(
                "{} <marquee> element(s) — deprecated since HTML5. Ignores prefers-reduced-motion, creates a vestibular trigger, confuses screen-reader announcement timing. Replace with a CSS animation that honors `@media (prefers-reduced-motion: reduce)`, or remove. Examples: {}",
                marquee.len(),
                examples.join("; ")
            ),
        });
    }
    if !blink.is_empty() {
        let examples: Vec<String> = blink
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "deprecated-motion-element.blink".to_owned(),
            detail: format!(
                "{} <blink> element(s) — deprecated since HTML5, removed from Firefox (2013) + Chrome (2015). Most browsers ignore the element today but the legacy markup signals untouched HTML4 content. Remove. Examples: {}",
                blink.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks for `<marquee>` +
/// `<blink>` elements, captures location + first 60 chars of
/// text content. Skip elements opted-out via
/// `data-motion-element-allow="true"`.
pub const DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS: &str = r#"
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
    const matches = document.querySelectorAll('marquee, blink');
    for (const el of matches) {
      if (el.getAttribute && el.getAttribute('data-motion-element-allow') === 'true') continue;
      scanned += 1;
      hits.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        text: (el.textContent || '').trim().substring(0, 60)
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedElements: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, tag: &str, text: &str) -> DeprecatedMotionElementHit {
        DeprecatedMotionElementHit {
            selector: selector.into(),
            tag: tag.into(),
            text: text.into(),
        }
    }

    fn snap(hits: Vec<DeprecatedMotionElementHit>) -> DeprecatedMotionElementSnapshot {
        let scanned = hits.len() as u32;
        DeprecatedMotionElementSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_elements: scanned,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_deprecated_motion_elements(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn marquee_is_strict() {
        let s = snap(vec![hit(".banner marquee", "marquee", "Welcome to our site!")]);
        let findings = detect_deprecated_motion_elements(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "deprecated-motion-element.marquee");
        assert!(findings[0].detail.contains("vestibular trigger"));
        assert!(findings[0].detail.contains("Welcome to our site!"));
    }

    #[test]
    fn blink_is_strict() {
        let s = snap(vec![hit(".header blink", "blink", "FLASH SALE")]);
        let findings = detect_deprecated_motion_elements(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "deprecated-motion-element.blink");
        assert!(findings[0].detail.contains("HTML5"));
        assert!(findings[0].detail.contains("FLASH SALE"));
    }

    #[test]
    fn both_emit_two_findings() {
        let s = snap(vec![
            hit(".m", "marquee", "scroll"),
            hit(".b", "blink", "flash"),
        ]);
        let findings = detect_deprecated_motion_elements(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"deprecated-motion-element.marquee"));
        assert!(kinds.contains(&"deprecated-motion-element.blink"));
    }

    #[test]
    fn empty_text_shows_selector_only() {
        let s = snap(vec![hit(".empty", "marquee", "")]);
        let findings = detect_deprecated_motion_elements(&s);
        assert_eq!(findings.len(), 1);
        // No quoted-text section when text is empty.
        assert!(!findings[0].detail.contains("(\"\")"));
        assert!(findings[0].detail.contains(".empty"));
    }

    #[test]
    fn unknown_tag_ignored_defensively() {
        let s = snap(vec![hit(".x", "future-deprecated", "X")]);
        let findings = detect_deprecated_motion_elements(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".m-{i}"), "marquee", "Text"));
        }
        let s = snap(hits);
        let findings = detect_deprecated_motion_elements(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <marquee> element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS.contains("hits"));
        assert!(DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS.contains("scannedElements"));
        // Selector contract — both deprecated elements.
        assert!(DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS.contains("'marquee, blink'"));
        // Opt-out contract.
        assert!(DEPRECATED_MOTION_ELEMENT_DOM_CAPTURE_JS.contains("data-motion-element-allow"));
    }
}
