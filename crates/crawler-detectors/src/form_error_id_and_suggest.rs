//! `form_error_id_and_suggest` — flag form inputs marked invalid
//! that don't carry a programmatic error message + suggestion.
//!
//! WCAG 2.1 SC 3.3.1 (Error Identification, Level A) + SC 3.3.3
//! (Error Suggestion, Level AA). When a form input fails
//! validation, the page must:
//!
//! 1. Identify the error in text the user can perceive.
//! 2. Reference it via `aria-invalid="true"` +
//!    `aria-describedby` pointing at the message element.
//! 3. Suggest a corrective action (3.3.3 — Level AA).
//!
//! ## Heuristic
//!
//! Snapshot walks every `<input>`, `<select>`, `<textarea>` with
//! `aria-invalid="true"` (or the legacy `:invalid` validity
//! pseudo) and captures:
//!
//! * Whether `aria-describedby` is present.
//! * Whether the referenced element actually exists in DOM.
//! * Whether the message contains corrective-suggestion language
//!   (heuristic — looks for words like "must", "should", "use",
//!   "enter", "try", "example", "format:", colon followed by an
//!   example).
//!
//! Three finding kinds:
//!
//! * `form-error.no-identification` (3.3.1, strict) — invalid
//!   input with no `aria-describedby` or with a `describedby`
//!   pointing at a missing element.
//! * `form-error.no-suggestion` (3.3.3, warn) — message exists
//!   but doesn't suggest a fix.
//! * `form-error.both-ok` not emitted; the absence of findings
//!   IS the signal.
//!
//! Caller opt-out: `data-loom-form-no-suggest="true"` on the
//! input — use when the error is genuinely non-recoverable
//! (e.g. "server unavailable").
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured invalid input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InvalidInputHit {
    /// Selector path of the input.
    pub selector: String,
    /// Field name or id (whichever is present, for context).
    pub field: String,
    /// True if `aria-describedby` resolved to an existing element.
    pub has_described_message: bool,
    /// Text of the resolved message (capped at 120 chars), if any.
    pub message_text: String,
    /// True if message text contains corrective-suggestion shape.
    pub message_suggests_fix: bool,
    /// True if caller declared the error non-recoverable.
    pub no_suggest_exempt: bool,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FormErrorSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every invalid input the walker found.
    pub hits: Vec<InvalidInputHit>,
    /// Total form inputs walked.
    pub scanned: u32,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_form_errors(snap: &FormErrorSnapshot) -> Vec<AxisFinding> {
    let mut no_id: Vec<&InvalidInputHit> = Vec::new();
    let mut no_suggest: Vec<&InvalidInputHit> = Vec::new();
    for hit in &snap.hits {
        if !hit.has_described_message {
            no_id.push(hit);
            continue;
        }
        if !hit.message_suggests_fix && !hit.no_suggest_exempt {
            no_suggest.push(hit);
        }
    }
    let mut out = Vec::new();
    if !no_id.is_empty() {
        let ex: Vec<String> = no_id
            .iter()
            .take(5)
            .map(|h| format!("{} field=\"{}\"", h.selector, h.field))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "form-error.no-identification".to_owned(),
            detail: format!(
                "WCAG 3.3.1 — {} invalid form input(s) lack a programmatic error message: `aria-invalid=\"true\"` is set but `aria-describedby` is missing or points at a non-existent element. Add a visible message element + reference it via aria-describedby. Examples: {}",
                no_id.len(),
                ex.join("; ")
            ),
        });
    }
    if !no_suggest.is_empty() {
        let ex: Vec<String> = no_suggest
            .iter()
            .take(5)
            .map(|h| {
                format!(
                    "{} field=\"{}\" message=\"{}\"",
                    h.selector, h.field, h.message_text
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "form-error.no-suggestion".to_owned(),
            detail: format!(
                "WCAG 3.3.3 — {} invalid form input(s) have an error message but it doesn't suggest a corrective action. Rewrite to include a verb (\"enter\", \"use\", \"try\") + example. If the error is genuinely non-recoverable, declare `data-loom-form-no-suggest=\"true\"` on the input. Examples: {}",
                no_suggest.len(),
                ex.join("; ")
            ),
        });
    }
    out
}

