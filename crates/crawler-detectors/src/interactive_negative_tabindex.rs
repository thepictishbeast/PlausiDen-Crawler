//! `interactive_negative_tabindex` — flags interactive
//! elements with `tabindex="-1"` that should be keyboard-
//! reachable.
//!
//! Companion to [`crate::tabindex_positive`] which flags
//! POSITIVE `tabindex` (an anti-pattern in the other
//! direction — manual override of DOM-order traversal).
//! This axis catches the opposite: `tabindex="-1"` on a
//! semantic interactive element that should stay in the
//! default tab order.
//!
//! WCAG 2.1.1 (Keyboard, Level A): all functionality must
//! be operable through the keyboard. Negative tabindex on
//! a `<button>` or `<a href>` removes it from sequential
//! keyboard navigation; users who can't use a mouse can't
//! reach the control.
//!
//! Defect class: operator writes `<button tabindex="-1"
//! onclick="…">` thinking "I'll script the focus" — usually
//! during prototyping — and forgets to re-enable keyboard
//! access. Or pastes JS from a stale "accessible carousel"
//! tutorial that intentionally removed all but one slide
//! from tab order without restoring it.
//!
//! Skip cases (out of scope):
//!
//! * `aria-hidden="true"` — operator declared the element
//!   removed from the accessibility tree entirely. The
//!   `tabindex="-1"` is consistent with that intent.
//! * `disabled` attribute on form controls — disabled
//!   controls SHOULD be skipped from tab order; some
//!   browsers do this automatically + don't need explicit
//!   `tabindex="-1"`, but pairing them isn't wrong.
//! * `inert` ancestor — operator declared the subtree
//!   non-interactive.
//! * `data-tabindex-allow="true"` opt-out for measured
//!   exceptions (e.g. focus-trap targets that programmatic
//!   focus moves into but the user shouldn't tab through).
//!
//! ## Severity
//!
//! Strict only. The defect is binary: a keyboard user
//! either can or cannot reach the control. There's no
//! "partial reach" tier.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InteractiveNegativeTabindexHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Lowercase tag name (`"button"`, `"a"`, `"input"`,
    /// `"select"`, `"textarea"`).
    pub tag: String,
    /// Visible text content (capped 60 chars). For `<input>`
    /// uses `value` or `placeholder` as fallback.
    pub text: String,
    /// Best-effort accessible name (`aria-label` falling
    /// back to text). Capped 60 chars.
    pub label: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InteractiveNegativeTabindexSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending elements (already pre-filtered to exclude
    /// aria-hidden / disabled / inert / opted-out).
    pub hits: Vec<InteractiveNegativeTabindexHit>,
    /// Total elements walked (interactive selectors only).
    pub scanned_elements: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_interactive_negative_tabindex(
    snap: &InteractiveNegativeTabindexSnapshot,
) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(MAX_EXAMPLES)
        .map(|h| {
            let label = if h.label.is_empty() {
                String::new()
            } else {
                format!(" [{}]", h.label)
            };
            format!("{} <{}>{}", h.selector, h.tag, label)
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Strict,
        kind: "interactive-negative-tabindex.unreachable".to_owned(),
        detail: format!(
            "{} interactive element(s) with `tabindex=\"-1\"` — removed from sequential keyboard navigation. Fails WCAG 2.1.1 (Keyboard, Level A). Either remove the attribute or add `aria-hidden=\"true\"` if the element is genuinely not part of the accessible UI. Opt out with `data-tabindex-allow=\"true\"` for measured exceptions (focus-trap targets, programmatic-only buttons). Examples: {}",
            snap.hits.len(),
            examples.join("; ")
        ),
    }]
}

