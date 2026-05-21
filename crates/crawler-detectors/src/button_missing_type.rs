//! `button_missing_type` — flags `<button>` elements without
//! an explicit `type=` attribute.
//!
//! Defect class: HTML5 says a bare `<button>` inside a `<form>`
//! defaults to `type="submit"`. So a `<button>` written for
//! "Show password" / "Toggle dark mode" / "Add tag" inside a
//! larger form WILL SUBMIT THE FORM when clicked — even though
//! the operator clearly intended a benign in-form helper button.
//! Easy to miss in code review; the form might POST to a stale
//! URL or, worse, double-submit credentials.
//!
//! Outside a `<form>` the implicit type is still `submit`, but
//! since there's no form to submit it's a no-op. Still flagged
//! at Warn because:
//!
//! 1. Refactor moving the button into a form silently changes
//!    its behaviour.
//! 2. Browsers handle bare buttons inconsistently in some
//!    edge cases (custom-elements adoption, dialog form
//!    submission).
//! 3. The intent should be explicit.
//!
//! Skip:
//! * `<button type="…">` — operator declared intent.
//! * `<input type="…">` — has a type by construction.
//! * `<button data-button-type-allow="true">` opt-out for
//!   measured edge cases.
//!
//! ## Severity
//!
//! * **Strict** — `<button>` without `type=` AND inside a
//!   `<form>` AND the button is NOT the form's intended
//!   submit (heuristic: there's already another `type="submit"`
//!   button OR the implicit-submit button doesn't look like a
//!   submit by its text — "Cancel", "Reset", "Show", "Toggle",
//!   etc.).
//! * **Warn** — `<button>` without `type=` OUTSIDE a form, OR
//!   inside a form when no clearly-intended submit indicator
//!   could be established.
//!
//! The detector keeps the Strict-tier heuristic conservative —
//! it only fires when there's positive evidence that the
//! implicit submit is unintended. Operators who genuinely want
//! a bare submit button can ignore the Warn-tier finding.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<button>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ButtonMissingTypeHit {
    /// CSS-ish path of the offending `<button>`.
    pub selector: String,
    /// Visible text content (capped 60 chars).
    pub text: String,
    /// Best-effort accessible name (`aria-label` fallback to
    /// text, capped 60 chars). Empty when none.
    pub label: String,
    /// True iff the button is inside a `<form>` ancestor.
    pub in_form: bool,
    /// True iff the JS observed evidence that the implicit
    /// submit is NOT the operator's intent (another typed
    /// submit button is present in the same form OR the
    /// button's text matches a known non-submit verb).
    pub implicit_submit_unintended: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ButtonMissingTypeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending `<button>` elements.
    pub hits: Vec<ButtonMissingTypeHit>,
    /// Total `<button>` elements walked (with or without type).
    pub scanned_buttons: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_button_missing_type(snap: &ButtonMissingTypeSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut strict: Vec<&ButtonMissingTypeHit> = Vec::new();
    let mut warn: Vec<&ButtonMissingTypeHit> = Vec::new();
    for h in &snap.hits {
        if h.in_form && h.implicit_submit_unintended {
            strict.push(h);
        } else {
            warn.push(h);
        }
    }

    let format_example = |h: &ButtonMissingTypeHit| -> String {
        let label = if h.label.is_empty() {
            String::new()
        } else {
            format!(" [{}]", h.label)
        };
        let in_form = if h.in_form { " · in <form>" } else { " · outside <form>" };
        format!("{}{}{}", h.selector, label, in_form)
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
            kind: "button-missing-type.implicit-submit".to_owned(),
            detail: format!(
                "{} <button> element(s) without explicit `type=` inside a <form> appear to submit the form unintentionally. Add `type=\"button\"` so the button doesn't trigger form submission on click. Examples: {}",
                strict.len(),
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
            kind: "button-missing-type.ambiguous".to_owned(),
            detail: format!(
                "{} <button> element(s) without explicit `type=`. Default is `submit`; specify `type=\"button\"` / `type=\"submit\"` / `type=\"reset\"` so the intent survives refactors. Examples: {}",
                warn.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<button>`
/// without an explicit `type=` attribute. Heuristic for
/// "implicit submit unintended":
///
/// 1. Another `<button type="submit">` exists in the same form
///    (the operator clearly intended THAT button as submit).
/// 2. OR the button text starts with a known non-submit verb
///    (case-insensitive prefix): cancel, reset, close, dismiss,
///    show, hide, toggle, copy, share, remove, delete, edit,
///    expand, collapse, add, undo.
///
/// Skip `data-button-type-allow="true"` opt-out.
pub const BUTTON_MISSING_TYPE_DOM_CAPTURE_JS: &str = r#"
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

    const NON_SUBMIT_VERBS = [
      'cancel', 'reset', 'close', 'dismiss', 'show', 'hide',
      'toggle', 'copy', 'share', 'remove', 'delete', 'edit',
      'expand', 'collapse', 'add', 'undo'
    ];

    const closestForm = function(el) {
      let p = el.parentElement;
      while (p) {
        if (p.tagName === 'FORM') return p;
        p = p.parentElement;
      }
      return null;
    };

    const hits = [];
    let scanned = 0;
    const buttons = document.querySelectorAll('button:not([type])');
    for (const btn of buttons) {
      if (btn.getAttribute && btn.getAttribute('data-button-type-allow') === 'true') continue;
      scanned += 1;
      const form = closestForm(btn);
      const inForm = form != null;
      const text = (btn.textContent || '').trim();
      const textLower = text.toLowerCase();
      let unintended = false;
      if (inForm) {
        // Another typed submit in the same form?
        const sibling = form.querySelector('button[type="submit"], input[type="submit"]');
        if (sibling) {
          unintended = true;
        } else {
          // Verb heuristic — split on whitespace and inspect
          // the leading token.
          const firstWord = textLower.split(/\s+/)[0] || '';
          if (NON_SUBMIT_VERBS.indexOf(firstWord) !== -1) {
            unintended = true;
          }
        }
      }
      const aria = btn.getAttribute('aria-label') || '';
      const label = (aria.trim() || text).substring(0, 60);
      hits.push({
        selector: selectorOf(btn),
        text: text.substring(0, 60),
        label: label,
        inForm: inForm,
        implicitSubmitUnintended: unintended
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedButtons: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        text: &str,
        in_form: bool,
        implicit_submit_unintended: bool,
    ) -> ButtonMissingTypeHit {
        ButtonMissingTypeHit {
            selector: selector.into(),
            text: text.into(),
            label: text.into(),
            in_form,
            implicit_submit_unintended,
        }
    }

    fn snap(hits: Vec<ButtonMissingTypeHit>) -> ButtonMissingTypeSnapshot {
        ButtonMissingTypeSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_buttons: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_button_missing_type(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn in_form_with_unintended_implicit_submit_is_strict() {
        let s = snap(vec![hit(".cancel", "Cancel", true, true)]);
        let findings = detect_button_missing_type(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(
            findings[0].kind,
            "button-missing-type.implicit-submit"
        );
        assert!(findings[0].detail.contains(".cancel"));
        assert!(findings[0].detail.contains("submit the form unintentionally"));
    }

    #[test]
    fn outside_form_is_warn() {
        let s = snap(vec![hit(".toggle", "Toggle theme", false, false)]);
        let findings = detect_button_missing_type(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "button-missing-type.ambiguous");
    }

    #[test]
    fn in_form_without_unintended_indicator_is_warn() {
        // No other typed submit + verb doesn't match the
        // non-submit list — operator may genuinely want this as
        // the submit button. Conservative: Warn only.
        let s = snap(vec![hit(".save", "Save", true, false)]);
        let findings = detect_button_missing_type(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn mixed_emits_two_findings() {
        let s = snap(vec![
            hit(".cancel", "Cancel", true, true),
            hit(".toggle", "Toggle theme", false, false),
        ]);
        let findings = detect_button_missing_type(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"button-missing-type.implicit-submit"));
        assert!(kinds.contains(&"button-missing-type.ambiguous"));
    }

    #[test]
    fn label_and_form_context_appear_in_examples() {
        let s = snap(vec![hit(".x", "Add", true, true)]);
        let findings = detect_button_missing_type(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Add]"));
        assert!(findings[0].detail.contains("in <form>"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".btn-{i}"), "Cancel", true, true));
        }
        let s = snap(hits);
        let findings = detect_button_missing_type(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <button> element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("hits"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("scannedButtons"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("inForm"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("implicitSubmitUnintended"));
        // Selector contract — only `button:not([type])`.
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("'button:not([type])'"));
        // Opt-out contract.
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("data-button-type-allow"));
        // Non-submit verb list — sample of canonical entries.
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("'cancel'"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("'toggle'"));
        assert!(BUTTON_MISSING_TYPE_DOM_CAPTURE_JS.contains("'close'"));
    }
}
