//! `form_pattern_mismatch` — flags `<input pattern="...">` whose
//! placeholder text doesn't validate against its own regex.
//!
//! Defect class: a hand-authored or stale form has an input
//! `pattern=` regex that doesn't accept its own `placeholder=`
//! example, so a user who literally types the suggestion gets a
//! native browser validation error. Common causes:
//!
//! 1. **Stale placeholder** — pattern was tightened (e.g. require
//!    `+1-555-…` country-code prefix) but the placeholder still
//!    shows the old shape (`555-555-5555`).
//! 2. **Wrong pattern** — copy-pasted from another field; the
//!    placeholder is correct but the pattern is for a different
//!    input kind.
//! 3. **Anchoring bug** — pattern uses `^…$` but the example has
//!    trailing whitespace, OR pattern lacks anchors and accepts
//!    too much.
//! 4. **Case sensitivity** — pattern requires `[A-Z]` but the
//!    placeholder is lowercase.
//!
//! ## Heuristic
//!
//! For each `<input pattern="X" placeholder="Y">`, the JS
//! evaluates `new RegExp("^(?:" + X + ")$").test(Y)`. The
//! `^(?:...)$` wrap mirrors HTML5's
//! [pattern semantics](https://html.spec.whatwg.org/multipage/input.html#attr-input-pattern):
//! the browser anchors the whole pattern. Inputs whose
//! placeholder fails this test are captured as hits.
//!
//! Inputs without `pattern=` are skipped (no contract to
//! verify). Inputs with `pattern=` but no `placeholder=` are
//! recorded with `placeholder_empty: true` for an
//! informational `warn` finding — operators should give users
//! an example matching the pattern.
//!
//! Skip these input types where `pattern` doesn't apply:
//! `checkbox`, `radio`, `submit`, `button`, `reset`, `image`,
//! `hidden`, `file`, `range`, `color`.
//!
//! ## Severity
//!
//! * **Strict** — `placeholder_matches: false`. User typing the
//!   suggested example gets a native validation error.
//! * **Warn** — `placeholder_empty: true`. No example to follow;
//!   pattern compliance becomes a guessing game.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured input — either a pattern/placeholder mismatch
/// or a pattern without a placeholder example.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FormPatternMismatchHit {
    /// CSS-ish path of the offending input.
    pub selector: String,
    /// The input's `pattern=` regex source.
    pub pattern: String,
    /// The input's `placeholder=` value (empty when missing).
    pub placeholder: String,
    /// Best-effort label string (associated `<label>` text, or
    /// `aria-label`, or `aria-labelledby` target text). Empty
    /// when none could be resolved — still flag, just without
    /// extra context.
    pub label: String,
    /// True iff `placeholder` is empty.
    pub placeholder_empty: bool,
    /// True iff the placeholder matched the pattern under
    /// HTML5 `^(?:…)$` anchoring. Only meaningful when
    /// `placeholder_empty: false`.
    pub placeholder_matches: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FormPatternMismatchSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Every input carrying `pattern=` (with or without
    /// placeholder).
    pub hits: Vec<FormPatternMismatchHit>,
    /// Total inputs walked (sans skipped types).
    pub scanned_inputs: u32,
}

