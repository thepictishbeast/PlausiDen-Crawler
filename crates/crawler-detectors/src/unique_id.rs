//! `unique_id` — duplicate `id=` attribute detector.
//!
//! WCAG 2.1 Success Criterion 4.1.1 Parsing (A) — historically
//! required no duplicate IDs. Although the SC was removed in WCAG
//! 2.2 as obsolete (because modern parsers no longer assume unique
//! IDs for accessibility computation), duplicate IDs remain a real
//! interop bug class:
//!
//! * Anchor links (`href="#x"`) — browsers scroll to the FIRST `#x`,
//!   leaving any later same-id element unreachable.
//! * `aria-labelledby="x"` / `aria-describedby="x"` / `aria-controls="x"`
//!   — screen readers may follow only the first match, breaking the
//!   reference for later usages.
//! * `<label for="x">` — clicking the label focuses only the first
//!   matching input.
//! * `getElementById("x")` — JS reads only the first match; later
//!   elements with the same id receive no event listeners.
//!
//! Detector counts every visible `id=` and flags any value that
//! appears more than once. Empty `id=""` is also flagged — invalid
//! per HTML spec.
//!
//! HEURISTIC
//! ---------
//! 1. Walk every element with an `id` attribute (no visibility filter —
//!    even hidden elements register in `document.getElementById`).
//! 2. Group by id value.
//! 3. Emit one offender per (id, count > 1) pair, plus one for
//!    each empty-string id.
//! 4. Skip `id="-loom-skip"` / `id="-loom-anchor-N"` patterns the
//!    substrate uses internally with a `-loom-` prefix? No — those
//!    aren't supposed to repeat either. No skip-list.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const UNIQUE_ID_JS: &str = r##"(() => {
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

    const groups = {};
    const empties = [];
    const all = document.querySelectorAll('[id]');
    let scanned = 0;
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      scanned += 1;
      const id = el.getAttribute('id') || '';
      if (id === '') {
        empties.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase()
        });
        continue;
      }
      if (!groups[id]) groups[id] = [];
      groups[id].push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase()
      });
    }

    const duplicates = [];
    const ids = Object.keys(groups);
    for (let i = 0; i < ids.length; i++) {
      const id = ids[i];
      const occurrences = groups[id];
      if (occurrences.length > 1) {
        duplicates.push({
          id: id,
          count: occurrences.length,
          firstSelector: occurrences[0].selector,
          firstTag: occurrences[0].tag,
          secondSelector: occurrences[1].selector,
          secondTag: occurrences[1].tag
        });
      }
    }

    return {
      totalIds: scanned,
      uniqueIds: ids.length,
      duplicates: duplicates.slice(0, 50),
      duplicateCount: duplicates.length,
      empties: empties.slice(0, 50),
      emptyCount: empties.length
    };
})()"##;

/// One duplicate-id row: an id value that appears more than once
/// on the page, with the first two offending elements surfaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct DuplicateId {
    /// The repeated id value.
    pub id: String,
    /// Total number of elements carrying this id.
    pub count: u32,
    /// First offender's CSS selector.
    pub first_selector: String,
    /// First offender's tag.
    pub first_tag: String,
    /// Second offender's CSS selector (the duplicate).
    pub second_selector: String,
    /// Second offender's tag.
    pub second_tag: String,
}

