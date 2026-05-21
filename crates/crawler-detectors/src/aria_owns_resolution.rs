//! `aria_owns_resolution` — id-reference resolution check for
//! `aria-owns` with cycle + double-parenting detection.
//!
//! Fourth and last sibling of the IDREF resolution detector
//! quartet: `aria_labelledby_resolution`,
//! `aria_describedby_resolution`, `aria_controls_resolution`,
//! and this axis. Same IDREF shape, different semantics:
//! `aria-owns` declares that the host element is the **logical
//! accessibility-tree parent** of the named element(s),
//! regardless of where those elements sit in the visible DOM.
//!
//! ## Why aria-owns needs more than just resolution
//!
//! Unlike labelledby/describedby/controls (which just refer),
//! aria-owns reorders the accessibility tree. Two new bug
//! classes appear:
//!
//! 1. **Double-parenting** — element X is named in
//!    `aria-owns` by both element A and element B. ARIA spec is
//!    explicit: the FIRST aria-owns wins and the second is
//!    silently ignored. Operators routinely write both because
//!    they didn't realise the rule, producing an accessibility
//!    tree that doesn't match the visible DOM in unpredictable
//!    ways.
//! 2. **Cycles** — A owns B, B owns A (or longer transitive
//!    chains). UAs vary in how they handle cycles; some bail
//!    silently, some loop until the page hangs. Surface
//!    aggressively.
//!
//! Plus the standard dangling / partial / empty checks shared
//! with the IDREF siblings.
//!
//! ## Findings
//!
//! * `aria-owns.dangling-ref` strict — NO id-ref in the
//!   attribute resolves to any element on the page.
//! * `aria-owns.partial-resolution` warn — multi-id attribute
//!   where SOME ids resolve and others don't.
//! * `aria-owns.empty-attribute` strict — attribute present
//!   with empty / whitespace-only value.
//! * `aria-owns.double-parented` strict — at least one target
//!   id is referenced by two or more aria-owns hosts. Second-
//!   and-later references are silently ignored by AT.
//! * `aria-owns.cycle-detected` strict — an aria-owns chain
//!   forms a cycle (A → B → A). Detector walks the directed
//!   graph and reports the cycle.
//!
//! Out of scope:
//!
//! * `aria-labelledby`, `aria-describedby`, `aria-controls` —
//!   covered by sibling axes.
//! * Validating that aria-owns is necessary at all (most cases
//!   should rely on DOM structure instead) — separate audit
//!   axis.
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
use std::collections::{HashMap, HashSet};

const MAX_EXAMPLES: usize = 5;

/// One element on the page carrying an `aria-owns` attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaOwnsEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Host element's own `id=` attribute (empty when none).
    /// Used for cycle detection.
    pub host_id: String,
    /// Raw attribute value (trimmed). Empty string means the
    /// attribute was present but its value was whitespace-only.
    pub attribute_value: String,
    /// Per-id resolution. Empty when `attribute_value` was empty.
    pub resolutions: Vec<OwnsResolution>,
}

/// Resolution result for a single id token inside the attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OwnsResolution {
    /// Id token from the attribute.
    pub id_ref: String,
    /// Whether the page has an element with this id.
    pub resolves: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AriaOwnsResolutionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page carrying `aria-owns`.
    pub entries: Vec<AriaOwnsEntry>,
}

