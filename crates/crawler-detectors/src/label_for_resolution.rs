//! `label_for_resolution` — `<label for="…">` target validity detector.
//!
//! Distinct from `form_labels` (which catches inputs with NO label):
//! this detector catches labels where the `for=` reference is broken
//! — the label is syntactically present but points at nothing, or
//! points at an element that isn't a form control.
//!
//! Cross-references `fragment_anchor` (which checks anchor href
//! references) and `unique_id` (which checks id-side hygiene). Even
//! with both passing, a label can still be broken: the operator typed
//! the id wrong, or pointed at a `<div>` instead of an `<input>`.
//!
//! WCAG 1.3.1 (Info and Relationships, A) + 3.3.2 (Labels or
//! Instructions, A) + 4.1.2 (Name, Role, Value, A) all implicated:
//! a broken `for` reference means screen readers announce the wrong
//! label (or no label) when the input receives focus.
//!
//! HEURISTIC
//! ---------
//! Walk every `<label for=…>`:
//!
//! 1. If the `for` value is empty → flag (warn): an empty attribute
//!    should be omitted, not present-but-blank.
//! 2. Look up `document.getElementById(for_value)`:
//!    * No match → strict finding (broken reference).
//!    * Multiple matches (via querySelectorAll) → cross-references
//!      `unique_id` but call it out here too — the label binding is
//!      ambiguous.
//!    * Match exists but the target's tag is not a form control →
//!      strict finding (label points at a non-control element).
//!
//! Recognized form controls (HTML living standard `labelable`):
//! `input` (except type=hidden), `select`, `textarea`, `button`,
//! `progress`, `meter`, `output`. Custom elements with a `role`
//! that matches a form-control role also count.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const LABEL_FOR_RESOLUTION_JS: &str = r##"(() => {
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

    const LABELABLE_TAGS = ['input', 'select', 'textarea', 'button', 'progress', 'meter', 'output'];
    const LABELABLE_ROLES = [
      'textbox', 'checkbox', 'radio', 'combobox', 'listbox',
      'searchbox', 'slider', 'spinbutton', 'switch'
    ];

    const isLabelable = function(el) {
      const tag = el.tagName.toLowerCase();
      if (tag === 'input') {
        const type = (el.getAttribute('type') || 'text').toLowerCase();
        return type !== 'hidden';
      }
      if (LABELABLE_TAGS.indexOf(tag) !== -1) return true;
      const role = el.getAttribute('role');
      if (role && LABELABLE_ROLES.indexOf(role) !== -1) return true;
      // Common custom-element pattern: contenteditable="true".
      if (el.getAttribute('contenteditable') === 'true') return true;
      return false;
    };

    let scanned = 0;
    const empty = [];
    const missing = [];
    const ambiguous = [];
    const wrongType = [];

    const labels = document.querySelectorAll('label[for]');
    for (let i = 0; i < labels.length; i++) {
      const el = labels[i];
      scanned += 1;
      const forVal = (el.getAttribute('for') || '').trim();
      const text = (el.textContent || '').trim().slice(0, 60);
      if (forVal === '') {
        empty.push({
          selector: selectorOf(el),
          text: text
        });
        continue;
      }
      // Count matches via querySelectorAll to surface ambiguity.
      // Note: id values can in theory contain attribute-selector-
      // breaking characters; escape minimally.
      const targets = document.querySelectorAll('[id]');
      let matchCount = 0;
      let firstMatch = null;
      for (let j = 0; j < targets.length; j++) {
        if (targets[j].getAttribute('id') === forVal) {
          matchCount += 1;
          if (firstMatch === null) firstMatch = targets[j];
        }
      }
      if (matchCount === 0) {
        missing.push({
          selector: selectorOf(el),
          text: text,
          forValue: forVal
        });
        continue;
      }
      if (matchCount > 1) {
        ambiguous.push({
          selector: selectorOf(el),
          text: text,
          forValue: forVal,
          count: matchCount
        });
        continue;
      }
      // Single match — check the target is labelable.
      if (!isLabelable(firstMatch)) {
        wrongType.push({
          selector: selectorOf(el),
          text: text,
          forValue: forVal,
          targetTag: firstMatch.tagName.toLowerCase(),
          targetRole: firstMatch.getAttribute('role') || ''
        });
      }
    }

    return {
      scanned: scanned,
      empty: empty.slice(0, 50),
      emptyCount: empty.length,
      missing: missing.slice(0, 50),
      missingCount: missing.length,
      ambiguous: ambiguous.slice(0, 50),
      ambiguousCount: ambiguous.length,
      wrongType: wrongType.slice(0, 50),
      wrongTypeCount: wrongType.length
    };
})()"##;

