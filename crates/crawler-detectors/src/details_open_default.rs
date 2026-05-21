//! `details_open_default` — `<details>` element configuration
//! audit.
//!
//! Sibling axis to `dialog_label` (which audits `<dialog>`
//! accessible names) and to `interactive_nesting` (which audits
//! interactive elements inside `<button>` / `<a>`). This
//! detector covers a different bug class on the
//! `<details>` / `<summary>` progressive-disclosure pair.
//!
//! ## The bug class
//!
//! `<details>` is the HTML native disclosure widget — a section
//! whose contents collapse / expand on user activation. Common
//! authoring failures:
//!
//! 1. **Missing `<summary>`** — `<details>` without a `<summary>`
//!    child renders with the browser-default "Details" label,
//!    which is meaningless to AT users.
//! 2. **Empty / whitespace-only `<summary>`** — same effect; the
//!    expand/collapse control has no accessible name.
//! 3. **Every `<details>` defaults to `open`** — operators
//!    sometimes set `open` on every disclosure thinking it's a
//!    UX improvement; defeats progressive disclosure (the user
//!    chose `<details>` over `<section>` to get the collapse
//!    behaviour). Surface as warn — sometimes intentional, but
//!    the all-open pattern usually indicates copy-paste or a
//!    misunderstanding.
//! 4. **`<summary>` not the first child of `<details>`** — per
//!    HTML spec, `<summary>` MUST be the first child. Other
//!    positions are ignored / browser-rewritten and AT
//!    interpretation varies.
//!
//! ## Findings
//!
//! * `details.missing-summary` strict — `<details>` with no
//!   `<summary>` child.
//! * `details.empty-summary` strict — `<details>` whose
//!   `<summary>` has empty / whitespace-only trimmed text.
//! * `details.summary-not-first-child` strict — `<details>`
//!   where the first element child is something other than
//!   `<summary>`.
//! * `details.all-open-by-default` warn — page has multiple
//!   `<details>` (>= 3) and ALL of them carry the `open`
//!   attribute. Almost certainly defeats progressive
//!   disclosure intent.
//!
//! Out of scope:
//!
//! * Polyfilled disclosures (`<button aria-expanded>` +
//!   `<div>`) — covered by future
//!   `aria_expanded_state_consistency` axis.
//! * Whether the `<details>` body content is rendered when
//!   collapsed — that's an SEO concern handled separately.
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

/// Threshold below which `all-open-by-default` is not flagged
/// (one or two open details is plausible design intent).
const ALL_OPEN_THRESHOLD: usize = 3;