/// Detector. Buckets findings by defect kind.
#[must_use]
pub fn detect_aria_owns_resolution(
    snap: &AriaOwnsResolutionSnapshot,
) -> Vec<AxisFinding> {
    let mut dangling: Vec<String> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    let mut empty_attr: Vec<&str> = Vec::new();
    // owned id → hosts referencing it
    let mut ownership: HashMap<&str, Vec<&str>> = HashMap::new();

    for entry in &snap.entries {
        if entry.attribute_value.is_empty() {
            empty_attr.push(entry.selector.as_str());
            continue;
        }
        let total = entry.resolutions.len();
        let resolved = entry.resolutions.iter().filter(|r| r.resolves).count();
        let unresolved: Vec<&str> = entry
            .resolutions
            .iter()
            .filter(|r| !r.resolves)
            .map(|r| r.id_ref.as_str())
            .collect();

        if resolved == 0 && total > 0 {
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

        // Track ownership for double-parent detection.
        for r in &entry.resolutions {
            if r.resolves {
                ownership
                    .entry(r.id_ref.as_str())
                    .or_default()
                    .push(entry.selector.as_str());
            }
        }
    }

    // Double-parent findings — any owned id with ≥ 2 referencing
    // hosts.
    let mut double_parented: Vec<String> = ownership
        .iter()
        .filter(|(_, hosts)| hosts.len() >= 2)
        .map(|(id, hosts)| format!("{id} <- {}", hosts.join(", ")))
        .collect();
    double_parented.sort();

    // Cycle detection: build host_id → owned_ids directed graph,
    // walk DFS for cycles.
    let cycles = detect_cycles(snap);

    let mut findings = Vec::new();
    let total_entries = snap.entries.len();

    if !dangling.is_empty() {
        let preview =
            preview_examples(&dangling.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-owns.dangling-ref".to_owned(),
            detail: format!(
                "{} of {} aria-owns host(s) have NO id-ref that resolves. Examples: {}",
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
            kind: "aria-owns.empty-attribute".to_owned(),
            detail: format!(
                "{} of {} aria-owns host(s) carry an empty / whitespace-only attribute value. Examples: {}",
                empty_attr.len(),
                total_entries,
                preview
            ),
        });
    }

    if !double_parented.is_empty() {
        let preview = preview_examples(
            &double_parented
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-owns.double-parented".to_owned(),
            detail: format!(
                "{} owned id(s) referenced by 2+ aria-owns hosts; second-and-later references are silently ignored by AT. Examples: {}",
                double_parented.len(),
                preview
            ),
        });
    }

    if !cycles.is_empty() {
        let preview =
            preview_examples(&cycles.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "aria-owns.cycle-detected".to_owned(),
            detail: format!(
                "{} aria-owns cycle(s) detected. UAs vary in cycle handling — never ship. Cycles: {}",
                cycles.len(),
                preview
            ),
        });
    }

    if !partial.is_empty() {
        let preview =
            preview_examples(&partial.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "aria-owns.partial-resolution".to_owned(),
            detail: format!(
                "{} of {} aria-owns host(s) have SOME ids that resolve and others that don't. Examples: {}",
                partial.len(),
                total_entries,
                preview
            ),
        });
    }

    findings
}

fn detect_cycles(snap: &AriaOwnsResolutionSnapshot) -> Vec<String> {
    // host_id → owned ids (only host entries with non-empty
    // host_id can participate in cycles, since cycle detection
    // requires walking aria-owns edges from owned id to that
    // element's own aria-owns).
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for entry in &snap.entries {
        if entry.host_id.is_empty() {
            continue;
        }
        let edges: Vec<&str> = entry
            .resolutions
            .iter()
            .filter(|r| r.resolves)
            .map(|r| r.id_ref.as_str())
            .collect();
        graph.insert(entry.host_id.as_str(), edges);
    }

    let mut cycles: Vec<String> = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();

    for &start in graph.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut on_path: HashSet<&str> = HashSet::new();
        dfs(start, &graph, &mut path, &mut on_path, &mut visited, &mut cycles);
    }

    cycles.sort();
    cycles.dedup();
    cycles
}