/// Browser-side DOM-capture script. Walks `button`, `a[href]`,
/// `input` (not type=hidden), `select`, `textarea` with
/// `tabindex="-1"`. Skips aria-hidden / disabled / inert /
/// opted-out. Captures basic identifying context.
pub const INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS: &str = r#"
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

    const SELECTOR =
      'button[tabindex="-1"], a[href][tabindex="-1"], select[tabindex="-1"], textarea[tabindex="-1"], input[tabindex="-1"]:not([type="hidden"])';

    // Returns true iff `el` or any ancestor (up to body) has
    // aria-hidden="true" or the `inert` attribute set.
    const isHiddenFromAt = function(el) {
      let p = el;
      let d = 0;
      while (p && p !== document.body && d < 10) {
        if (p.getAttribute) {
          if (p.getAttribute('aria-hidden') === 'true') return true;
          if (p.hasAttribute && p.hasAttribute('inert')) return true;
        }
        p = p.parentElement;
        d += 1;
      }
      return false;
    };

    const labelOf = function(el) {
      const aria = el.getAttribute && el.getAttribute('aria-label');
      if (aria) return aria.trim().substring(0, 60);
      const t = (el.textContent || '').trim();
      if (t) return t.substring(0, 60);
      const val = el.getAttribute ? (el.getAttribute('value') || el.getAttribute('placeholder') || '') : '';
      if (val) return val.trim().substring(0, 60);
      return '';
    };

    const hits = [];
    let scanned = 0;
    const matches = document.querySelectorAll(SELECTOR);
    for (const el of matches) {
      if (el.getAttribute && el.getAttribute('data-tabindex-allow') === 'true') continue;
      // disabled controls are naturally excluded from tab order;
      // an explicit tabindex="-1" alongside is redundant but
      // not a defect.
      if (el.disabled === true) continue;
      if (isHiddenFromAt(el)) continue;
      scanned += 1;
      const tag = el.tagName.toLowerCase();
      const text = (el.textContent || '').trim().substring(0, 60);
      hits.push({
        selector: selectorOf(el),
        tag: tag,
        text: text,
        label: labelOf(el)
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

    fn hit(selector: &str, tag: &str, text: &str) -> InteractiveNegativeTabindexHit {
        InteractiveNegativeTabindexHit {
            selector: selector.into(),
            tag: tag.into(),
            text: text.into(),
            label: text.into(),
        }
    }

    fn snap(hits: Vec<InteractiveNegativeTabindexHit>) -> InteractiveNegativeTabindexSnapshot {
        let scanned = hits.len() as u32;
        InteractiveNegativeTabindexSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_elements: scanned,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_interactive_negative_tabindex(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn single_hit_is_strict() {
        let s = snap(vec![hit(".cta", "button", "Submit")]);
        let findings = detect_interactive_negative_tabindex(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(
            findings[0].kind,
            "interactive-negative-tabindex.unreachable"
        );
        assert!(findings[0].detail.contains(".cta"));
        assert!(findings[0].detail.contains("WCAG 2.1.1"));
        assert!(findings[0].detail.contains("Submit"));
    }

    #[test]
    fn multiple_hits_single_finding() {
        let s = snap(vec![
            hit(".btn", "button", "A"),
            hit(".link", "a", "B"),
            hit(".input", "input", ""),
        ]);
        let findings = detect_interactive_negative_tabindex(&s);
        // Single finding kind regardless of how many offenders.
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("3 interactive element(s)"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit(".cta", "button", "");
        h.label = "Subscribe to newsletter".into();
        let s = snap(vec![h]);
        let findings = detect_interactive_negative_tabindex(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Subscribe to newsletter]"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".btn-{i}"), "button", "X"));
        }
        let s = snap(hits);
        let findings = detect_interactive_negative_tabindex(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 interactive element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("hits"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("scannedElements"));
        // Selector contract — all 5 interactive element types.
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("button[tabindex=\"-1\"]"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("a[href][tabindex=\"-1\"]"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("input[tabindex=\"-1\"]"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("select[tabindex=\"-1\"]"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("textarea[tabindex=\"-1\"]"));
        // Skip contracts.
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("aria-hidden"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("'inert'"));
        assert!(INTERACTIVE_NEGATIVE_TABINDEX_DOM_CAPTURE_JS.contains("data-tabindex-allow"));
    }
}
