//! `dialog_label` — `<dialog>` accessible-name detector.
//!
//! Forward step on rolling task #117 (Crawler axes). Pairs with
//! `iframe_title`, `aria_required_attrs`, `form_labels` — all of
//! which are accessible-name detectors for specific element kinds.
//!
//! ## The bug class
//!
//! `<dialog>` is a browser-native modal element + an interactive
//! landmark to assistive technologies. When a screen reader hits
//! a dialog it MUST announce a name — without one, the user just
//! hears "dialog" with no information about what the dialog is
//! for. ARIA-Authoring Practices specifies dialogs need an
//! accessible name via `aria-label`, `aria-labelledby`, or (less
//! ideally) the first heading/title element inside the dialog
//! body itself if browsers resolve it through name-computation.
//!
//! Practical bug: operators ship `<dialog>` with no aria-label,
//! no aria-labelledby, no `<h2>` inside, and screen-reader users
//! land on an unnamed modal. This is a WCAG 2.4.6 (Headings and
//! Labels) failure.
//!
//! ## Findings
//!
//! * `dialog-label.missing-name` strict — `<dialog>` with NO
//!   aria-label AND NO aria-labelledby AND NO heading (h1-h6)
//!   inside its first 200 chars of text content.
//! * `dialog-label.empty-aria-label` strict — `<dialog aria-label="">`
//!   — explicit empty string defeats the accessible-name
//!   computation; treat as missing-name.
//! * `dialog-label.dangling-labelledby` strict — `<dialog aria-
//!   labelledby="x">` where no element with `id="x"` exists on
//!   the page. The reference resolves to nothing; effectively no
//!   accessible name.
//!
//! Out of scope:
//!
//! * `<div role="dialog">` polyfilled dialogs — same accessibility
//!   needs but the JS-driven open/close lifecycle differs; the
//!   `aria_required_attrs` detector covers the `role="dialog"`
//!   shape with overlapping but not identical heuristics.
//! * `<dialog open>` vs `<dialog>` (closed) — accessible-name
//!   check applies to both; renderer-time visibility doesn't
//!   change the contract.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured `<dialog>` with its accessible-name signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DialogLabelEntry {
    /// CSS-ish selector pointing at the dialog.
    pub selector: String,
    /// aria-label attribute value if present (None = attribute absent).
    pub aria_label: Option<String>,
    /// aria-labelledby attribute value if present (the id reference;
    /// may include multiple space-separated ids per ARIA spec).
    pub aria_labelledby: Option<String>,
    /// Whether the dialog contains a heading element (h1-h6) within
    /// its first ~200 chars of content — fallback accessible-name
    /// source under browser name-computation.
    pub has_heading_in_content: bool,
    /// For each space-separated id in `aria_labelledby`, whether
    /// the page has a matching `id="..."` element. Empty Vec when
    /// `aria_labelledby` is None.
    pub labelledby_resolutions: Vec<LabelledByResolution>,
}