fn dfs<'a>(
    node: &'a str,
    graph: &HashMap<&'a str, Vec<&'a str>>,
    path: &mut Vec<&'a str>,
    on_path: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
    cycles: &mut Vec<String>,
) {
    if on_path.contains(node) {
        // Found a cycle — slice path from node back to current.
        if let Some(start) = path.iter().position(|&x| x == node) {
            let cycle: Vec<&str> = path[start..].iter().copied().chain(std::iter::once(node)).collect();
            cycles.push(cycle.join(" -> "));
        }
        return;
    }
    if visited.contains(node) {
        return;
    }
    path.push(node);
    on_path.insert(node);
    if let Some(neighbours) = graph.get(node) {
        for &n in neighbours {
            dfs(n, graph, path, on_path, visited, cycles);
        }
    }
    on_path.remove(node);
    path.pop();
    visited.insert(node);
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

/// Page-side eval. Walks every element with `aria-owns`,
/// captures host id + ownership references.
pub const ARIA_OWNS_RESOLUTION_JS: &str = r##"(() => {
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

    const hosts = Array.from(document.querySelectorAll('[aria-owns]'));
    const entries = hosts.map(function(el) {
      const raw = (el.getAttribute('aria-owns') || '').trim();
      const hostId = el.id || '';
      if (!raw) {
        return {
          selector: selectorOf(el),
          hostId: hostId,
          attributeValue: '',
          resolutions: []
        };
      }
      const ids = raw.split(/\s+/).filter(function(s) { return s.length > 0; });
      const resolutions = ids.map(function(id) {
        const target = document.getElementById(id);
        return { idRef: id, resolves: !!target };
      });
      return {
        selector: selectorOf(el),
        hostId: hostId,
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

    fn res(id: &str, resolves: bool) -> OwnsResolution {
        OwnsResolution {
            id_ref: id.to_owned(),
            resolves,
        }
    }

    fn entry(
        sel: &str,
        host_id: &str,
        attr: &str,
        resolutions: Vec<OwnsResolution>,
    ) -> AriaOwnsEntry {
        AriaOwnsEntry {
            selector: sel.to_owned(),
            host_id: host_id.to_owned(),
            attribute_value: attr.to_owned(),
            resolutions,
        }
    }

    fn snap(entries: Vec<AriaOwnsEntry>) -> AriaOwnsResolutionSnapshot {
        AriaOwnsResolutionSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_aria_owns_resolution(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn fully_resolved_no_double_parent_no_cycle_is_clean() {
        let f = detect_aria_owns_resolution(&snap(vec![
            entry("section#a", "a", "child-1", vec![res("child-1", true)]),
            entry("section#b", "b", "child-2", vec![res("child-2", true)]),
        ]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn dangling_is_strict() {
        let f = detect_aria_owns_resolution(&snap(vec![entry(
            "section#a",
            "a",
            "missing",
            vec![res("missing", false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-owns.dangling-ref")
            .expect("dangling expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("section#a"));
    }

    #[test]
    fn partial_resolution_is_warn() {
        let f = detect_aria_owns_resolution(&snap(vec![entry(
            "section#a",
            "a",
            "x y",
            vec![res("x", true), res("y", false)],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-owns.partial-resolution")
            .expect("partial expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("missing=y"));
    }

    #[test]
    fn empty_attribute_is_strict() {
        let f = detect_aria_owns_resolution(&snap(vec![entry(
            "section#a",
            "a",
            "",
            vec![],
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-owns.empty-attribute")
            .expect("empty expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn double_parented_is_strict() {
        let f = detect_aria_owns_resolution(&snap(vec![
            entry("section#a", "a", "shared", vec![res("shared", true)]),
            entry("section#b", "b", "shared", vec![res("shared", true)]),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-owns.double-parented")
            .expect("double-parented expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("shared"));
        assert!(hit.detail.contains("section#a"));
        assert!(hit.detail.contains("section#b"));
    }

    #[test]
    fn two_cycle_is_detected() {
        // a owns b, b owns a
        let f = detect_aria_owns_resolution(&snap(vec![
            entry("section#a", "a", "b", vec![res("b", true)]),
            entry("section#b", "b", "a", vec![res("a", true)]),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "aria-owns.cycle-detected")
            .expect("cycle expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(
            hit.detail.contains("a -> b -> a") || hit.detail.contains("b -> a -> b"),
            "cycle not in detail: {}",
            hit.detail
        );
    }

    #[test]
    fn three_cycle_is_detected() {
        // a -> b -> c -> a
        let f = detect_aria_owns_resolution(&snap(vec![
            entry("section#a", "a", "b", vec![res("b", true)]),
            entry("section#b", "b", "c", vec![res("c", true)]),
            entry("section#c", "c", "a", vec![res("a", true)]),
        ]));
        assert!(f
            .iter()
            .any(|x| x.kind == "aria-owns.cycle-detected"));
    }

    #[test]
    fn linear_chain_no_cycle_is_clean() {
        // a -> b -> c (no back-edge)
        let f = detect_aria_owns_resolution(&snap(vec![
            entry("section#a", "a", "b", vec![res("b", true)]),
            entry("section#b", "b", "c", vec![res("c", true)]),
        ]));
        assert!(
            !f.iter().any(|x| x.kind == "aria-owns.cycle-detected"),
            "linear chain should not flag a cycle: {f:?}"
        );
    }

    #[test]
    fn host_without_id_cannot_participate_in_cycle() {
        // Host has no id, so even if owned by something pointing
        // at "x" it can't close a cycle. Just confirms detector
        // doesn't crash on missing host_id.
        let f = detect_aria_owns_resolution(&snap(vec![entry(
            "section",
            "",
            "x",
            vec![res("x", true)],
        )]));
        assert!(!f.iter().any(|x| x.kind == "aria-owns.cycle-detected"));
    }

    #[test]
    fn js_const_is_iife_and_walks_aria_owns_hosts() {
        assert!(ARIA_OWNS_RESOLUTION_JS.starts_with("(() => {"));
        assert!(ARIA_OWNS_RESOLUTION_JS.ends_with(")()"));
        assert!(ARIA_OWNS_RESOLUTION_JS.contains("aria-owns"));
        assert!(ARIA_OWNS_RESOLUTION_JS.contains("getElementById"));
    }
}
