//! `tab_panel_reciprocal` — `tab` ↔ `tabpanel` bidirectional
//! relationship audit.
//!
//! Sibling to `aria_labelledby_resolution`,
//! `aria_controls_resolution`, and `aria_expanded_state`. This
//! detector targets the specific reciprocal contract between
//! `role="tab"` and `role="tabpanel"`:
//!
//! ## The bidirectional contract
//!
//! Per ARIA Authoring Practices Guide (APG) for tabs:
//!
//! 1. Each tab `<button role="tab" aria-controls="panel-1">`
//!    points at its panel via `aria-controls`.
//! 2. Each panel `<div role="tabpanel" aria-labelledby="tab-1">`
//!    points BACK at its tab via `aria-labelledby` so the panel
//!    inherits the tab's accessible name as its own label.
//!
//! The reciprocal link is required for AT to navigate
//! correctly: forward (Tab key → panel content; arrow keys
//! within the tablist) and backward (the panel announces with
//! the tab's name so users know which content they're in).
//!
//! ## The bug class
//!
//! 1. **Tab points at panel but panel doesn't point back** —
//!    very common. The tab has `aria-controls="panel-1"` but
//!    `<div role="tabpanel">` has no `aria-labelledby` (or it
//!    points at a different element). Panel announces as
//!    "tabpanel" with no specific name.
//! 2. **Panel points back at a different tab** — operator
//!    copy-paste error. `<div role="tabpanel"
//!    aria-labelledby="tab-2">` but the controlling tab is
//!    `tab-1`. AT reports the wrong tab's name.
//! 3. **Multiple tabs control the same panel** — unusual but
//!    seen on tab-multiplexing sites; APG specifies one-to-one.
//!
//! ## Findings
//!
//! * `tab-panel.missing-back-reference` strict — tab points at
//!   a tabpanel target that exists in the snapshot AND that
//!   tabpanel has no `aria-labelledby` at all.
//! * `tab-panel.mismatched-back-reference` strict — tabpanel
//!   has `aria-labelledby` but the referenced element's id
//!   doesn't match the controlling tab's id.
//! * `tab-panel.multiple-tabs-control-same-panel` warn — two
//!   or more tabs share the same `aria-controls` target.
//!
//! Out of scope:
//!
//! * `aria-selected` state consistency — separate axis.
//! * Roving-tabindex management — separate runtime axis.
//! * Tabs / tabpanels that aren't connected via aria-controls
//!   at all — covered by `aria_controls_resolution` (the
//!   dangling case).
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
use std::collections::HashMap;

const MAX_EXAMPLES: usize = 5;

/// One captured tab + the panel it controls (when resolved).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TabEntry {
    /// CSS-ish selector pointing at the tab.
    pub tab_selector: String,
    /// `id=` on the tab itself (empty when none).
    pub tab_id: String,
    /// Tab's `aria-controls` target id (first id when multiple).
    pub controls_id: String,
    /// Whether the controls target exists on the page.
    pub controls_target_exists: bool,
    /// Tab's controlled-panel `aria-labelledby` value (empty
    /// when the panel doesn't have one).
    pub panel_labelledby: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TabPanelReciprocalSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `role="tab"` element on the page that carries
    /// `aria-controls`.
    pub tabs: Vec<TabEntry>,
}