/// One id reference inside aria-labelledby + whether it resolves.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LabelledByResolution {
    /// The id token from the aria-labelledby attribute.
    pub id_ref: String,
    /// Whether the page has an element with this id.
    pub resolves: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DialogLabelSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<dialog>` on the page.
    pub entries: Vec<DialogLabelEntry>,
}

/// Page-side eval. Walks every `<dialog>` element, captures its
/// accessible-name signals + resolves aria-labelledby id refs.
pub const DIALOG_LABEL_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };
    const entries = [];
    const dialogs = document.querySelectorAll('dialog');
    for (let i = 0; i < dialogs.length; i++) {
      const el = dialogs[i];
      const ariaLabel = el.getAttribute('aria-label');
      const ariaLabelledby = el.getAttribute('aria-labelledby');
      const hasHeading = !!el.querySelector('h1, h2, h3, h4, h5, h6');
      let labelledbyResolutions = [];
      if (ariaLabelledby !== null) {
        const ids = ariaLabelledby.trim().split(/\s+/).filter(function(s) { return s.length > 0; });
        for (let j = 0; j < ids.length; j++) {
          const idRef = ids[j];
          const resolves = !!document.getElementById(idRef);
          labelledbyResolutions.push({ idRef: idRef, resolves: resolves });
        }
      }
      entries.push({
        selector: selectorOf(el),
        ariaLabel: ariaLabel,
        ariaLabelledby: ariaLabelledby,
        hasHeadingInContent: hasHeading,
        labelledbyResolutions: labelledbyResolutions
      });
    }
    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_dialog_label_issues(snap: &DialogLabelSnapshot) -> Vec<AxisFinding> {
    let mut findings = Vec::new();
    for entry in &snap.entries {
        // Empty-string aria-label fires first (explicit defeat).
        if let Some(label) = entry.aria_label.as_deref() {
            if label.trim().is_empty() {
                findings.push(AxisFinding {
                    severity: AxisSeverity::Strict,
                    kind: "dialog-label.empty-aria-label".to_owned(),
                    detail: format!(
                        "{} — `<dialog aria-label=\"\">` (empty string defeats accessible-name computation). Supply a non-empty label or remove the attribute and rely on aria-labelledby / inner heading.",
                        entry.selector
                    ),
                });
                continue;
            }
        }
        // aria-labelledby with dangling refs (none of the ids resolve).
        if let Some(refs) = entry.aria_labelledby.as_deref() {
            let trimmed = refs.trim();
            if !trimmed.is_empty() {
                let any_resolves = entry.labelledby_resolutions.iter().any(|r| r.resolves);
                let any_unresolved = entry.labelledby_resolutions.iter().any(|r| !r.resolves);
                if !any_resolves {
                    findings.push(AxisFinding {
                        severity: AxisSeverity::Strict,
                        kind: "dialog-label.dangling-labelledby".to_owned(),
                        detail: format!(
                            "{} — `<dialog aria-labelledby=\"{trimmed}\">` but no matching element id exists on the page. Reference resolves to nothing; accessible name is empty.",
                            entry.selector
                        ),
                    });
                    continue;
                }
                if any_unresolved {
                    // Partial resolve — at least one id matches, at
                    // least one doesn't. The match still gives a name
                    // (so don't fire strict), but warn the operator.
                    let dangling: Vec<&str> = entry
                        .labelledby_resolutions
                        .iter()
                        .filter(|r| !r.resolves)
                        .map(|r| r.id_ref.as_str())
                        .collect();
                    findings.push(AxisFinding {
                        severity: AxisSeverity::Warn,
                        kind: "dialog-label.partially-dangling-labelledby".to_owned(),
                        detail: format!(
                            "{} — `<dialog aria-labelledby>` references some ids that don't exist: [{}]. Computed name still works via the resolved refs, but the dangling ones are dead and should be fixed.",
                            entry.selector,
                            dangling.join(", ")
                        ),
                    });
                }
                // If labelledby has at least one resolving ref, accessible name
                // is computed — no missing-name finding.
                continue;
            }
        }
        // Has aria-label with non-empty value → fine.
        if entry.aria_label.is_some() {
            continue;
        }
        // No aria-label, no aria-labelledby. Last fallback: heading inside.
        if entry.has_heading_in_content {
            continue;
        }
        // Nothing — strictly missing name.
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "dialog-label.missing-name".to_owned(),
            detail: format!(
                "{} — `<dialog>` has no aria-label, no aria-labelledby, and no heading inside its content. Screen-reader users hear \"dialog\" with no context. Add aria-label=\"...\" or wrap an `<h2>` inside the dialog body.",
                entry.selector
            ),
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        aria_label: Option<&str>,
        aria_labelledby: Option<&str>,
        has_heading: bool,
        resolutions: Vec<LabelledByResolution>,
    ) -> DialogLabelEntry {
        DialogLabelEntry {
            selector: "body > dialog".to_owned(),
            aria_label: aria_label.map(str::to_owned),
            aria_labelledby: aria_labelledby.map(str::to_owned),
            has_heading_in_content: has_heading,
            labelledby_resolutions: resolutions,
        }
    }

    fn snap(entries: Vec<DialogLabelEntry>) -> DialogLabelSnapshot {
        DialogLabelSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    fn lb(id_ref: &str, resolves: bool) -> LabelledByResolution {
        LabelledByResolution {
            id_ref: id_ref.to_owned(),
            resolves,
        }
    }

    #[test]
    fn empty_entries_no_findings() {
        assert!(detect_dialog_label_issues(&snap(Vec::new())).is_empty());
    }

    #[test]
    fn dialog_with_aria_label_is_fine() {
        let findings = detect_dialog_label_issues(&snap(vec![entry(
            Some("Confirm deletion"),
            None,
            false,
            Vec::new(),
        )]));
        assert!(findings.is_empty());
    }

    #[test]
    fn dialog_with_resolving_labelledby_is_fine() {
        let findings = detect_dialog_label_issues(&snap(vec![entry(
            None,
            Some("dialog-title"),
            false,
            vec![lb("dialog-title", true)],
        )]));
        assert!(findings.is_empty());
    }

    #[test]
    fn dialog_with_heading_inside_is_fine() {
        let findings = detect_dialog_label_issues(&snap(vec![entry(
            None,
            None,
            true, // has h2 inside
            Vec::new(),
        )]));
        assert!(findings.is_empty());
    }

    #[test]
    fn dialog_with_no_name_signals_is_strict_missing() {
        let findings =
            detect_dialog_label_issues(&snap(vec![entry(None, None, false, Vec::new())]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "dialog-label.missing-name");
    }

    #[test]
    fn dialog_with_empty_aria_label_is_strict_empty() {
        let findings = detect_dialog_label_issues(&snap(vec![entry(
            Some(""),
            None,
            true, // heading present but empty aria-label still defeats
            Vec::new(),
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "dialog-label.empty-aria-label");
    }

    #[test]
    fn dialog_with_whitespace_aria_label_is_strict_empty() {
        let findings =
            detect_dialog_label_issues(&snap(vec![entry(Some("   "), None, false, Vec::new())]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "dialog-label.empty-aria-label");
    }

    #[test]
    fn dialog_with_dangling_labelledby_is_strict() {
        let findings = detect_dialog_label_issues(&snap(vec![entry(
            None,
            Some("nonexistent-id"),
            false,
            vec![lb("nonexistent-id", false)],
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "dialog-label.dangling-labelledby");
    }

    #[test]
    fn dialog_with_partial_labelledby_dangle_is_warn() {
        // One ref resolves, one doesn't — accessible name computed
        // from the resolving ref, but warn about the dead one.
        let findings = detect_dialog_label_issues(&snap(vec![entry(
            None,
            Some("good bad"),
            false,
            vec![lb("good", true), lb("bad", false)],
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(
            findings[0].kind,
            "dialog-label.partially-dangling-labelledby"
        );
        assert!(findings[0].detail.contains("bad"));
    }

    #[test]
    fn multiple_dialogs_emit_one_finding_each() {
        let findings = detect_dialog_label_issues(&snap(vec![
            entry(None, None, false, Vec::new()),
            entry(Some(""), None, false, Vec::new()),
            entry(Some("Fine dialog"), None, false, Vec::new()),
        ]));
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].kind, "dialog-label.missing-name");
        assert_eq!(findings[1].kind, "dialog-label.empty-aria-label");
    }

    #[test]
    fn snapshot_serde_camel_case() {
        let s = snap(vec![entry(
            Some("ok"),
            Some("ref-id"),
            true,
            vec![lb("ref-id", true)],
        )]);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pageUrl\""));
        assert!(j.contains("\"ariaLabel\""));
        assert!(j.contains("\"ariaLabelledby\""));
        assert!(j.contains("\"hasHeadingInContent\""));
        assert!(j.contains("\"labelledbyResolutions\""));
        assert!(j.contains("\"idRef\""));
        let back: DialogLabelSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn js_eval_const_walks_dialogs_and_resolves_id_refs() {
        assert!(DIALOG_LABEL_JS.contains("querySelectorAll('dialog')"));
        assert!(DIALOG_LABEL_JS.contains("aria-label"));
        assert!(DIALOG_LABEL_JS.contains("aria-labelledby"));
        assert!(DIALOG_LABEL_JS.contains("h1, h2, h3, h4, h5, h6"));
        assert!(DIALOG_LABEL_JS.contains("document.getElementById"));
    }
}
