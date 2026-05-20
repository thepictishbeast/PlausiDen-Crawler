//! `empty_button` — `<button>` accessible-name detector.
//!
//! Sister detector to `link_text` (which catches empty `<a>`) and
//! `iframe_title` (which catches title-less `<iframe>`). Catches
//! `<button>` elements with no accessible name:
//!
//! 1. No visible text content (after trimming).
//! 2. No `aria-label` (non-empty).
//! 3. No `aria-labelledby` (resolves to non-empty text).
//! 4. No inner image / svg with non-empty `alt` / `aria-label` /
//!    `<title>`.
//!
//! Screen-reader users hear "button" with no further context —
//! they cannot determine the button's purpose. WCAG 2.1 SC 4.1.2
//! (Name, Role, Value, A) violation.
//!
//! Out of scope:
//! * `<button type="submit">` inside a form that's the canonical
//!   form-submit button — the form's accessible name usually
//!   conveys intent. We still flag those at warn (vs. strict)
//!   when no other name source exists.
//! * `<button hidden>` / `disabled` — hidden / disabled buttons
//!   are filtered out by visibility check.
//! * `role="button"` on non-button elements — covered by a
//!   parallel detector (not this one).
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const EMPTY_BUTTON_JS: &str = r##"(() => {
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

    const isVisible = function(el) {
      if (el.hasAttribute('hidden')) return false;
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const op = parseFloat(cs.opacity);
      if (!isNaN(op) && op === 0) return false;
      return true;
    };

    const resolveLabelledBy = function(labelledby) {
      if (!labelledby) return '';
      const ids = labelledby.split(/\s+/).filter(function(s) { return s.length > 0; });
      const parts = [];
      for (let i = 0; i < ids.length; i++) {
        const ref = document.getElementById(ids[i]);
        if (ref) parts.push((ref.textContent || '').trim());
      }
      return parts.join(' ').trim();
    };

    // Inner image / svg with an accessible name counts as the
    // button's accessible name.
    const innerImageName = function(el) {
      const imgs = el.querySelectorAll('img, svg');
      for (let i = 0; i < imgs.length; i++) {
        const img = imgs[i];
        if (img.tagName.toLowerCase() === 'img') {
          const alt = img.getAttribute('alt');
          if (alt !== null && alt.trim().length > 0) return alt.trim();
        }
        // SVG: aria-label or <title> child.
        const aria = img.getAttribute('aria-label');
        if (aria && aria.trim().length > 0) return aria.trim();
        const titleEl = img.querySelector('title');
        if (titleEl) {
          const t = (titleEl.textContent || '').trim();
          if (t.length > 0) return t;
        }
      }
      return '';
    };

    let scanned = 0;
    const offenders = [];

    const buttons = document.querySelectorAll('button');
    for (let i = 0; i < buttons.length; i++) {
      const el = buttons[i];
      scanned += 1;
      if (!isVisible(el)) continue;
      // Visible text.
      const text = (el.textContent || '').trim();
      if (text.length > 0) continue;
      // aria-label.
      const ariaLabel = el.getAttribute('aria-label');
      if (ariaLabel && ariaLabel.trim().length > 0) continue;
      // aria-labelledby.
      const labelledby = el.getAttribute('aria-labelledby');
      const labelledText = resolveLabelledBy(labelledby);
      if (labelledText.length > 0) continue;
      // Inner image / svg accessible name.
      const imgName = innerImageName(el);
      if (imgName.length > 0) continue;

      // No accessible name. Classify by button type.
      const type = (el.getAttribute('type') || 'button').toLowerCase();
      const inForm = el.closest('form') !== null;
      const hasOnlyEmptyAttrs =
        (ariaLabel !== null && ariaLabel.trim().length === 0)
        || (labelledby !== null && labelledText.length === 0);

      offenders.push({
        selector: selectorOf(el),
        type: type,
        inForm: inForm,
        hasOnlyEmptyAttrs: hasOnlyEmptyAttrs
      });
      if (offenders.length >= 50) break;
    }

    return {
      scanned: scanned,
      offenderCount: offenders.length,
      offenders: offenders
    };
})()"##;

/// One empty-button offender row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyButtonOffender {
    /// CSS selector for the button.
    pub selector: String,
    /// `type=` attribute (`button` / `submit` / `reset`).
    pub r#type: String,
    /// `true` if the button is inside a `<form>`.
    pub in_form: bool,
    /// `true` if `aria-label=""` or `aria-labelledby` resolves to
    /// empty text — caller had the binding right but the value is
    /// empty. (Subtly different from "no aria-label at all.")
    pub has_only_empty_attrs: bool,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyButtonSnapshot {
    /// Total `<button>` elements walked.
    pub scanned: u32,
    /// Number of offenders (capped at 50; count honest).
    pub offender_count: u32,
    /// Per-offender rows.
    pub offenders: Vec<EmptyButtonOffender>,
}