/// Maximum number of examples reported per finding to keep
/// reports readable.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
///
/// Returns one Strict finding for inputs whose placeholder
/// fails its own pattern, and one Warn finding for inputs that
/// carry a pattern but no placeholder. Empty inputs section →
/// empty findings.
#[must_use]
pub fn detect_form_pattern_mismatch(snap: &FormPatternMismatchSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let mut mismatches: Vec<&FormPatternMismatchHit> = Vec::new();
    let mut empties: Vec<&FormPatternMismatchHit> = Vec::new();

    for hit in &snap.hits {
        if hit.placeholder_empty {
            empties.push(hit);
        } else if !hit.placeholder_matches {
            mismatches.push(hit);
        }
    }

    let mut out = Vec::new();

    if !mismatches.is_empty() {
        let examples: Vec<String> = mismatches
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                let label = if h.label.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", h.label)
                };
                format!(
                    "{}{} (pattern=`{}`, placeholder=`{}`)",
                    h.selector, label, h.pattern, h.placeholder
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "form-pattern-mismatch.placeholder-fails-pattern".to_owned(),
            detail: format!(
                "{} input(s) whose placeholder example fails its own pattern regex — users typing the suggested text will get a native validation error. Fix either the pattern or the placeholder so the example matches `^(?:pattern)$`. Examples: {}",
                mismatches.len(),
                examples.join("; ")
            ),
        });
    }

    if !empties.is_empty() {
        let examples: Vec<String> = empties
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                let label = if h.label.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", h.label)
                };
                format!("{}{} (pattern=`{}`)", h.selector, label, h.pattern)
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "form-pattern-mismatch.no-placeholder".to_owned(),
            detail: format!(
                "{} input(s) carry a pattern but no placeholder example — pattern compliance becomes a guessing game. Add a placeholder showing one valid value. Examples: {}",
                empties.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

/// Browser-side DOM-capture script. Mirror any change in this
/// file's `FormPatternMismatchHit` + snapshot fields.
///
/// The JS performs the `new RegExp("^(?:" + p + ")$").test(v)`
/// check directly so the snapshot the Rust detector consumes
/// only contains the verdict, not the regex engine's burden.
/// Patterns that fail to compile (bad regex) are captured with
/// `placeholder_matches: false` — that itself is the bug.
pub const FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS: &str = r#"
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

    const SKIP_TYPES = new Set([
      'checkbox', 'radio', 'submit', 'button', 'reset', 'image',
      'hidden', 'file', 'range', 'color'
    ]);

    const labelFor = function(input) {
      // <label for="X"> matching this input's id.
      if (input.id) {
        const l = document.querySelector('label[for="' + CSS.escape(input.id) + '"]');
        if (l && l.textContent) return l.textContent.trim().substring(0, 80);
      }
      // <label>…<input>…</label> ancestor.
      let p = input.parentElement;
      let depth = 0;
      while (p && depth < 5) {
        if (p.tagName === 'LABEL' && p.textContent) {
          return p.textContent.trim().substring(0, 80);
        }
        p = p.parentElement;
        depth += 1;
      }
      // aria-label / aria-labelledby fallback.
      const aria = input.getAttribute('aria-label');
      if (aria) return aria.trim().substring(0, 80);
      const labelledBy = input.getAttribute('aria-labelledby');
      if (labelledBy) {
        const ref = document.getElementById(labelledBy);
        if (ref && ref.textContent) return ref.textContent.trim().substring(0, 80);
      }
      return '';
    };

    const hits = [];
    let scanned = 0;
    const inputs = document.querySelectorAll('input[pattern]');
    for (const input of inputs) {
      const type = (input.getAttribute('type') || 'text').toLowerCase();
      if (SKIP_TYPES.has(type)) continue;
      scanned += 1;
      const pattern = input.getAttribute('pattern') || '';
      const placeholder = input.getAttribute('placeholder') || '';
      const placeholderEmpty = placeholder.length === 0;
      let placeholderMatches = false;
      if (!placeholderEmpty) {
        try {
          // HTML5 pattern semantics: implicit ^(?:…)$ wrap.
          const re = new RegExp('^(?:' + pattern + ')$');
          placeholderMatches = re.test(placeholder);
        } catch (_e) {
          // Invalid regex — treat as mismatch; the bad pattern
          // is itself the bug to surface.
          placeholderMatches = false;
        }
      }
      hits.push({
        selector: selectorOf(input),
        pattern: pattern,
        placeholder: placeholder,
        label: labelFor(input),
        placeholderEmpty: placeholderEmpty,
        placeholderMatches: placeholderMatches
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedInputs: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        pattern: &str,
        placeholder: &str,
        empty: bool,
        matches: bool,
    ) -> FormPatternMismatchHit {
        FormPatternMismatchHit {
            selector: selector.into(),
            pattern: pattern.into(),
            placeholder: placeholder.into(),
            label: String::new(),
            placeholder_empty: empty,
            placeholder_matches: matches,
        }
    }

    fn snap(hits: Vec<FormPatternMismatchHit>) -> FormPatternMismatchSnapshot {
        FormPatternMismatchSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_inputs: 0,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_form_pattern_mismatch(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn placeholder_matches_pattern_no_finding() {
        // Phone pattern + matching placeholder → no flag.
        let s = snap(vec![hit(
            "#phone",
            "[0-9]{3}-[0-9]{4}",
            "555-1234",
            false,
            true,
        )]);
        let findings = detect_form_pattern_mismatch(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn placeholder_fails_pattern_is_strict() {
        // Pattern requires digits + dash; placeholder is wrong shape.
        let s = snap(vec![hit(
            "#phone",
            "[0-9]{3}-[0-9]{4}",
            "555-555-5555",
            false,
            false,
        )]);
        let findings = detect_form_pattern_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(
            findings[0].kind,
            "form-pattern-mismatch.placeholder-fails-pattern"
        );
        assert!(findings[0].detail.contains("#phone"));
        assert!(findings[0].detail.contains("555-555-5555"));
    }

    #[test]
    fn empty_placeholder_is_warn() {
        let s = snap(vec![hit("#zip", "[0-9]{5}", "", true, false)]);
        let findings = detect_form_pattern_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "form-pattern-mismatch.no-placeholder");
        assert!(findings[0].detail.contains("#zip"));
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(vec![
            hit("#a", "[0-9]+", "abc", false, false),
            hit("#b", "[0-9]+", "", true, false),
            hit("#c", "[0-9]+", "999", false, true),
        ]);
        let findings = detect_form_pattern_mismatch(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"form-pattern-mismatch.placeholder-fails-pattern"));
        assert!(kinds.contains(&"form-pattern-mismatch.no-placeholder"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit("#email", "[a-z]+@[a-z]+", "BAD", false, false);
        h.label = "Email address".into();
        let s = snap(vec![h]);
        let findings = detect_form_pattern_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Email address]"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!("#input-{i}"),
                "[0-9]+",
                "abc",
                false,
                false,
            ));
        }
        let s = snap(hits);
        let findings = detect_form_pattern_mismatch(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 input(s)"));
        // 5 examples joined by "; " → 4 semicolons in the
        // examples list. Body message must not introduce extra
        // semicolons.
        let semicolons = findings[0].detail.matches(';').count();
        assert_eq!(semicolons, 4, "5 examples → 4 semicolons");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: the JS exposes the documented fields.
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("hits"));
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("scannedInputs"));
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("placeholderEmpty"));
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("placeholderMatches"));
        // HTML5 pattern anchoring contract.
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("^(?:"));
        // Skip-types contract — at least the high-confidence ones.
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("'checkbox'"));
        assert!(FORM_PATTERN_MISMATCH_DOM_CAPTURE_JS.contains("'hidden'"));
    }
}