/// Browser-side capture.
pub const FORM_ERROR_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
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

    // Heuristic for "suggests fix" — look for corrective-action
    // verbs / format hints in the message text.
    const suggestionShapes = [
      'must',
      'should',
      'use ',
      'enter ',
      'try ',
      'example',
      'format:',
      'e.g.',
      'such as',
      'at least',
      'at most',
      'min ',
      'max ',
      'maximum',
      'minimum',
    ];

    const inputs = document.querySelectorAll('input, select, textarea');
    const hits = [];
    let scanned = 0;
    for (let i = 0; i < inputs.length; i++) {
      const el = inputs[i];
      scanned += 1;
      // Skip hidden / type=hidden / submit / button.
      const tag = el.tagName.toLowerCase();
      if (tag === 'input') {
        const t = (el.getAttribute('type') || 'text').toLowerCase();
        if (t === 'hidden' || t === 'submit' || t === 'button' || t === 'reset') continue;
      }
      const ariaInvalid = (el.getAttribute('aria-invalid') || '').toLowerCase() === 'true';
      // checkValidity exists on form-associated elements.
      let nativeInvalid = false;
      try {
        nativeInvalid = el.willValidate === true && el.checkValidity && !el.checkValidity();
      } catch (_) {}
      if (!ariaInvalid && !nativeInvalid) continue;

      const noSuggestExempt = el.getAttribute('data-loom-form-no-suggest') === 'true';
      const field = el.getAttribute('name') || el.getAttribute('id') || '';

      const describedBy = el.getAttribute('aria-describedby') || '';
      let messageText = '';
      let hasDescribedMessage = false;
      if (describedBy) {
        const ids = describedBy.split(/\s+/).filter(function(x) { return x.length > 0; });
        for (let j = 0; j < ids.length; j++) {
          const node = document.getElementById(ids[j]);
          if (node && node.textContent && node.textContent.trim().length > 0) {
            messageText = (messageText + ' ' + node.textContent.trim()).trim();
            hasDescribedMessage = true;
          }
        }
      }
      messageText = messageText.slice(0, 120);

      let suggests = false;
      if (hasDescribedMessage) {
        const lower = messageText.toLowerCase();
        for (let s = 0; s < suggestionShapes.length; s++) {
          if (lower.indexOf(suggestionShapes[s]) !== -1) {
            suggests = true;
            break;
          }
        }
      }

      hits.push({
        selector: selectorOf(el),
        field: field,
        hasDescribedMessage: hasDescribedMessage,
        messageText: messageText,
        messageSuggestsFix: suggests,
        noSuggestExempt: noSuggestExempt,
      });
      if (hits.length >= 50) break;
    }

    return { pageUrl: window.location.href, hits: hits, scanned: scanned };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snap() -> FormErrorSnapshot {
        FormErrorSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            hits: Vec::new(),
            scanned: 0,
        }
    }

    fn hit(field: &str, has_msg: bool, suggests: bool, exempt: bool) -> InvalidInputHit {
        InvalidInputHit {
            selector: format!("body > form > input[name={field}]"),
            field: field.to_owned(),
            has_described_message: has_msg,
            message_text: if has_msg { "Bad value".to_owned() } else { String::new() },
            message_suggests_fix: suggests,
            no_suggest_exempt: exempt,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = empty_snap();
        assert!(detect_form_errors(&s).is_empty());
    }

    #[test]
    fn missing_described_message_is_strict() {
        let mut s = empty_snap();
        s.hits.push(hit("email", false, false, false));
        let f = detect_form_errors(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "form-error.no-identification");
    }

    #[test]
    fn message_without_suggestion_is_warn() {
        let mut s = empty_snap();
        s.hits.push(hit("email", true, false, false));
        let f = detect_form_errors(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert_eq!(f[0].kind, "form-error.no-suggestion");
    }

    #[test]
    fn message_with_suggestion_no_finding() {
        let mut s = empty_snap();
        s.hits.push(hit("email", true, true, false));
        assert!(detect_form_errors(&s).is_empty());
    }

    #[test]
    fn no_suggest_exempt_does_not_warn() {
        let mut s = empty_snap();
        s.hits.push(hit("server", true, false, true));
        assert!(detect_form_errors(&s).is_empty());
    }

    #[test]
    fn mixed_emits_two_findings() {
        let mut s = empty_snap();
        s.hits.push(hit("email", false, false, false));
        s.hits.push(hit("phone", true, false, false));
        let f = detect_form_errors(&s);
        assert_eq!(f.len(), 2);
        let kinds: Vec<&str> = f.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"form-error.no-identification"));
        assert!(kinds.contains(&"form-error.no-suggestion"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut s = empty_snap();
        for i in 0..10 {
            s.hits.push(hit(&format!("f{i}"), false, false, false));
        }
        let f = detect_form_errors(&s);
        assert!(f[0].detail.contains("10 invalid"));
        let arrows = f[0].detail.matches(" field=\"").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let mut s = empty_snap();
        s.hits.push(hit("email", true, true, false));
        let j = serde_json::to_string(&s).expect("ser");
        let back: FormErrorSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.hits[0].field, "email");
        assert!(back.hits[0].message_suggests_fix);
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in FORM_ERROR_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }

    #[test]
    fn js_includes_opt_out_marker() {
        assert!(
            FORM_ERROR_DOM_CAPTURE_JS.contains("data-loom-form-no-suggest"),
            "capture JS missing the caller-side opt-out marker"
        );
    }

    #[test]
    fn js_includes_suggestion_shape_examples() {
        for s in ["'must'", "'example'", "'format:'"] {
            assert!(
                FORM_ERROR_DOM_CAPTURE_JS.contains(s),
                "capture JS missing suggestion-shape literal: {s}"
            );
        }
    }
}