/// One captured `<details>` element with its summary signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DetailsEntry {
    /// CSS-ish selector pointing at the `<details>`.
    pub selector: String,
    /// Whether the `<details>` carries the `open` attribute.
    pub is_open: bool,
    /// Whether the `<details>` has at least one `<summary>`
    /// child (anywhere).
    pub has_summary: bool,
    /// Whether the `<summary>` (if present) is the first
    /// element child of the `<details>`.
    pub summary_is_first_child: bool,
    /// Trimmed text content of the `<summary>` (empty when no
    /// summary).
    pub summary_text: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DetailsOpenDefaultSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<details>` element on the page.
    pub entries: Vec<DetailsEntry>,
}

/// Detector.
#[must_use]
pub fn detect_details_open_default(
    snap: &DetailsOpenDefaultSnapshot,
) -> Vec<AxisFinding> {
    let mut missing_summary: Vec<&str> = Vec::new();
    let mut empty_summary: Vec<&str> = Vec::new();
    let mut summary_not_first: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        if !entry.has_summary {
            missing_summary.push(entry.selector.as_str());
            continue;
        }
        if entry.summary_text.trim().is_empty() {
            empty_summary.push(entry.selector.as_str());
        }
        if !entry.summary_is_first_child {
            summary_not_first.push(entry.selector.as_str());
        }
    }

    let total_details = snap.entries.len();
    let open_count = snap.entries.iter().filter(|e| e.is_open).count();
    let all_open = total_details >= ALL_OPEN_THRESHOLD && open_count == total_details;

    let mut findings = Vec::new();

    if !missing_summary.is_empty() {
        let preview = preview_examples(&missing_summary);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "details.missing-summary".to_owned(),
            detail: format!(
                "{} of {} <details> element(s) have no <summary> child. Examples: {}",
                missing_summary.len(),
                total_details,
                preview
            ),
        });
    }

    if !empty_summary.is_empty() {
        let preview = preview_examples(&empty_summary);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "details.empty-summary".to_owned(),
            detail: format!(
                "{} of {} <details> element(s) have a <summary> with empty / whitespace-only text. Examples: {}",
                empty_summary.len(),
                total_details,
                preview
            ),
        });
    }

    if !summary_not_first.is_empty() {
        let preview = preview_examples(&summary_not_first);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "details.summary-not-first-child".to_owned(),
            detail: format!(
                "{} of {} <details> element(s) have a <summary> that is NOT the first element child. Examples: {}",
                summary_not_first.len(),
                total_details,
                preview
            ),
        });
    }

    if all_open {
        let preview = preview_examples(
            &snap
                .entries
                .iter()
                .map(|e| e.selector.as_str())
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "details.all-open-by-default".to_owned(),
            detail: format!(
                "All {} <details> element(s) on the page default to open; defeats progressive disclosure intent. Examples: {}",
                total_details, preview
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

/// Page-side eval. Walks every `<details>` and captures the
/// summary signals.
pub const DETAILS_OPEN_DEFAULT_JS: &str = r##"(() => {
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

    const entries = Array.from(document.querySelectorAll('details')).map(function(d) {
      const summary = d.querySelector(':scope > summary');
      const hasSummary = !!summary;
      const firstElChild = d.firstElementChild;
      const summaryIsFirst = hasSummary && firstElChild === summary;
      const summaryText = hasSummary ? (summary.textContent || '').trim() : '';
      return {
        selector: selectorOf(d),
        isOpen: d.hasAttribute('open'),
        hasSummary: hasSummary,
        summaryIsFirstChild: summaryIsFirst,
        summaryText: summaryText
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

    fn entry(
        sel: &str,
        is_open: bool,
        has_summary: bool,
        is_first: bool,
        text: &str,
    ) -> DetailsEntry {
        DetailsEntry {
            selector: sel.to_owned(),
            is_open,
            has_summary,
            summary_is_first_child: is_first,
            summary_text: text.to_owned(),
        }
    }

    fn snap(entries: Vec<DetailsEntry>) -> DetailsOpenDefaultSnapshot {
        DetailsOpenDefaultSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_details_open_default(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn well_formed_details_is_clean() {
        let f = detect_details_open_default(&snap(vec![
            entry("details#faq-1", false, true, true, "FAQ question 1"),
            entry("details#faq-2", false, true, true, "FAQ question 2"),
        ]));
        assert!(f.is_empty(), "well-formed details should pass: {f:?}");
    }

    #[test]
    fn missing_summary_is_strict() {
        let f = detect_details_open_default(&snap(vec![entry(
            "details#bare",
            false,
            false,
            false,
            "",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "details.missing-summary")
            .expect("missing-summary expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("details#bare"));
    }

    #[test]
    fn empty_summary_is_strict() {
        let f = detect_details_open_default(&snap(vec![entry(
            "details#empty",
            false,
            true,
            true,
            "   ",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "details.empty-summary")
            .expect("empty-summary expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn summary_not_first_child_is_strict() {
        let f = detect_details_open_default(&snap(vec![entry(
            "details#out-of-order",
            false,
            true,
            false,
            "Summary",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "details.summary-not-first-child")
            .expect("not-first-child expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn three_or_more_all_open_is_warn() {
        let f = detect_details_open_default(&snap(vec![
            entry("details#a", true, true, true, "A"),
            entry("details#b", true, true, true, "B"),
            entry("details#c", true, true, true, "C"),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "details.all-open-by-default")
            .expect("all-open expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("All 3"));
    }

    #[test]
    fn two_all_open_below_threshold_not_flagged() {
        // Threshold is 3 — two open details is plausibly
        // intentional.
        let f = detect_details_open_default(&snap(vec![
            entry("details#a", true, true, true, "A"),
            entry("details#b", true, true, true, "B"),
        ]));
        assert!(
            !f.iter().any(|x| x.kind == "details.all-open-by-default"),
            "below threshold should not flag: {f:?}"
        );
    }

    #[test]
    fn mixed_open_state_not_flagged_as_all_open() {
        let f = detect_details_open_default(&snap(vec![
            entry("details#a", true, true, true, "A"),
            entry("details#b", false, true, true, "B"),
            entry("details#c", true, true, true, "C"),
            entry("details#d", false, true, true, "D"),
        ]));
        assert!(
            !f.iter().any(|x| x.kind == "details.all-open-by-default"),
            "mixed open state should not flag: {f:?}"
        );
    }

    #[test]
    fn missing_summary_does_not_also_trigger_empty_or_not_first() {
        // When has_summary is false, the other strict findings
        // should not double-fire.
        let f = detect_details_open_default(&snap(vec![entry(
            "details#bare",
            false,
            false,
            false,
            "",
        )]));
        assert!(f.iter().any(|x| x.kind == "details.missing-summary"));
        assert!(!f.iter().any(|x| x.kind == "details.empty-summary"));
        assert!(!f.iter().any(|x| x.kind == "details.summary-not-first-child"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("details#x{i}"), false, false, false, ""))
            .collect();
        let f = detect_details_open_default(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "details.missing-summary")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_details() {
        assert!(DETAILS_OPEN_DEFAULT_JS.starts_with("(() => {"));
        assert!(DETAILS_OPEN_DEFAULT_JS.ends_with(")()"));
        assert!(DETAILS_OPEN_DEFAULT_JS.contains("document.querySelectorAll('details')"));
        assert!(DETAILS_OPEN_DEFAULT_JS.contains(":scope > summary"));
        assert!(DETAILS_OPEN_DEFAULT_JS.contains("firstElementChild"));
    }
}