/// One empty-for offender: `<label for="">`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyForLabel {
    /// CSS selector for the label.
    pub selector: String,
    /// First 60 chars of label text.
    pub text: String,
}

/// One missing-target offender: `<label for="x">` where no element
/// has `id="x"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct MissingForTarget {
    /// CSS selector for the label.
    pub selector: String,
    /// First 60 chars of label text.
    pub text: String,
    /// The `for` attribute value that didn't resolve.
    pub for_value: String,
}

/// One ambiguous-target offender: `<label for="x">` where multiple
/// elements have `id="x"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousForTarget {
    /// CSS selector for the label.
    pub selector: String,
    /// First 60 chars of label text.
    pub text: String,
    /// The `for` attribute value.
    pub for_value: String,
    /// Number of elements sharing that id.
    pub count: u32,
}

/// One wrong-type offender: `<label for="x">` where the target exists
/// but is not a form control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct WrongTypeTarget {
    /// CSS selector for the label.
    pub selector: String,
    /// First 60 chars of label text.
    pub text: String,
    /// The `for` value.
    pub for_value: String,
    /// Target tag (lowercased).
    pub target_tag: String,
    /// Target `role` attribute, if any.
    pub target_role: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct LabelForResolutionSnapshot {
    /// Total `<label for=…>` walked.
    pub scanned: u32,
    /// Labels with `for=""`.
    pub empty: Vec<EmptyForLabel>,
    /// Total empty-for labels.
    pub empty_count: u32,
    /// Labels pointing at a non-existent id.
    pub missing: Vec<MissingForTarget>,
    /// Total missing-target labels.
    pub missing_count: u32,
    /// Labels pointing at a duplicated id.
    pub ambiguous: Vec<AmbiguousForTarget>,
    /// Total ambiguous labels.
    pub ambiguous_count: u32,
    /// Labels pointing at a non-form-control element.
    pub wrong_type: Vec<WrongTypeTarget>,
    /// Total wrong-type labels.
    pub wrong_type_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits up to four findings — one per category that contains
/// offenders. Severity:
/// * missing → strict
/// * wrong-type → strict
/// * ambiguous → strict (cross-references unique_id)
/// * empty → warn
#[must_use]
pub fn detect_label_for_resolution_issues(
    snap: &LabelForResolutionSnapshot,
) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    if !snap.missing.is_empty() {
        let first = &snap.missing[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "label-for.missing-target".to_owned(),
            detail: format!(
                "{} <label for=\"…\"> reference(s) point at a non-existent id. Screen readers announce the wrong label (or none) when the input receives focus. WCAG 1.3.1 + 4.1.2. First: <label for=\"{}\"> \"{}\" @ {}. Add `id=\"{}\"` to the intended target, or wrap the input inside the label.",
                snap.missing_count,
                first.for_value,
                first.text,
                first.selector,
                first.for_value,
            ),
        });
    }
    if !snap.wrong_type.is_empty() {
        let first = &snap.wrong_type[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "label-for.wrong-target-type".to_owned(),
            detail: format!(
                "{} <label for=\"…\"> reference(s) target a non-form-control element. WCAG 1.3.1 + 3.3.2 — clicking the label doesn't focus an input. First: <label for=\"{}\"> \"{}\" @ {} targets <{}{}>. Point `for` at an actual form control (input / select / textarea / button / progress / meter / output) or a custom element with a labelable ARIA role.",
                snap.wrong_type_count,
                first.for_value,
                first.text,
                first.selector,
                first.target_tag,
                if first.target_role.is_empty() {
                    String::new()
                } else {
                    format!(" role={}", first.target_role)
                },
            ),
        });
    }
    if !snap.ambiguous.is_empty() {
        let first = &snap.ambiguous[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "label-for.ambiguous-target".to_owned(),
            detail: format!(
                "{} <label for=\"…\"> reference(s) target an id that appears {}× on the page. Browser binds to the FIRST match — the operator's intent is ambiguous. First: <label for=\"{}\"> \"{}\" @ {}. Resolve via the `unique_id` detector + this label's intended destination.",
                snap.ambiguous_count,
                first.count,
                first.for_value,
                first.text,
                first.selector,
            ),
        });
    }
    if !snap.empty.is_empty() {
        let first = &snap.empty[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "label-for.empty-attribute".to_owned(),
            detail: format!(
                "{} <label for=\"\"> with empty `for` attribute. Either populate it or remove the attribute entirely. First: \"{}\" @ {}.",
                snap.empty_count,
                first.text,
                first.selector,
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            LABEL_FOR_RESOLUTION_JS.matches('(').count(),
            LABEL_FOR_RESOLUTION_JS.matches(')').count()
        );
        assert_eq!(
            LABEL_FOR_RESOLUTION_JS.matches('{').count(),
            LABEL_FOR_RESOLUTION_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(LABEL_FOR_RESOLUTION_JS.starts_with("(() => {"));
        assert!(LABEL_FOR_RESOLUTION_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "scanned",
            "empty", "emptyCount",
            "missing", "missingCount",
            "ambiguous", "ambiguousCount",
            "wrongType", "wrongTypeCount",
            "selector", "text", "forValue", "count", "targetTag", "targetRole",
        ] {
            assert!(LABEL_FOR_RESOLUTION_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_enumerates_labelable_tags() {
        for tag in ["input", "select", "textarea", "button", "progress", "meter", "output"] {
            assert!(
                LABEL_FOR_RESOLUTION_JS.contains(&format!("'{tag}'")),
                "missing labelable tag: {tag}"
            );
        }
    }

    #[test]
    fn js_enumerates_labelable_aria_roles() {
        for role in ["textbox", "checkbox", "radio", "combobox", "listbox", "searchbox", "slider", "spinbutton", "switch"] {
            assert!(
                LABEL_FOR_RESOLUTION_JS.contains(&format!("'{role}'")),
                "missing labelable role: {role}"
            );
        }
    }

    #[test]
    fn js_excludes_input_type_hidden() {
        assert!(LABEL_FOR_RESOLUTION_JS.contains("type !== 'hidden'"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = LabelForResolutionSnapshot {
            scanned: 5,
            empty: vec![],
            empty_count: 0,
            missing: vec![],
            missing_count: 0,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![],
            wrong_type_count: 0,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_target_emits_strict() {
        let snap = LabelForResolutionSnapshot {
            scanned: 3,
            empty: vec![],
            empty_count: 0,
            missing: vec![MissingForTarget {
                selector: "body > form > label".to_owned(),
                text: "Email".to_owned(),
                for_value: "email-input".to_owned(),
            }],
            missing_count: 1,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![],
            wrong_type_count: 0,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "label-for.missing-target");
        assert!(findings[0].detail.contains(r#"for="email-input""#));
        assert!(findings[0].detail.contains(r#"id="email-input""#));
        assert!(findings[0].detail.contains("WCAG 1.3.1 + 4.1.2"));
    }

    #[test]
    fn wrong_type_target_emits_strict() {
        let snap = LabelForResolutionSnapshot {
            scanned: 2,
            empty: vec![],
            empty_count: 0,
            missing: vec![],
            missing_count: 0,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![WrongTypeTarget {
                selector: "body > form > label".to_owned(),
                text: "Username".to_owned(),
                for_value: "user-section".to_owned(),
                target_tag: "div".to_owned(),
                target_role: String::new(),
            }],
            wrong_type_count: 1,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "label-for.wrong-target-type");
        assert!(findings[0].detail.contains("<div>"));
        assert!(findings[0].detail.contains("non-form-control"));
    }

    #[test]
    fn wrong_type_target_with_role_surfaces_role() {
        let snap = LabelForResolutionSnapshot {
            scanned: 1,
            empty: vec![],
            empty_count: 0,
            missing: vec![],
            missing_count: 0,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![WrongTypeTarget {
                selector: "x".to_owned(),
                text: "T".to_owned(),
                for_value: "t".to_owned(),
                target_tag: "span".to_owned(),
                target_role: "tab".to_owned(),
            }],
            wrong_type_count: 1,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert!(findings[0].detail.contains("<span role=tab>"));
    }

    #[test]
    fn ambiguous_target_emits_strict_with_count() {
        let snap = LabelForResolutionSnapshot {
            scanned: 2,
            empty: vec![],
            empty_count: 0,
            missing: vec![],
            missing_count: 0,
            ambiguous: vec![AmbiguousForTarget {
                selector: "x".to_owned(),
                text: "Multi".to_owned(),
                for_value: "name".to_owned(),
                count: 3,
            }],
            ambiguous_count: 1,
            wrong_type: vec![],
            wrong_type_count: 0,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert!(findings[0].detail.contains("3×"));
        assert!(findings[0].detail.contains("unique_id"));
    }

    #[test]
    fn empty_for_emits_warn() {
        let snap = LabelForResolutionSnapshot {
            scanned: 1,
            empty: vec![EmptyForLabel {
                selector: "y".to_owned(),
                text: "Field".to_owned(),
            }],
            empty_count: 1,
            missing: vec![],
            missing_count: 0,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![],
            wrong_type_count: 0,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert!(findings[0].detail.contains(r#"<label for="">"#));
    }

    #[test]
    fn all_four_categories_emit_four_findings() {
        let snap = LabelForResolutionSnapshot {
            scanned: 8,
            empty: vec![EmptyForLabel {
                selector: "e".to_owned(),
                text: "e".to_owned(),
            }],
            empty_count: 1,
            missing: vec![MissingForTarget {
                selector: "m".to_owned(),
                text: "m".to_owned(),
                for_value: "m".to_owned(),
            }],
            missing_count: 1,
            ambiguous: vec![AmbiguousForTarget {
                selector: "a".to_owned(),
                text: "a".to_owned(),
                for_value: "a".to_owned(),
                count: 2,
            }],
            ambiguous_count: 1,
            wrong_type: vec![WrongTypeTarget {
                selector: "w".to_owned(),
                text: "w".to_owned(),
                for_value: "w".to_owned(),
                target_tag: "section".to_owned(),
                target_role: String::new(),
            }],
            wrong_type_count: 1,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert_eq!(findings.len(), 4);
        assert_eq!(findings[0].kind, "label-for.missing-target");
        assert_eq!(findings[1].kind, "label-for.wrong-target-type");
        assert_eq!(findings[2].kind, "label-for.ambiguous-target");
        assert_eq!(findings[3].kind, "label-for.empty-attribute");
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = LabelForResolutionSnapshot {
            scanned: 4,
            empty: vec![EmptyForLabel {
                selector: "x".to_owned(),
                text: "RT".to_owned(),
            }],
            empty_count: 1,
            missing: vec![MissingForTarget {
                selector: "y".to_owned(),
                text: "M".to_owned(),
                for_value: "m".to_owned(),
            }],
            missing_count: 1,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![],
            wrong_type_count: 0,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":4"));
        let back: LabelForResolutionSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.empty.len(), 1);
        assert_eq!(back.missing[0].for_value, "m");
    }

    #[test]
    fn truncated_counts_above_array_len_surface_correctly() {
        let snap = LabelForResolutionSnapshot {
            scanned: 200,
            empty: vec![],
            empty_count: 0,
            missing: vec![MissingForTarget {
                selector: "x".to_owned(),
                text: "x".to_owned(),
                for_value: "x".to_owned(),
            }],
            missing_count: 73,
            ambiguous: vec![],
            ambiguous_count: 0,
            wrong_type: vec![],
            wrong_type_count: 0,
        };
        let findings = detect_label_for_resolution_issues(&snap);
        assert!(findings[0].detail.contains("73 <label"));
    }
}
