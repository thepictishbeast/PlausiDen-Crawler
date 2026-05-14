//! `form_labels` — form-control labelling detector.
//!
//! Catches three high-impact form-UX bugs:
//!
//! * `form.no-label`               strict — no accessible name at
//!   all (no `<label>`, aria-label, aria-labelledby, title, or
//!   placeholder). Screen-reader users hear "edit text" with no
//!   hint. WCAG 1.3.1 + 4.1.2 (both A).
//!
//! * `form.placeholder-only-label` warn — placeholder is the ONLY
//!   "label". WCAG 3.3.2: placeholders aren't labels (vanish on
//!   focus, low contrast by default, confuse autofill).
//!
//! * `form.required-no-indicator`  warn — `required` /
//!   `aria-required="true"` set but no visible indicator (`*` or
//!   the word 'required' in the visible label). Sighted users
//!   discover the requirement only on submit failure.
//!
//! Mirror of `src/formLabels.ts` — JS string + structs + finding
//! kinds + severities are byte-equivalent.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on enums.
//! * Pure detector function; no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — captures every visible non-button form
/// control with its computed accessible name + the SOURCE that
/// produced it (label-for | label-wrap | aria-label |
/// aria-labelledby | title | placeholder | none).
pub const FORM_LABELS_JS: &str = r##"(() => {
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
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    const nameAndSource = function(el) {
      const labelledby = el.getAttribute('aria-labelledby');
      if (labelledby) {
        const ids = labelledby.split(/\s+/).filter(Boolean);
        const parts = [];
        for (const id of ids) {
          const ref = document.getElementById(id);
          if (ref) parts.push((ref.textContent || '').trim());
        }
        const joined = parts.join(' ').trim();
        if (joined) return { name: joined, source: 'aria-labelledby' };
      }
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return { name: aria.trim(), source: 'aria-label' };
      if (el.id) {
        const lab = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
        if (lab) {
          const t = (lab.textContent || '').trim();
          if (t) return { name: t, source: 'label-for' };
        }
      }
      let parent = el.parentElement;
      let hops = 0;
      while (parent && hops < 4) {
        if (parent.tagName === 'LABEL') {
          const t = (parent.textContent || '').trim();
          if (t) return { name: t, source: 'label-wrap' };
          break;
        }
        parent = parent.parentElement;
        hops += 1;
      }
      const title = el.getAttribute('title');
      if (title && title.trim()) return { name: title.trim(), source: 'title' };
      const placeholder = el.getAttribute('placeholder');
      if (placeholder && placeholder.trim()) {
        return { name: placeholder.trim(), source: 'placeholder' };
      }
      return { name: '', source: 'none' };
    };

    const out = [];
    const els = document.querySelectorAll('input,textarea,select');
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      if (!isVisible(el)) continue;
      const tag = el.tagName.toLowerCase();
      const type = (el.getAttribute('type') || '').toLowerCase();
      if (tag === 'input') {
        const skip = ['hidden', 'submit', 'reset', 'button', 'image'];
        if (skip.indexOf(type) >= 0) continue;
      }
      const ns = nameAndSource(el);
      const required = el.hasAttribute('required') ||
                       el.getAttribute('aria-required') === 'true';
      let requiredIndicated = false;
      if (required && ns.name) {
        const lower = ns.name.toLowerCase();
        if (ns.name.indexOf('*') >= 0 || lower.indexOf('required') >= 0) {
          requiredIndicated = true;
        }
      }
      out.push({
        selector: selectorOf(el),
        tag: tag,
        type: type,
        accessibleName: ns.name.slice(0, 120),
        nameSource: ns.source,
        placeholder: (el.getAttribute('placeholder') || '').slice(0, 80),
        required: required,
        requiredIndicated: requiredIndicated,
      });
    }
    return { pageUrl: window.location.href, controls: out };
})()"##;

