//! `form_field_outside_form` — form-associated controls without a
//! `<form>` association.
//!
//! HTML defines a small set of form-associated elements that
//! REQUIRE a form association to be meaningful: `<input>`,
//! `<select>`, `<textarea>`, `<button type="submit">`. The
//! association is established by either:
//!
//! * Living inside a `<form>` ancestor (most common), OR
//! * Carrying a `form="form-id"` attribute that points at a
//!   `<form>` with matching `id`.
//!
//! When neither holds, the control submits to nothing. Common
//! failure modes:
//!
//! * CMS author pastes a styled "subscribe" input into a
//!   paragraph or aside without realizing the parent isn't a
//!   `<form>`. Pressing Enter does nothing.
//! * SPA framework forgets to render the `<form>` wrapper in a
//!   particular code path, leaving orphan inputs.
//! * Migration from a legacy CMS strips `<form>` open/close tags
//!   while preserving the inputs.
//! * `<input type="submit">` outside any form: clicking submits
//!   to no endpoint, the user sees "nothing happened."
//!
//! HEURISTIC
//! ---------
//! 1. Walk every `<input>` (excluding `type="hidden"`,
//!    `type="button"`, `type="reset"` — those don't need a
//!    submit endpoint), `<select>`, `<textarea>`,
//!    `<button type="submit">`.
//! 2. Skip elements inside `<template>` / `<script>` (inert).
//! 3. For each, check association:
//!    a. If `form="x"` attribute is set AND a `<form id="x">`
//!       exists in the document → associated, skip.
//!    b. Else walk parent chain; if any ancestor is `<form>` →
//!       associated, skip.
//!    c. Else → orphan, flag.
//! 4. Cap surfaced offenders at 50; full count in
//!    `orphanCount`.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const FORM_FIELD_OUTSIDE_FORM_JS: &str = r##"(() => {
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

    const inInertContext = function(el) {
      let node = el.parentElement;
      while (node) {
        if (node.tagName === 'TEMPLATE') return true;
        if (node.tagName === 'SCRIPT') return true;
        node = node.parentElement;
      }
      return false;
    };

    const hasFormAncestor = function(el) {
      let node = el.parentElement;
      while (node) {
        if (node.tagName === 'FORM') return true;
        node = node.parentElement;
      }
      return false;
    };

    const submitButtonSelector = 'button[type="submit"], button:not([type])';
    const fieldSelector =
      'input:not([type="hidden"]):not([type="button"]):not([type="reset"]), select, textarea, ' +
      submitButtonSelector;
    const all = document.querySelectorAll(fieldSelector);
    let scanned = 0;
    const orphans = [];
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (inInertContext(el)) continue;
      scanned += 1;
      const formAttr = el.getAttribute('form') || '';
      if (formAttr !== '') {
        const target = document.getElementById(formAttr);
        if (target && target.tagName === 'FORM') continue;
        // form="x" but no matching <form id="x"> — flag as
        // dangling-form-ref orphan.
        orphans.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          inputType: el.getAttribute('type') || '',
          formAttr: formAttr,
          association: 'dangling-form-ref'
        });
        continue;
      }
      if (hasFormAncestor(el)) continue;
      orphans.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        inputType: el.getAttribute('type') || '',
        formAttr: '',
        association: 'no-form-ancestor'
      });
    }

    return {
      totalFields: scanned,
      orphans: orphans.slice(0, 50),
      orphanCount: orphans.length
    };
})()"##;

