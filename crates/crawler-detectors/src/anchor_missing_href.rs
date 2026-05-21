//! `anchor_missing_href` — flags `<a>` elements without an
//! `href` attribute.
//!
//! Completes the anchor-defects triad alongside
//! [`crate::anchor_href_defects`] (empty + `javascript:`) and
//! [`crate::fragment_anchor`] (`#` + `#missing-target`). This
//! axis handles the case those two skip: the `<a>` with NO
//! `href` attribute at all.
//!
//! Why it matters: without an `href`, an `<a>` is not a link
//! per the HTML spec — it's not focusable by keyboard, not
//! announced as a link by screen readers, not navigable.
//! Operators usually arrive at this by:
//!
//! 1. **Forgetting** to fill it in during dev iteration
//!    (the most common case).
//! 2. **Misusing `<a>` as a JS-driven control** —
//!    `<a onclick="…">` with no href. Should be
//!    `<button type="button">`. ARIA-Authoring Practices is
//!    explicit on this.
//! 3. **Using `<a>` as a placeholder** while waiting for the
//!    real target (link will come).
//!
//! Defect classes:
//!
//! 1. **Bare `<a>` without href** (Strict). No href, no
//!    `role="button"`. The element is non-interactive but
//!    looks like a link to sighted users.
//!
//! 2. **`<a role="button">` without href** (Warn). Operator
//!    declared `role="button"` to recover semantics but
//!    should be using a real `<button>` element. The role
//!    fix is half-right: keyboard activation still requires
//!    Enter+Space handlers a real button gets for free.
//!
//! Skip cases:
//!
//! * `<a name="anchor-target">` — named-anchor target syntax
//!   (legacy but valid; no href is correct).
//! * `<a id="…">` used purely as a scroll target — same shape
//!   as named-anchor; some operators prefer it over `<span
//!   id="…">`.
//! * `<a data-anchor-allow="true">` opt-out.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<a>` without href.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnchorMissingHrefHit {
    /// CSS-ish path of the offending `<a>`.
    pub selector: String,
    /// Visible text content (capped 60 chars).
    pub text: String,
    /// Best-effort accessible name (`aria-label` falling back
    /// to text, capped 60 chars). Empty when none.
    pub label: String,
    /// True iff the `<a>` declares `role="button"`.
    pub has_role_button: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnchorMissingHrefSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending `<a>` elements (already pre-filtered to
    /// exclude `name=`/`id=`-only legacy anchor-target syntax).
    pub hits: Vec<AnchorMissingHrefHit>,
    /// Total `<a>` elements walked.
    pub scanned_anchors: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_anchor_missing_href(snap: &AnchorMissingHrefSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut bare: Vec<&AnchorMissingHrefHit> = Vec::new();
    let mut role_button: Vec<&AnchorMissingHrefHit> = Vec::new();
    for h in &snap.hits {
        if h.has_role_button {
            role_button.push(h);
        } else {
            bare.push(h);
        }
    }

    let format_example = |h: &AnchorMissingHrefHit| -> String {
        let label = if h.label.is_empty() {
            String::new()
        } else {
            format!(" [{}]", h.label)
        };
        format!("{}{}", h.selector, label)
    };

    let mut out = Vec::new();
    if !bare.is_empty() {
        let examples: Vec<String> = bare
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "anchor-missing-href.bare".to_owned(),
            detail: format!(
                "{} <a> element(s) without an `href` attribute. Without `href`, an <a> is not a link per the HTML spec — it's not keyboard-focusable, not announced as a link by screen readers, not navigable. Either add a real href or replace with `<button type=\"button\">` (if JS-driven). Examples: {}",
                bare.len(),
                examples.join("; ")
            ),
        });
    }
    if !role_button.is_empty() {
        let examples: Vec<String> = role_button
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "anchor-missing-href.role-button".to_owned(),
            detail: format!(
                "{} `<a role=\"button\">` element(s) without `href`. The role fix is half-right: a real `<button type=\"button\">` gets Enter+Space keyboard activation handlers for free. Replace the anchor with a button. Examples: {}",
                role_button.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<a>`
/// without `href`. Pre-filters legacy anchor-target syntax
/// (`<a name="…">` / `<a id="…">` with no other interactive
/// attrs) so the snapshot stays focused on the real defect.
pub const ANCHOR_MISSING_HREF_DOM_CAPTURE_JS: &str = r#"
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
    // `a:not([href])` — anchors without href attribute at all.
    const anchors = document.querySelectorAll('a:not([href])');
    for (const a of anchors) {
      // Opt-out for operator-intentional shapes.
      if (a.getAttribute && a.getAttribute('data-anchor-allow') === 'true') continue;
      // Legacy anchor-target syntax: <a name="…"> or
      // <a id="…"> with no text content and no role. Treat as
      // scroll-target marker; not the defect this axis flags.
      const text = (a.textContent || '').trim();
      const hasName = a.hasAttribute('name');
      const role = (a.getAttribute('role') || '').trim().toLowerCase();
      if (hasName && text === '' && role === '') continue;
      if (text === '' && a.hasAttribute('id') && role === '' && !a.querySelector('img')) continue;
      scanned += 1;

      hits.push({
        selector: selectorOf(a),
        text: text.substring(0, 60),
        label: labelOf(a),
        hasRoleButton: role === 'button'
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

    fn hit(selector: &str, text: &str, has_role_button: bool) -> AnchorMissingHrefHit {
        AnchorMissingHrefHit {
            selector: selector.into(),
            text: text.into(),
            label: text.into(),
            has_role_button,
        }
    }

    fn snap(hits: Vec<AnchorMissingHrefHit>) -> AnchorMissingHrefSnapshot {
        AnchorMissingHrefSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_anchors: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_anchor_missing_href(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn bare_anchor_is_strict() {
        let s = snap(vec![hit(".broken", "Click here", false)]);
        let findings = detect_anchor_missing_href(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "anchor-missing-href.bare");
        assert!(findings[0].detail.contains(".broken"));
        assert!(findings[0].detail.contains("not keyboard-focusable"));
    }

    #[test]
    fn role_button_anchor_is_warn() {
        let s = snap(vec![hit(".x", "Show details", true)]);
        let findings = detect_anchor_missing_href(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "anchor-missing-href.role-button");
        assert!(findings[0].detail.contains("Enter+Space"));
    }

    #[test]
    fn mixed_emits_two_findings() {
        let s = snap(vec![
            hit(".bare", "Bare", false),
            hit(".role", "Role", true),
        ]);
        let findings = detect_anchor_missing_href(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"anchor-missing-href.bare"));
        assert!(kinds.contains(&"anchor-missing-href.role-button"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit(".cta", "Click", false);
        h.label = "Continue to checkout".into();
        let s = snap(vec![h]);
        let findings = detect_anchor_missing_href(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Continue to checkout]"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".a-{i}"), "X", false));
        }
        let s = snap(hits);
        let findings = detect_anchor_missing_href(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <a> element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("hits"));
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("scannedAnchors"));
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("hasRoleButton"));
        // Selector contract — anchors without href.
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("'a:not([href])'"));
        // Legacy anchor-target skip contract.
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("hasName"));
        // Opt-out contract.
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("data-anchor-allow"));
        // Role detection.
        assert!(ANCHOR_MISSING_HREF_DOM_CAPTURE_JS.contains("'button'"));
    }
}