/// One captured form control (input / textarea / select) that's
/// not a non-labelled type (button / submit / reset / image / hidden).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedFormControl {
    /// Best-effort CSS selector.
    pub selector: String,
    /// Lowercased tag (`input` / `textarea` / `select`).
    pub tag: String,
    /// `type` for inputs, empty string otherwise.
    pub r#type: String,
    /// Computed accessible name (first 120 chars).
    pub accessible_name: String,
    /// What produced the name. One of:
    /// `aria-labelledby` | `aria-label` | `label-for` |
    /// `label-wrap` | `title` | `placeholder` | `none`.
    pub name_source: String,
    /// `placeholder` attribute value (first 80 chars).
    pub placeholder: String,
    /// `required` attribute or `aria-required="true"`.
    pub required: bool,
    /// True iff the visible label text contains `*` or the
    /// word "required" (case-insensitive).
    pub required_indicated: bool,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FormLabelsSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// Captured user-fillable form controls.
    pub controls: Vec<CapturedFormControl>,
}

/// Apply detection rules. Pure function. Mirrors
/// `detectFormLabelIssues` in `src/formLabels.ts`.
#[must_use]
pub fn detect_form_label_issues(snap: &FormLabelsSnapshot) -> Vec<crate::AxisFinding> {
    let non_labelled =
        ["hidden", "submit", "reset", "button", "image"];

    let mut no_label = Vec::<&CapturedFormControl>::new();
    let mut placeholder_only = Vec::<&CapturedFormControl>::new();
    let mut required_no_indicator = Vec::<&CapturedFormControl>::new();

    for c in &snap.controls {
        if c.tag == "input" && non_labelled.contains(&c.r#type.as_str()) {
            continue;
        }
        if c.name_source == "none" || c.accessible_name.is_empty() {
            no_label.push(c);
            continue;
        }
        if c.name_source == "placeholder" {
            placeholder_only.push(c);
        }
        if c.required && !c.required_indicated {
            required_no_indicator.push(c);
        }
    }

    let mut out = Vec::<crate::AxisFinding>::new();
    let render_target = |c: &&CapturedFormControl| -> String {
        let t = if c.tag == "input" {
            let ty = if c.r#type.is_empty() { "text" } else { c.r#type.as_str() };
            format!("{}[type={ty}]", c.tag)
        } else {
            c.tag.clone()
        };
        format!("{} {t}", c.selector)
    };

    if !no_label.is_empty() {
        let examples: Vec<String> = no_label.iter().take(5).map(render_target).collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "form.no-label".to_owned(),
            detail: format!(
                "{} form control(s) have no accessible name (no <label>, aria-label, aria-labelledby, title, or placeholder). WCAG 1.3.1 + 4.1.2 (both A) — screen-reader users hear 'edit text' with no hint. Examples: {}",
                no_label.len(),
                examples.join("; ")
            ),
        });
    }

    if !placeholder_only.is_empty() {
        let examples: Vec<String> = placeholder_only
            .iter()
            .take(5)
            .map(|c| {
                let t = if c.tag == "input" {
                    let ty = if c.r#type.is_empty() { "text" } else { c.r#type.as_str() };
                    format!("{}[type={ty}]", c.tag)
                } else {
                    c.tag.clone()
                };
                format!("{} {t} (placeholder='{}')", c.selector, c.placeholder)
            })
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "form.placeholder-only-label".to_owned(),
            detail: format!(
                "{} form control(s) use a placeholder as the ONLY label. WCAG 3.3.2: placeholders are not labels — they vanish on focus, default to low contrast, and confuse autofill. Add a <label> or aria-label. Examples: {}",
                placeholder_only.len(),
                examples.join("; ")
            ),
        });
    }

    if !required_no_indicator.is_empty() {
        let examples: Vec<String> = required_no_indicator
            .iter()
            .take(5)
            .map(|c| {
                let t = if c.tag == "input" {
                    let ty = if c.r#type.is_empty() { "text" } else { c.r#type.as_str() };
                    format!("{}[type={ty}]", c.tag)
                } else {
                    c.tag.clone()
                };
                format!("{} {t} (label='{}')", c.selector, c.accessible_name)
            })
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "form.required-no-indicator".to_owned(),
            detail: format!(
                "{} required field(s) have no visible required indicator (no '*' or 'required' in the label text). Sighted users discover the requirement only on submission failure. WCAG 3.3.2 + UX best practice. Examples: {}",
                required_no_indicator.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(
        selector: &str,
        tag: &str,
        ty: &str,
        name: &str,
        source: &str,
        required: bool,
        required_indicated: bool,
    ) -> CapturedFormControl {
        CapturedFormControl {
            selector: selector.to_owned(),
            tag: tag.to_owned(),
            r#type: ty.to_owned(),
            accessible_name: name.to_owned(),
            name_source: source.to_owned(),
            placeholder: String::new(),
            required,
            required_indicated,
        }
    }

    fn snap(controls: Vec<CapturedFormControl>) -> FormLabelsSnapshot {
        FormLabelsSnapshot {
            page_url: "http://t/".to_owned(),
            controls,
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(
            FORM_LABELS_JS.matches('(').count(),
            FORM_LABELS_JS.matches(')').count()
        );
        assert_eq!(
            FORM_LABELS_JS.matches('{').count(),
            FORM_LABELS_JS.matches('}').count()
        );
    }

    #[test]
    fn clean_labelled_no_findings() {
        let s = snap(vec![ctrl("a", "input", "text", "Email", "label-for", false, false)]);
        assert!(detect_form_label_issues(&s).is_empty());
    }

    #[test]
    fn no_source_strict_no_label() {
        let s = snap(vec![ctrl("a", "input", "text", "", "none", false, false)]);
        assert!(detect_form_label_issues(&s)
            .iter()
            .any(|f| f.kind == "form.no-label" && f.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn placeholder_only_warn_not_no_label() {
        let s = snap(vec![ctrl("a", "input", "text", "Your email", "placeholder", false, false)]);
        let f = detect_form_label_issues(&s);
        assert!(f.iter().any(|x| x.kind == "form.placeholder-only-label"));
        assert!(!f.iter().any(|x| x.kind == "form.no-label"));
    }

    #[test]
    fn required_no_indicator_warn() {
        let s = snap(vec![ctrl("a", "input", "text", "Email", "label-for", true, false)]);
        assert!(detect_form_label_issues(&s)
            .iter()
            .any(|f| f.kind == "form.required-no-indicator"
                && f.severity == crate::AxisSeverity::Warn));
    }

    #[test]
    fn required_with_star_passes() {
        let s = snap(vec![ctrl("a", "input", "text", "Email *", "label-for", true, true)]);
        assert!(!detect_form_label_issues(&s)
            .iter()
            .any(|f| f.kind == "form.required-no-indicator"));
    }

    #[test]
    fn submit_button_exempt() {
        let s = snap(vec![ctrl("a", "input", "submit", "", "none", false, false)]);
        assert!(detect_form_label_issues(&s).is_empty());
    }

    #[test]
    fn hidden_input_exempt() {
        let s = snap(vec![ctrl("a", "input", "hidden", "", "none", false, false)]);
        assert!(detect_form_label_issues(&s).is_empty());
    }

    #[test]
    fn mixed_snapshot_three_findings() {
        let s = snap(vec![
            ctrl("a", "input", "text", "", "none", false, false),
            ctrl("b", "input", "text", "X", "placeholder", false, false),
            ctrl("c", "input", "text", "Y", "label-for", true, false),
        ]);
        let f = detect_form_label_issues(&s);
        assert_eq!(f.len(), 3, "{:?}", f);
    }

    #[test]
    fn examples_capped_at_five() {
        let mut controls = Vec::new();
        for i in 0..10 {
            controls.push(ctrl(
                &format!("a:nth-of-type({i})"),
                "input", "text", "",
                "none", false, false,
            ));
        }
        let s = snap(controls);
        let f = detect_form_label_issues(&s);
        let no_label = f.iter().find(|x| x.kind == "form.no-label").expect("no-label");
        // 5 examples → 4 separators.
        assert_eq!(no_label.detail.matches("; ").count(), 4);
    }
}
