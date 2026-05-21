//! `aria_describedby_resolution` — id-reference resolution check
//! for `aria-describedby`.
//!
//! Sibling axis to `dialog_label` (which checks accessible names
//! on `<dialog>` and resolves `aria-labelledby` references on
//! those dialogs only) and to `aria_required_attrs` (which checks
//! that required ARIA attributes for a role are present). This
//! detector targets a different, broader bug class: any element
//! anywhere on the page that carries `aria-describedby="…"`
//! pointing at one or more ids — do those ids actually resolve?
//!
//! ## The bug class
//!
//! `aria-describedby` is the ARIA mechanism for attaching
//! descriptive text to a control without rendering it inline.
//! Common bugs:
//!
//! 1. The target element was renamed or removed in a refactor,
//!    leaving a dangling id reference. Screen readers announce
//!    the control's name but no description.
//! 2. Multiple ids in the attribute (`"err1 hint1"`) but only one
//!    resolves. Screen readers fall back to whatever does
//!    resolve; the other half is silently lost.
//! 3. The target exists but is `aria-hidden="true"` or inside an
//!    `aria-hidden` subtree. Per ARIA spec, name + description
//!    computation walks `aria-hidden` subtrees, so a description
//!    element marked `aria-hidden` should NOT be referenced —
//!    the accessibility tree contains the text but it's confusing
//!    for the AT to manage.
//! 4. Empty `aria-describedby=""` — explicit attribute presence
//!    with no id token defeats the description-name computation;
//!    semantically equivalent to absence, but worth surfacing
//!    because it's almost certainly an editor or templating bug.
//!
//! ## Findings
//!
//! * `aria-describedby.dangling-ref` strict — at least one id
//!   token in the attribute does not resolve to any element on
//!   the page.
//! * `aria-describedby.partial-resolution` warn — multi-id
//!   attribute where SOME ids resolve and OTHERS don't.
//! * `aria-describedby.target-aria-hidden` warn — the resolved
//!   target sits inside an `aria-hidden="true"` ancestor (or is
//!   itself hidden). Description text is in DOM but AT may
//!   treat it inconsistently.
//! * `aria-describedby.empty-attribute` strict — the attribute is
//!   present but its trimmed value is empty.
//!
//! Out of scope:
//!
//! * `aria-labelledby` — same shape, different attribute. Future
//!   `aria_labelledby_resolution` axis can mirror this code; the
//!   dialog-scoped check already exists in `dialog_label`.
//! * `aria-controls`, `aria-owns`, `aria-flowto` — all use the
//!   same IDREF shape but have different semantics; each gets
//!   its own dedicated axis.
//! * `aria-describedby` on inputs handled by `form_labels` — the
//!   labels axis confirms an accessible name; this detector
//!   confirms description resolution which is a separate
//!   contract.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const MAX_EXAMPLES: usize = 5;

/// One element on the page carrying an `aria-describedby`
/// attribute, with per-id resolution captured.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaDescribedbyEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Raw attribute value (trimmed). Empty string means the
    /// attribute was present but its value was whitespace-only.
    pub attribute_value: String,
    /// Per-id resolution. Empty when `attribute_value` was empty.
    pub resolutions: Vec<DescribedbyResolution>,
}

