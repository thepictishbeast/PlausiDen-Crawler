//! `aria_labelledby_resolution` — id-reference resolution check
//! for `aria-labelledby`.
//!
//! Sibling axis to `aria_describedby_resolution` (same shape,
//! description attribute) and to `dialog_label` (which performs
//! a labelledby check scoped to `<dialog>` only). This detector
//! covers `aria-labelledby` on EVERY element on the page.
//!
//! ## Why this is separately critical from describedby
//!
//! `aria-labelledby` defines the **accessible name** of an
//! element — not its description. A dangling labelledby reference
//! collapses the accessible name to the element's other name-
//! computation sources (often nothing for `<div role="…">`
//! widgets), leaving the control unnamed to assistive tech.
//! Result: screen-reader users hear "button" / "tab" / "dialog"
//! with no further information about what the control is for.
//! Strict-severity bug in every case.
//!
//! ## Findings
//!
//! * `aria-labelledby.dangling-ref` strict — NO id-ref in the
//!   attribute resolves to any element on the page. Accessible
//!   name is unrecoverable from this attribute.
//! * `aria-labelledby.empty-attribute` strict — the attribute is
//!   present but its trimmed value is empty.
//! * `aria-labelledby.partial-resolution` strict — multi-id
//!   attribute where SOME ids resolve and OTHERS don't. Per
//!   ARIA name-computation, the unresolved ids contribute the
//!   empty string and the final accessible name is the
//!   concatenation of the resolved ones — usually missing a
//!   meaningful chunk. Stricter than the describedby equivalent
//!   because for labelledby, the name itself is at stake.
//! * `aria-labelledby.target-empty-text` warn — every resolved
//!   target is empty of text content. Accessible name computes
//!   to empty string, which is equivalent to no name at all.
//!
//! Out of scope:
//!
//! * `aria-describedby` — covered by
//!   `aria_describedby_resolution`.
//! * `aria-controls`, `aria-owns`, `aria-flowto` — same IDREF
//!   shape, different semantics; each gets its own dedicated
//!   axis.
//! * Whether the resolved target is `aria-hidden` — for
//!   labelledby, name-computation EXPLICITLY descends into
//!   aria-hidden subtrees, so this is not a bug for labelledby
//!   (it IS for describedby — see that axis).
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

/// One element on the page carrying an `aria-labelledby`
/// attribute, with per-id resolution captured.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaLabelledbyEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Raw attribute value (trimmed). Empty string means the
    /// attribute was present but its value was whitespace-only.
    pub attribute_value: String,
    /// Per-id resolution. Empty when `attribute_value` was empty.
    pub resolutions: Vec<LabelledbyResolution>,
}