/// One orphan-field row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct OrphanField {
    /// CSS selector to the offender.
    pub selector: String,
    /// Element tag (lowercased).
    pub tag: String,
    /// Value of the `type` attribute, if any (mostly relevant for
    /// `<input>` and `<button>`).
    pub input_type: String,
    /// Value of the `form="x"` attribute, if any.
    pub form_attr: String,
    /// `no-form-ancestor` (no `<form>` in parent chain + no `form=`
    /// attribute) or `dangling-form-ref` (`form="x"` set but no
    /// `<form id="x">` exists).
    pub association: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct FormFieldOutsideFormSnapshot {
    /// Total form fields walked (excluding inert-context +
    /// type=hidden/button/reset).
    pub total_fields: u32,
    /// Top 50 orphan-field offenders.
    pub orphans: Vec<OrphanField>,
    /// Total orphan count (may exceed `orphans.len()` if
    /// truncated).
    pub orphan_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits one **strict** finding when at least one form field has
/// no form association. The control submits to nothing — pressing
/// Enter or clicking submit does nothing visible to the user.
#[must_use]
pub fn detect_form_field_outside_form_issues(
    snap: &FormFieldOutsideFormSnapshot,
) -> Vec<crate::AxisFinding> {
    if snap.orphans.is_empty() {
        return Vec::new();
    }
    let first = &snap.orphans[0];
    let type_note = if first.input_type.is_empty() {
        String::new()
    } else {
        format!(" type=\"{}\"", first.input_type)
    };
    let assoc_note = match first.association.as_str() {
        "dangling-form-ref" => format!(
            "form=\"{}\" but no matching <form id> exists",
            first.form_attr
        ),
        _ => "no <form> ancestor, no form= attr".to_owned(),
    };
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "form-field-outside-form.orphan".to_owned(),
        detail: format!(
            "{} form field(s) with no <form> association. Total fields scanned: {}. First offender: <{}{}> @ {} ({})",
            snap.orphan_count, snap.total_fields, first.tag, type_note, first.selector, assoc_note,
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
            FORM_FIELD_OUTSIDE_FORM_JS.matches('(').count(),
            FORM_FIELD_OUTSIDE_FORM_JS.matches(')').count()
        );
        assert_eq!(
            FORM_FIELD_OUTSIDE_FORM_JS.matches('{').count(),
            FORM_FIELD_OUTSIDE_FORM_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(FORM_FIELD_OUTSIDE_FORM_JS.starts_with("(() => {"));
        assert!(FORM_FIELD_OUTSIDE_FORM_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "totalFields",
            "orphans",
            "orphanCount",
            "selector",
            "tag",
            "inputType",
            "formAttr",
            "association",
        ] {
            assert!(FORM_FIELD_OUTSIDE_FORM_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_skips_non_submitting_input_types() {
        // type=hidden / button / reset don't need form
        // association — the detector must explicitly exclude
        // them.
        let s = FORM_FIELD_OUTSIDE_FORM_JS;
        assert!(s.contains(r#"not([type="hidden"])"#));
        assert!(s.contains(r#"not([type="button"])"#));
        assert!(s.contains(r#"not([type="reset"])"#));
    }

    #[test]
    fn js_includes_submit_button_variants() {
        // Both `<button type="submit">` AND `<button>` (default
        // type is submit) must be checked.
        let s = FORM_FIELD_OUTSIDE_FORM_JS;
        assert!(s.contains(r#"button[type="submit"]"#));
        assert!(s.contains("button:not([type])"));
    }

    #[test]
    fn js_resolves_form_attribute_via_getelementbyid() {
        // form="x" association: the JS must look up
        // document.getElementById(formAttr) AND verify the target
        // is actually a <form>.
        let s = FORM_FIELD_OUTSIDE_FORM_JS;
        assert!(s.contains("getElementById(formAttr)"));
        assert!(s.contains("'FORM'"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 12,
            orphans: vec![],
            orphan_count: 0,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn orphan_no_ancestor_emits_strict() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 4,
            orphans: vec![OrphanField {
                selector: "body > main > input".to_owned(),
                tag: "input".to_owned(),
                input_type: "email".to_owned(),
                form_attr: String::new(),
                association: "no-form-ancestor".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "form-field-outside-form.orphan");
        assert!(findings[0].detail.contains("<input type=\"email\">"));
        assert!(findings[0].detail.contains("no <form> ancestor"));
    }

    #[test]
    fn dangling_form_ref_surfaces_distinct_association() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 1,
            orphans: vec![OrphanField {
                selector: "x".to_owned(),
                tag: "input".to_owned(),
                input_type: "text".to_owned(),
                form_attr: "missing-form-id".to_owned(),
                association: "dangling-form-ref".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert!(findings[0]
            .detail
            .contains("form=\"missing-form-id\" but no matching"));
    }

    #[test]
    fn submit_button_outside_form_flags() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 1,
            orphans: vec![OrphanField {
                selector: "body > div > button".to_owned(),
                tag: "button".to_owned(),
                input_type: "submit".to_owned(),
                form_attr: String::new(),
                association: "no-form-ancestor".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert!(findings[0].detail.contains("<button type=\"submit\">"));
    }

    #[test]
    fn missing_input_type_omits_type_clause() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 1,
            orphans: vec![OrphanField {
                selector: "x".to_owned(),
                tag: "select".to_owned(),
                input_type: String::new(),
                form_attr: String::new(),
                association: "no-form-ancestor".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert!(!findings[0].detail.contains("type="));
    }

    #[test]
    fn truncated_count_surfaces_higher_than_array_len() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 200,
            orphans: vec![OrphanField {
                selector: "x".to_owned(),
                tag: "input".to_owned(),
                input_type: "text".to_owned(),
                form_attr: String::new(),
                association: "no-form-ancestor".to_owned(),
            }],
            orphan_count: 73,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert!(findings[0].detail.contains("73 form field"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 5,
            orphans: vec![OrphanField {
                selector: "body > div > input".to_owned(),
                tag: "input".to_owned(),
                input_type: "email".to_owned(),
                form_attr: String::new(),
                association: "no-form-ancestor".to_owned(),
            }],
            orphan_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"totalFields\":5"));
        assert!(json.contains("\"orphanCount\":1"));
        assert!(json.contains("\"inputType\":\"email\""));
        let back: FormFieldOutsideFormSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.orphans.len(), 1);
        assert_eq!(back.orphans[0].input_type, "email");
    }

    #[test]
    fn detail_includes_total_fields_count() {
        let snap = FormFieldOutsideFormSnapshot {
            total_fields: 42,
            orphans: vec![OrphanField {
                selector: "x".to_owned(),
                tag: "input".to_owned(),
                input_type: "text".to_owned(),
                form_attr: String::new(),
                association: "no-form-ancestor".to_owned(),
            }],
            orphan_count: 1,
        };
        let findings = detect_form_field_outside_form_issues(&snap);
        assert!(findings[0].detail.contains("Total fields scanned: 42"));
    }
}