/// Resolution result for a single id token inside the attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DescribedbyResolution {
    /// Id token from the attribute (`"err1"`).
    pub id_ref: String,
    /// Whether the page has an element with this id.
    pub resolves: bool,
    /// Whether the resolved element (if any) is inside an
    /// `aria-hidden="true"` ancestor or is itself `aria-hidden=
    /// "true"`. Meaningful only when `resolves` is true.
    pub target_aria_hidden: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaDescribedbyResolutionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-describedby`.
    pub entries: Vec<AriaDescribedbyEntry>,
}

/// Detector. Buckets findings by defect kind so consumers see at
/// most one finding per kind (with up to MAX_EXAMPLES selectors
/// surfaced in `detail`).
#[must_use]
pub fn detect_aria_describedby_resolution(
    snap: &AriaDescribedbyResolutionSnapshot,
) -> Vec<AxisFinding> {
    let mut dangling: Vec<String> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    let mut target_hidden: Vec<String> = Vec::new();
    let mut empty_attr: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        if entry.attribute_value.is_empty() {
            empty_attr.push(entry.selector.as_str());
            continue;
        }
        let total = entry.resolutions.len();
        let resolved_count = entry.resolutions.iter().filter(|r| r.resolves).count();
        let unresolved: Vec<&str> = entry
            .resolutions
            .iter()
            .filter(|r| !r.resolves)
            .map(|r| r.id_ref.as_str())
            .collect();

        if resolved_count == 0 && total > 0 {
            dangling.push(format!(
                "{} (ids={})",
                entry.selector,
                unresolved.join(",")
            ));
        } else if !unresolved.is_empty() {
            partial.push(format!(
                "{} (missing={})",
                entry.selector,
                unresolved.join(",")
            ));
        }

        let hidden_ids: Vec<&str> = entry
            .resolutions
            .iter()
            .filter(|r| r.resolves && r.target_aria_hidden)
            .map(|r| r.id_ref.as_str())
            .collect();
        if !hidden_ids.is_empty() {
            target_hidden.push(format!(
                "{} (hidden ids={})",
                entry.selector,
                hidden_ids.join(",")
            ));
        }
    }

    let mut findings = Vec::new();
    let total_entries = snap.entries.len();

    if !dangling.is_empty() {
        let preview =
            preview_examples(&dangling.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-describedby.dangling-ref".to_owned(),
            detail: format!(
                "{} of {} aria-describedby host(s) have NO id-ref that resolves. Examples: {}",
                dangling.len(),
                total_entries,
                preview
            ),
        });
    }

    if !empty_attr.is_empty() {
        let preview = preview_examples(&empty_attr);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-describedby.empty-attribute".to_owned(),
            detail: format!(
                "{} of {} aria-describedby host(s) carry an empty / whitespace-only attribute value. Examples: {}",
                empty_attr.len(),
                total_entries,
                preview
            ),
        });
    }

    if !partial.is_empty() {
        let preview =
            preview_examples(&partial.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-describedby.partial-resolution".to_owned(),
            detail: format!(
                "{} of {} aria-describedby host(s) have SOME ids that resolve and others that don't. Examples: {}",
                partial.len(),
                total_entries,
                preview
            ),
        });
    }

    if !target_hidden.is_empty() {
        let preview = preview_examples(
            &target_hidden.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-describedby.target-aria-hidden".to_owned(),
            detail: format!(
                "{} of {} aria-describedby host(s) point at target(s) inside aria-hidden subtrees. Examples: {}",
                target_hidden.len(),
                total_entries,
                preview
            ),
        });
    }

    findings
}

fn preview_examples(examples: &[&str]) -> String {
    let mut buf = String::new();
    let n = examples.len().min(MAX_EXAMPLES);
    for (i, sel) in examples.iter().take(n).enumerate() {
        if i > 0 {
            buf.push_str(" | ");
        }
        buf.push_str(sel);
    }
    if examples.len() > MAX_EXAMPLES {
        buf.push_str(&format!(" (+{} more)", examples.len() - MAX_EXAMPLES));
    }
    buf
}

/// Page-side eval. Walks every element with `aria-describedby`,
/// resolves each id token, and reports whether each target is
/// inside an `aria-hidden="true"` ancestor.
pub const ARIA_DESCRIBEDBY_RESOLUTION_JS: &str = r##"(() => {
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
      return parts.join(' > ') || 'body';
    };

    const hasAriaHiddenAncestor = function(el) {
      let node = el;
      while (node && node !== document.documentElement) {
        if (node.getAttribute && node.getAttribute('aria-hidden') === 'true') return true;
        node = node.parentElement;
      }
      return false;
    };

    const hosts = Array.from(document.querySelectorAll('[aria-describedby]'));
    const entries = hosts.map(function(el) {
      const raw = (el.getAttribute('aria-describedby') || '').trim();
      if (!raw) {
        return {
          selector: selectorOf(el),
          attributeValue: '',
          resolutions: []
        };
      }
      const ids = raw.split(/\s+/).filter(function(s) { return s.length > 0; });
      const resolutions = ids.map(function(id) {
        const target = document.getElementById(id);
        if (!target) {
          return { idRef: id, resolves: false, targetAriaHidden: false };
        }
        return {
          idRef: id,
          resolves: true,
          targetAriaHidden: hasAriaHiddenAncestor(target)
        };
      });
      return {
        selector: selectorOf(el),
        attributeValue: raw,
        resolutions: resolutions
      };
    });

    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn res(id: &str, resolves: bool, hidden: bool) -> DescribedbyResolution {
        DescribedbyResolution {
            id_ref: id.to_owned(),
            resolves,
            target_aria_hidden: hidden,
        }
    }

    fn entry(
        sel: &str,
        attr: &str,
        resolutions: Vec<DescribedbyResolution>,
    ) -> AriaDescribedbyEntry {
        AriaDescribedbyEntry {
            selector: sel.to_owned(),
            attribute_value: attr.to_owned(),
            resolutions,
        }
    }

    fn snap(entries: Vec<AriaDescribedbyEntry>) -> AriaDescribedbyResolutionSnapshot {
        AriaDescribedbyResolutionSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_describedby_resolution(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn fully_resolved_single_id_is_clean() {
        let f = detect_aria_describedby_resolution(&snap(vec![entry(
            "input#email",
            "email-hint",
            vec![res("email-hint", true, false)],
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn no_ids_resolve_is_strict_dangling() {
        let f = detect_aria_describedby_resolution(&snap(vec![entry(
            "input#password",
            "missing-hint",
            vec![res("missing-hint", false, false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-describedby.dangling-ref")
            .expect("dangling-ref finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("input#password"));
        assert!(hit.detail.contains("missing-hint"));
        // Partial-resolution NOT emitted (zero resolved).
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-describedby.partial-resolution"));
    }

    #[test]
    fn partial_resolution_is_warn_not_strict() {
        // Two ids, one resolves and one doesn't.
        let f = detect_aria_describedby_resolution(&snap(vec![entry(
            "input#username",
            "hint1 hint2",
            vec![res("hint1", true, false), res("hint2", false, false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-describedby.partial-resolution")
            .expect("partial-resolution finding expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("input#username"));
        assert!(hit.detail.contains("missing=hint2"));
        // Dangling NOT emitted (one id resolves).
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-describedby.dangling-ref"));
    }

    #[test]
    fn empty_attribute_is_strict() {
        let f = detect_aria_describedby_resolution(&snap(vec![entry(
            "input#email",
            "",
            vec![],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-describedby.empty-attribute")
            .expect("empty-attribute finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("input#email"));
    }

    #[test]
    fn target_aria_hidden_is_warn() {
        let f = detect_aria_describedby_resolution(&snap(vec![entry(
            "input#email",
            "hidden-hint",
            vec![res("hidden-hint", true, true)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-describedby.target-aria-hidden")
            .expect("target-aria-hidden finding expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("hidden-hint"));
        // No dangling / partial since resolution succeeded.
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-describedby.dangling-ref"));
        assert!(!f
            .iter()
            .any(|x| x.kind == "aria-describedby.partial-resolution"));
    }

    #[test]
    fn multiple_dangling_share_bucket() {
        let f = detect_aria_describedby_resolution(&snap(vec![
            entry("input#a", "x", vec![res("x", false, false)]),
            entry("input#b", "y", vec![res("y", false, false)]),
            entry("input#c", "z", vec![res("z", false, false)]),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-describedby.dangling-ref")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
        assert!(hit.detail.contains("input#a"));
        assert!(hit.detail.contains("input#b"));
        assert!(hit.detail.contains("input#c"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("input#h{i}"),
                    "x",
                    vec![res("x", false, false)],
                )
            })
            .collect();
        let f = detect_aria_describedby_resolution(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-describedby.dangling-ref")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
        for i in 0..5 {
            assert!(hit.detail.contains(&format!("input#h{i}")));
        }
        for i in 5..8 {
            assert!(
                !hit.detail.contains(&format!("input#h{i}")),
                "h{i} should be hidden"
            );
        }
    }

    #[test]
    fn mixed_kinds_each_emit_their_own_finding() {
        let f = detect_aria_describedby_resolution(&snap(vec![
            // dangling
            entry("input#a", "x", vec![res("x", false, false)]),
            // partial
            entry(
                "input#b",
                "y z",
                vec![res("y", true, false), res("z", false, false)],
            ),
            // empty
            entry("input#c", "", vec![]),
            // target-hidden
            entry("input#d", "h", vec![res("h", true, true)]),
        ]));
        let kinds: Vec<&str> = f.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"aria-describedby.dangling-ref"));
        assert!(kinds.contains(&"aria-describedby.empty-attribute"));
        assert!(kinds.contains(&"aria-describedby.partial-resolution"));
        assert!(kinds.contains(&"aria-describedby.target-aria-hidden"));
        assert_eq!(f.len(), 4);
    }

    #[test]
    fn js_const_is_iife_and_walks_attribute_hosts() {
        assert!(ARIA_DESCRIBEDBY_RESOLUTION_JS.starts_with("(() => {"));
        assert!(ARIA_DESCRIBEDBY_RESOLUTION_JS.ends_with(")()"));
        assert!(ARIA_DESCRIBEDBY_RESOLUTION_JS.contains("aria-describedby"));
        assert!(ARIA_DESCRIBEDBY_RESOLUTION_JS.contains("getElementById"));
        assert!(ARIA_DESCRIBEDBY_RESOLUTION_JS.contains("aria-hidden"));
    }
}