/// Resolution result for a single id token inside the attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LabelledbyResolution {
    /// Id token from the attribute (`"hdr-1"`).
    pub id_ref: String,
    /// Whether the page has an element with this id.
    pub resolves: bool,
    /// Whether the resolved element (if any) has non-empty
    /// trimmed text content. Meaningful only when `resolves` is
    /// true.
    pub target_has_text: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaLabelledbyResolutionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-labelledby`.
    pub entries: Vec<AriaLabelledbyEntry>,
}

/// Detector. Buckets findings by defect kind so consumers see at
/// most one finding per kind (with up to MAX_EXAMPLES selectors
/// surfaced in `detail`).
#[must_use]
pub fn detect_aria_labelledby_resolution(
    snap: &AriaLabelledbyResolutionSnapshot,
) -> Vec<AxisFinding> {
    let mut dangling: Vec<String> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    let mut all_targets_empty_text: Vec<&str> = Vec::new();
    let mut empty_attr: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        if entry.attribute_value.is_empty() {
            empty_attr.push(entry.selector.as_str());
            continue;
        }
        let total = entry.resolutions.len();
        let resolved: Vec<&LabelledbyResolution> =
            entry.resolutions.iter().filter(|r| r.resolves).collect();
        let unresolved: Vec<&str> = entry
            .resolutions
            .iter()
            .filter(|r| !r.resolves)
            .map(|r| r.id_ref.as_str())
            .collect();

        if resolved.is_empty() && total > 0 {
            dangling.push(format!(
                "{} (ids={})",
                entry.selector,
                unresolved.join(",")
            ));
            continue;
        }
        if !unresolved.is_empty() {
            partial.push(format!(
                "{} (missing={})",
                entry.selector,
                unresolved.join(",")
            ));
            // No need to also check target-empty-text — partial
            // resolution is the dominant issue here.
            continue;
        }
        // Fully resolved — check that resolved targets contribute
        // any non-empty text.
        let any_text = resolved.iter().any(|r| r.target_has_text);
        if !any_text {
            all_targets_empty_text.push(entry.selector.as_str());
        }
    }

    let mut findings = Vec::new();
    let total_entries = snap.entries.len();

    if !dangling.is_empty() {
        let preview =
            preview_examples(&dangling.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-labelledby.dangling-ref".to_owned(),
            detail: format!(
                "{} of {} aria-labelledby host(s) have NO id-ref that resolves; accessible name is unrecoverable from this attribute. Examples: {}",
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
            kind: "aria-labelledby.empty-attribute".to_owned(),
            detail: format!(
                "{} of {} aria-labelledby host(s) carry an empty / whitespace-only attribute value. Examples: {}",
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
            severity: AxisSeverity::Strict,
            kind: "aria-labelledby.partial-resolution".to_owned(),
            detail: format!(
                "{} of {} aria-labelledby host(s) have SOME ids that resolve and others that don't; the accessible name is missing the unresolved chunk. Examples: {}",
                partial.len(),
                total_entries,
                preview
            ),
        });
    }

    if !all_targets_empty_text.is_empty() {
        let preview = preview_examples(&all_targets_empty_text);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-labelledby.target-empty-text".to_owned(),
            detail: format!(
                "{} of {} aria-labelledby host(s) resolve to target(s) with empty text; accessible name computes to empty string. Examples: {}",
                all_targets_empty_text.len(),
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

/// Page-side eval. Walks every element with `aria-labelledby`,
/// resolves each id token, and reports whether each target has
/// non-empty trimmed text content.
pub const ARIA_LABELLEDBY_RESOLUTION_JS: &str = r##"(() => {
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

    const hosts = Array.from(document.querySelectorAll('[aria-labelledby]'));
    const entries = hosts.map(function(el) {
      const raw = (el.getAttribute('aria-labelledby') || '').trim();
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
          return { idRef: id, resolves: false, targetHasText: false };
        }
        const txt = (target.textContent || '').trim();
        return { idRef: id, resolves: true, targetHasText: txt.length > 0 };
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

    fn res(id: &str, resolves: bool, has_text: bool) -> LabelledbyResolution {
        LabelledbyResolution {
            id_ref: id.to_owned(),
            resolves,
            target_has_text: has_text,
        }
    }

    fn entry(
        sel: &str,
        attr: &str,
        resolutions: Vec<LabelledbyResolution>,
    ) -> AriaLabelledbyEntry {
        AriaLabelledbyEntry {
            selector: sel.to_owned(),
            attribute_value: attr.to_owned(),
            resolutions,
        }
    }

    fn snap(entries: Vec<AriaLabelledbyEntry>) -> AriaLabelledbyResolutionSnapshot {
        AriaLabelledbyResolutionSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_labelledby_resolution(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn fully_resolved_with_text_is_clean() {
        let f = detect_aria_labelledby_resolution(&snap(vec![entry(
            "section.card",
            "card-h",
            vec![res("card-h", true, true)],
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn no_ids_resolve_is_strict_dangling() {
        let f = detect_aria_labelledby_resolution(&snap(vec![entry(
            "div[role=tabpanel]",
            "missing-tab",
            vec![res("missing-tab", false, false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-labelledby.dangling-ref")
            .expect("dangling-ref finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("div[role=tabpanel]"));
        assert!(hit.detail.contains("missing-tab"));
        assert!(hit.detail.contains("unrecoverable"));
    }

    #[test]
    fn partial_resolution_is_strict_unlike_describedby() {
        // Two ids, only one resolves. For aria-labelledby this
        // is strict (the accessible name is incomplete);
        // describedby treats the same shape as warn.
        let f = detect_aria_labelledby_resolution(&snap(vec![entry(
            "button#save",
            "label-1 label-2",
            vec![res("label-1", true, true), res("label-2", false, false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-labelledby.partial-resolution")
            .expect("partial-resolution finding expected");
        assert_eq!(
            hit.severity,
            AxisSeverity::Strict,
            "partial labelledby resolution is strict"
        );
        assert!(hit.detail.contains("button#save"));
        assert!(hit.detail.contains("missing=label-2"));
    }

    #[test]
    fn empty_attribute_is_strict() {
        let f = detect_aria_labelledby_resolution(&snap(vec![entry(
            "section",
            "",
            vec![],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-labelledby.empty-attribute")
            .expect("empty-attribute finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn target_with_empty_text_is_warn() {
        let f = detect_aria_labelledby_resolution(&snap(vec![entry(
            "div[role=dialog]",
            "empty-h",
            vec![res("empty-h", true, false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-labelledby.target-empty-text")
            .expect("target-empty-text finding expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("dialog"));
    }

    #[test]
    fn one_target_with_text_among_many_empties_is_clean() {
        // If at least one resolved target contributes text, the
        // accessible name is non-empty — fine.
        let f = detect_aria_labelledby_resolution(&snap(vec![entry(
            "div[role=dialog]",
            "a b c",
            vec![
                res("a", true, false),
                res("b", true, true),
                res("c", true, false),
            ],
        )]));
        assert!(
            f.is_empty(),
            "at-least-one-with-text should not trigger empty-text: {f:?}"
        );
    }

    #[test]
    fn multiple_dangling_share_bucket() {
        let f = detect_aria_labelledby_resolution(&snap(vec![
            entry("section#a", "x", vec![res("x", false, false)]),
            entry("section#b", "y", vec![res("y", false, false)]),
            entry("section#c", "z", vec![res("z", false, false)]),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-labelledby.dangling-ref")
            .unwrap();
        assert!(hit.detail.contains("3 of 3"));
        assert!(hit.detail.contains("section#a"));
        assert!(hit.detail.contains("section#b"));
        assert!(hit.detail.contains("section#c"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("section#h{i}"),
                    "x",
                    vec![res("x", false, false)],
                )
            })
            .collect();
        let f = detect_aria_labelledby_resolution(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-labelledby.dangling-ref")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_attribute_hosts() {
        assert!(ARIA_LABELLEDBY_RESOLUTION_JS.starts_with("(() => {"));
        assert!(ARIA_LABELLEDBY_RESOLUTION_JS.ends_with(")()"));
        assert!(ARIA_LABELLEDBY_RESOLUTION_JS.contains("aria-labelledby"));
        assert!(ARIA_LABELLEDBY_RESOLUTION_JS.contains("getElementById"));
        assert!(ARIA_LABELLEDBY_RESOLUTION_JS.contains("textContent"));
    }
}