/// Apply detection rules. Pure function.
///
/// Emits at most one finding (strict). Surfaces the first offender's
/// type + form-presence so the operator can triage:
/// * `type=button` standalone → almost always a real bug
/// * `type=submit` inside a form → still a bug, but the form's
///   submit-button label often implies intent; detail message
///   gives the form-aware nudge.
#[must_use]
pub fn detect_empty_button_issues(snap: &EmptyButtonSnapshot) -> Vec<crate::AxisFinding> {
    if snap.offenders.is_empty() {
        return Vec::new();
    }
    let first = &snap.offenders[0];
    let context = if first.in_form {
        format!(
            "type={} inside a <form>; the form's submit ARIA still needs a concrete name",
            first.r#type
        )
    } else {
        format!("type={} standalone", first.r#type)
    };
    let attr_note = if first.has_only_empty_attrs {
        " (one of aria-label / aria-labelledby is present but empty — populate it or remove it)"
    } else {
        ""
    };
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "empty-button.no-accessible-name".to_owned(),
        detail: format!(
            "{} <button> element(s) lack an accessible name (no text, no aria-label, no aria-labelledby, no inner img/svg alt). Screen-reader users hear 'button' with no further context. WCAG 2.1 SC 4.1.2. First offender: {} @ {}{}. Add visible text inside the button or `aria-label=\"<purpose>\"` describing the action.",
            snap.offender_count,
            context,
            first.selector,
            attr_note,
        ),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            EMPTY_BUTTON_JS.matches('(').count(),
            EMPTY_BUTTON_JS.matches(')').count()
        );
        assert_eq!(
            EMPTY_BUTTON_JS.matches('{').count(),
            EMPTY_BUTTON_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(EMPTY_BUTTON_JS.starts_with("(() => {"));
        assert!(EMPTY_BUTTON_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "scanned",
            "offenderCount",
            "offenders",
            "selector",
            "type",
            "inForm",
            "hasOnlyEmptyAttrs",
        ] {
            assert!(EMPTY_BUTTON_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_checks_inner_image_accessible_name() {
        assert!(EMPTY_BUTTON_JS.contains("innerImageName"));
        assert!(EMPTY_BUTTON_JS.contains("querySelectorAll('img, svg')"));
    }

    #[test]
    fn js_resolves_aria_labelledby() {
        assert!(EMPTY_BUTTON_JS.contains("resolveLabelledBy"));
        assert!(EMPTY_BUTTON_JS.contains("getElementById"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = EmptyButtonSnapshot {
            scanned: 4,
            offender_count: 0,
            offenders: vec![],
        };
        let findings = detect_empty_button_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn empty_standalone_button_emits_strict() {
        let snap = EmptyButtonSnapshot {
            scanned: 2,
            offender_count: 1,
            offenders: vec![EmptyButtonOffender {
                selector: "body > nav > button".to_owned(),
                r#type: "button".to_owned(),
                in_form: false,
                has_only_empty_attrs: false,
            }],
        };
        let findings = detect_empty_button_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "empty-button.no-accessible-name");
        assert!(findings[0].detail.contains("WCAG 2.1 SC 4.1.2"));
        assert!(findings[0].detail.contains("type=button standalone"));
        assert!(findings[0].detail.contains(r#"aria-label="<purpose>""#));
    }

    #[test]
    fn empty_form_submit_button_calls_out_form_context() {
        let snap = EmptyButtonSnapshot {
            scanned: 1,
            offender_count: 1,
            offenders: vec![EmptyButtonOffender {
                selector: "body > form > button".to_owned(),
                r#type: "submit".to_owned(),
                in_form: true,
                has_only_empty_attrs: false,
            }],
        };
        let findings = detect_empty_button_issues(&snap);
        assert!(findings[0].detail.contains("type=submit inside a <form>"));
        assert!(findings[0].detail.contains("submit ARIA still needs"));
    }

    #[test]
    fn empty_aria_attribute_surfaces_in_detail() {
        let snap = EmptyButtonSnapshot {
            scanned: 1,
            offender_count: 1,
            offenders: vec![EmptyButtonOffender {
                selector: "x".to_owned(),
                r#type: "button".to_owned(),
                in_form: false,
                has_only_empty_attrs: true,
            }],
        };
        let findings = detect_empty_button_issues(&snap);
        assert!(findings[0]
            .detail
            .contains("aria-label / aria-labelledby is present but empty"));
    }

    #[test]
    fn multiple_offenders_surface_count() {
        let mut offenders = Vec::new();
        for i in 0..7 {
            offenders.push(EmptyButtonOffender {
                selector: format!("button[{i}]"),
                r#type: "button".to_owned(),
                in_form: false,
                has_only_empty_attrs: false,
            });
        }
        let snap = EmptyButtonSnapshot {
            scanned: 10,
            offender_count: 7,
            offenders,
        };
        let findings = detect_empty_button_issues(&snap);
        assert!(findings[0].detail.contains("7 <button>"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = EmptyButtonSnapshot {
            scanned: 3,
            offender_count: 1,
            offenders: vec![EmptyButtonOffender {
                selector: "body > x".to_owned(),
                r#type: "submit".to_owned(),
                in_form: true,
                has_only_empty_attrs: true,
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":3"));
        assert!(json.contains("\"offenderCount\":1"));
        assert!(json.contains("\"inForm\":true"));
        assert!(json.contains("\"hasOnlyEmptyAttrs\":true"));
        let back: EmptyButtonSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.offenders.len(), 1);
        assert!(back.offenders[0].in_form);
        assert!(back.offenders[0].has_only_empty_attrs);
        assert_eq!(back.offenders[0].r#type, "submit");
    }

    #[test]
    fn truncated_count_above_array_len_honest() {
        let snap = EmptyButtonSnapshot {
            scanned: 200,
            offender_count: 73,
            offenders: vec![EmptyButtonOffender {
                selector: "x".to_owned(),
                r#type: "button".to_owned(),
                in_form: false,
                has_only_empty_attrs: false,
            }],
        };
        let findings = detect_empty_button_issues(&snap);
        assert!(findings[0].detail.contains("73 <button>"));
    }
}