/// One empty-id row: an element with `id=""`. Invalid per HTML spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct EmptyId {
    /// CSS selector.
    pub selector: String,
    /// Element tag.
    pub tag: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct UniqueIdSnapshot {
    /// Total elements with `id` attribute.
    pub total_ids: u32,
    /// Number of distinct non-empty id values.
    pub unique_ids: u32,
    /// Top 50 duplicate-id offenders, sorted by insertion order.
    pub duplicates: Vec<DuplicateId>,
    /// Total duplicate id values (may exceed `duplicates.len()` if
    /// truncated).
    pub duplicate_count: u32,
    /// Top 50 empty-id offenders.
    pub empties: Vec<EmptyId>,
    /// Total empty-id occurrences (may exceed `empties.len()`).
    pub empty_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits one strict finding per snapshot that contained at least one
/// duplicate or empty id. Duplicates and empties report together so
/// the operator sees a single id-hygiene summary, not two parallel
/// findings.
#[must_use]
pub fn detect_unique_id_issues(snap: &UniqueIdSnapshot) -> Vec<crate::AxisFinding> {
    if snap.duplicates.is_empty() && snap.empties.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<String> = Vec::new();
    if !snap.duplicates.is_empty() {
        let first = &snap.duplicates[0];
        parts.push(format!(
            "{} duplicate id value(s); worst: id=\"{}\" appears {}× (first: <{}> @ {}; second: <{}> @ {})",
            snap.duplicate_count,
            first.id,
            first.count,
            first.first_tag,
            first.first_selector,
            first.second_tag,
            first.second_selector,
        ));
    }
    if !snap.empties.is_empty() {
        let first = &snap.empties[0];
        parts.push(format!(
            "{} element(s) with empty `id=\"\"` (invalid per HTML spec; first: <{}> @ {})",
            snap.empty_count, first.tag, first.selector,
        ));
    }
    let mut out = Vec::with_capacity(1);
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "unique-id.duplicate-or-empty".to_owned(),
        detail: format!(
            "Page has id-hygiene violations. Total ids scanned: {}, unique: {}. {}",
            snap.total_ids,
            snap.unique_ids,
            parts.join(" · "),
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
            UNIQUE_ID_JS.matches('(').count(),
            UNIQUE_ID_JS.matches(')').count()
        );
        assert_eq!(
            UNIQUE_ID_JS.matches('{').count(),
            UNIQUE_ID_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(UNIQUE_ID_JS.starts_with("(() => {"));
        assert!(UNIQUE_ID_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "totalIds",
            "uniqueIds",
            "duplicates",
            "duplicateCount",
            "empties",
            "emptyCount",
            "id",
            "count",
            "firstSelector",
            "firstTag",
            "secondSelector",
            "secondTag",
            "selector",
            "tag",
        ] {
            assert!(UNIQUE_ID_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = UniqueIdSnapshot {
            total_ids: 12,
            unique_ids: 12,
            duplicates: vec![],
            duplicate_count: 0,
            empties: vec![],
            empty_count: 0,
        };
        let findings = detect_unique_id_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn duplicate_id_emits_strict() {
        let snap = UniqueIdSnapshot {
            total_ids: 5,
            unique_ids: 4,
            duplicates: vec![DuplicateId {
                id: "main".to_owned(),
                count: 2,
                first_selector: "body > header > div".to_owned(),
                first_tag: "div".to_owned(),
                second_selector: "body > main > div".to_owned(),
                second_tag: "div".to_owned(),
            }],
            duplicate_count: 1,
            empties: vec![],
            empty_count: 0,
        };
        let findings = detect_unique_id_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "unique-id.duplicate-or-empty");
        assert!(findings[0].detail.contains("duplicate id"));
        assert!(findings[0].detail.contains(r#"id="main""#));
        assert!(findings[0].detail.contains("2×"));
        assert!(findings[0].detail.contains("first: <div>"));
        assert!(findings[0].detail.contains("second: <div>"));
    }

    #[test]
    fn empty_id_emits_strict() {
        let snap = UniqueIdSnapshot {
            total_ids: 3,
            unique_ids: 2,
            duplicates: vec![],
            duplicate_count: 0,
            empties: vec![EmptyId {
                selector: "body > main > section".to_owned(),
                tag: "section".to_owned(),
            }],
            empty_count: 1,
        };
        let findings = detect_unique_id_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains(r#"empty `id=""`"#));
        assert!(findings[0].detail.contains("<section>"));
    }

    #[test]
    fn duplicate_and_empty_combine_into_one_finding() {
        // Both kinds of violation report in the same finding so the
        // operator sees a single id-hygiene summary, not two
        // parallel findings.
        let snap = UniqueIdSnapshot {
            total_ids: 8,
            unique_ids: 6,
            duplicates: vec![DuplicateId {
                id: "dup".to_owned(),
                count: 3,
                first_selector: "x".to_owned(),
                first_tag: "div".to_owned(),
                second_selector: "y".to_owned(),
                second_tag: "div".to_owned(),
            }],
            duplicate_count: 1,
            empties: vec![EmptyId {
                selector: "z".to_owned(),
                tag: "p".to_owned(),
            }],
            empty_count: 1,
        };
        let findings = detect_unique_id_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("duplicate id"));
        assert!(findings[0].detail.contains("empty"));
        // " · " separator between the two summary parts.
        assert!(findings[0].detail.contains(" · "));
    }

    #[test]
    fn total_and_unique_counts_surface() {
        let snap = UniqueIdSnapshot {
            total_ids: 42,
            unique_ids: 40,
            duplicates: vec![DuplicateId {
                id: "a".to_owned(),
                count: 2,
                first_selector: "x".to_owned(),
                first_tag: "div".to_owned(),
                second_selector: "y".to_owned(),
                second_tag: "div".to_owned(),
            }],
            duplicate_count: 1,
            empties: vec![],
            empty_count: 0,
        };
        let findings = detect_unique_id_issues(&snap);
        assert!(findings[0].detail.contains("Total ids scanned: 42"));
        assert!(findings[0].detail.contains("unique: 40"));
    }

    #[test]
    fn truncated_counts_surface_higher_than_array_len() {
        // If the JS truncated the offender list at 50 but counted
        // 73 total duplicates, the finding reports the full count
        // even though `.duplicates` is shorter.
        let snap = UniqueIdSnapshot {
            total_ids: 200,
            unique_ids: 127,
            duplicates: vec![DuplicateId {
                id: "x".to_owned(),
                count: 5,
                first_selector: "a".to_owned(),
                first_tag: "div".to_owned(),
                second_selector: "b".to_owned(),
                second_tag: "div".to_owned(),
            }],
            duplicate_count: 73,
            empties: vec![],
            empty_count: 0,
        };
        let findings = detect_unique_id_issues(&snap);
        assert!(findings[0].detail.contains("73 duplicate id"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = UniqueIdSnapshot {
            total_ids: 5,
            unique_ids: 4,
            duplicates: vec![DuplicateId {
                id: "x".to_owned(),
                count: 2,
                first_selector: "a".to_owned(),
                first_tag: "div".to_owned(),
                second_selector: "b".to_owned(),
                second_tag: "div".to_owned(),
            }],
            duplicate_count: 1,
            empties: vec![EmptyId {
                selector: "c".to_owned(),
                tag: "p".to_owned(),
            }],
            empty_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"totalIds\":5"));
        assert!(json.contains("\"duplicateCount\":1"));
        let back: UniqueIdSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.duplicates.len(), 1);
        assert_eq!(back.empties.len(), 1);
        assert_eq!(back.duplicates[0].id, "x");
    }
}