/// Detector.
#[must_use]
pub fn detect_tab_panel_reciprocal(
    snap: &TabPanelReciprocalSnapshot,
) -> Vec<AxisFinding> {
    let mut missing_back: Vec<String> = Vec::new();
    let mut mismatched_back: Vec<String> = Vec::new();

    // Track which controls-target ids are referenced by how
    // many tabs — for the duplicate-control check.
    let mut control_counts: HashMap<&str, Vec<&str>> = HashMap::new();

    for entry in &snap.tabs {
        if entry.controls_id.is_empty() {
            continue;
        }
        control_counts
            .entry(entry.controls_id.as_str())
            .or_default()
            .push(entry.tab_selector.as_str());

        if !entry.controls_target_exists {
            // Dangling reference — covered by
            // aria_controls_resolution; skip here.
            continue;
        }

        if entry.panel_labelledby.trim().is_empty() {
            missing_back.push(format!(
                "{} controls {} — panel has no aria-labelledby",
                entry.tab_selector, entry.controls_id
            ));
            continue;
        }

        // Tabs with no id can't be back-referenced; treat as
        // missing.
        if entry.tab_id.is_empty() {
            missing_back.push(format!(
                "{} (tab has no id, panel can't point back)",
                entry.tab_selector
            ));
            continue;
        }

        // Check that the panel's labelledby references this
        // tab's id.
        let labelledby_ids: Vec<&str> = entry
            .panel_labelledby
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .collect();
        let back_ok = labelledby_ids.iter().any(|id| *id == entry.tab_id);
        if !back_ok {
            mismatched_back.push(format!(
                "{} (tab id={}, panel labelledby=\"{}\")",
                entry.tab_selector,
                entry.tab_id,
                entry.panel_labelledby
            ));
        }
    }

    let mut multiple_controllers: Vec<String> = control_counts
        .iter()
        .filter(|(_, hosts)| hosts.len() >= 2)
        .map(|(panel_id, hosts)| format!("panel #{panel_id} controlled by: {}", hosts.join(", ")))
        .collect();
    multiple_controllers.sort();

    let mut findings = Vec::new();
    let total = snap.tabs.len();

    if !missing_back.is_empty() {
        let preview = preview_examples(
            &missing_back.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "tab-panel.missing-back-reference".to_owned(),
            detail: format!(
                "{} of {} tab → tabpanel relationships lack a back-reference (aria-labelledby) from the panel to the tab. Examples: {}",
                missing_back.len(),
                total,
                preview
            ),
        });
    }

    if !mismatched_back.is_empty() {
        let preview = preview_examples(
            &mismatched_back
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "tab-panel.mismatched-back-reference".to_owned(),
            detail: format!(
                "{} of {} tabpanel(s) have aria-labelledby pointing at an id other than their controlling tab. Examples: {}",
                mismatched_back.len(),
                total,
                preview
            ),
        });
    }

    if !multiple_controllers.is_empty() {
        let preview = preview_examples(
            &multiple_controllers
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tab-panel.multiple-tabs-control-same-panel".to_owned(),
            detail: format!(
                "{} panel(s) referenced by 2+ tabs; APG specifies one-to-one tab/panel pairing. Examples: {}",
                multiple_controllers.len(),
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

/// Page-side eval. Walks `[role="tab"][aria-controls]` and
/// resolves each panel's aria-labelledby.
pub const TAB_PANEL_RECIPROCAL_JS: &str = r##"(() => {
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

    const tabs = Array.from(document.querySelectorAll('[role="tab"][aria-controls]'));
    const entries = tabs.map(function(tab) {
      const controlsRaw = (tab.getAttribute('aria-controls') || '').trim();
      const controlsId = controlsRaw.split(/\s+/)[0] || '';
      let targetExists = false;
      let labelledby = '';
      if (controlsId) {
        const panel = document.getElementById(controlsId);
        if (panel) {
          targetExists = true;
          labelledby = (panel.getAttribute('aria-labelledby') || '').trim();
        }
      }
      return {
        tabSelector: selectorOf(tab),
        tabId: tab.id || '',
        controlsId: controlsId,
        controlsTargetExists: targetExists,
        panelLabelledby: labelledby
      };
    });

    return {
      pageUrl: location.href,
      tabs: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        sel: &str,
        tab_id: &str,
        controls: &str,
        exists: bool,
        labelledby: &str,
    ) -> TabEntry {
        TabEntry {
            tab_selector: sel.to_owned(),
            tab_id: tab_id.to_owned(),
            controls_id: controls.to_owned(),
            controls_target_exists: exists,
            panel_labelledby: labelledby.to_owned(),
        }
    }

    fn snap(tabs: Vec<TabEntry>) -> TabPanelReciprocalSnapshot {
        TabPanelReciprocalSnapshot {
            page_url: "https://example.test/".to_owned(),
            tabs,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_tab_panel_reciprocal(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn fully_reciprocal_tab_pair_is_clean() {
        let f = detect_tab_panel_reciprocal(&snap(vec![entry(
            "button#tab-1",
            "tab-1",
            "panel-1",
            true,
            "tab-1",
        )]));
        assert!(f.is_empty(), "reciprocal pair should pass: {f:?}");
    }

    #[test]
    fn missing_panel_back_reference_is_strict() {
        let f = detect_tab_panel_reciprocal(&snap(vec![entry(
            "button#tab-1",
            "tab-1",
            "panel-1",
            true,
            "",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tab-panel.missing-back-reference")
            .expect("missing-back-reference expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("button#tab-1"));
    }

    #[test]
    fn mismatched_back_reference_is_strict() {
        let f = detect_tab_panel_reciprocal(&snap(vec![entry(
            "button#tab-1",
            "tab-1",
            "panel-1",
            true,
            "tab-2",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tab-panel.mismatched-back-reference")
            .expect("mismatched expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("tab-2"));
    }

    #[test]
    fn panel_labelledby_with_multiple_ids_passes_when_one_matches() {
        // Some patterns include both the tab id and a separate
        // heading id; as long as our tab id appears, accept it.
        let f = detect_tab_panel_reciprocal(&snap(vec![entry(
            "button#tab-1",
            "tab-1",
            "panel-1",
            true,
            "tab-1 panel-heading",
        )]));
        assert!(f.is_empty(), "multi-id labelledby with match should pass: {f:?}");
    }

    #[test]
    fn dangling_controls_target_does_not_double_fire() {
        // controls target doesn't exist — aria_controls_resolution
        // covers it. Don't double-fire here.
        let f = detect_tab_panel_reciprocal(&snap(vec![entry(
            "button#tab-x",
            "tab-x",
            "panel-x",
            false,
            "",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "tab-panel.missing-back-reference"),
            "dangling target should not flag missing-back: {f:?}"
        );
    }

    #[test]
    fn tab_without_id_cant_be_back_referenced_flags_missing() {
        let f = detect_tab_panel_reciprocal(&snap(vec![entry(
            "button.tab-trigger",
            "",
            "panel-1",
            true,
            "some-other-id",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tab-panel.missing-back-reference")
            .expect("missing-back-reference expected");
        assert!(hit.detail.contains("tab has no id"));
    }

    #[test]
    fn multiple_tabs_controlling_same_panel_is_warn() {
        let f = detect_tab_panel_reciprocal(&snap(vec![
            entry("button#tab-a", "tab-a", "panel-1", true, "tab-a"),
            entry("button#tab-b", "tab-b", "panel-1", true, "tab-a"),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tab-panel.multiple-tabs-control-same-panel")
            .expect("multiple-controllers expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("panel-1"));
        assert!(hit.detail.contains("button#tab-a"));
        assert!(hit.detail.contains("button#tab-b"));
    }

    #[test]
    fn three_tabs_one_panel_aggregated_into_single_finding() {
        let f = detect_tab_panel_reciprocal(&snap(vec![
            entry("button#a", "a", "p1", true, "a"),
            entry("button#b", "b", "p1", true, "a"),
            entry("button#c", "c", "p1", true, "a"),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tab-panel.multiple-tabs-control-same-panel")
            .unwrap();
        assert!(hit.detail.contains("1 panel(s)"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let tabs: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("button#t{i}"),
                    &format!("t{i}"),
                    &format!("p{i}"),
                    true,
                    "",
                )
            })
            .collect();
        let f = detect_tab_panel_reciprocal(&snap(tabs));
        let hit = f
            .iter()
            .find(|x| x.kind == "tab-panel.missing-back-reference")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_tabs() {
        assert!(TAB_PANEL_RECIPROCAL_JS.starts_with("(() => {"));
        assert!(TAB_PANEL_RECIPROCAL_JS.ends_with(")()"));
        assert!(TAB_PANEL_RECIPROCAL_JS.contains("[role=\"tab\"][aria-controls]"));
        assert!(TAB_PANEL_RECIPROCAL_JS.contains("aria-labelledby"));
    }
}
