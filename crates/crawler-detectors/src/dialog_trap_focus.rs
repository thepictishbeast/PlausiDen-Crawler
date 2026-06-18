//! `dialog_trap_focus` — open-modal focus-management detector.
//!
//! Sibling axis to `dialog_label` (which checks accessible names
//! on `<dialog>` regardless of state). This detector targets the
//! OPEN state: when a modal dialog is on screen, keyboard focus
//! must be confined to it. A leaking modal lets Tab walk into the
//! background page, confusing both sighted keyboard users and
//! screen-reader users — and on touch-AT devices it routes around
//! the modal entirely.
//!
//! ## The bug class
//!
//! Native `<dialog open>` (and `dialog.showModal()`) DOES sequester
//! focus by browser default — but only if there are focusable
//! children inside the dialog AND nothing outside has been left
//! reachable via `tabindex`. Operators frequently:
//!
//! 1. Render `<dialog open>` with no focusable children. The
//!    initial Tab press escapes the dialog because there's nothing
//!    inside to focus on.
//! 2. Use `<div role="dialog">` polyfills without applying `inert`
//!    or `aria-hidden="true"` to the rest of the document, so the
//!    background page remains tab-reachable.
//! 3. Open a `<dialog>` but leave document `tabindex` overrides
//!    pointing into the background, making Tab cycle out.
//!
//! Each shape breaks WCAG 2.1.2 (No Keyboard Trap — applied in
//! reverse: keyboard should be *trapped inside* a modal, not
//! escape out of it) and 2.4.3 (Focus Order).
//!
//! ## Findings
//!
//! * `dialog-trap.no-focusable-inside` strict — open dialog with
//!   zero focusable descendants. Cannot satisfy modal-focus
//!   contract.
//! * `dialog-trap.background-not-inert` strict — open dialog and
//!   background page (siblings of the open dialog) still hold
//!   tab-reachable focusable elements without `inert` applied.
//!   Background remains keyboard-reachable behind the modal.
//! * `dialog-trap.initial-focus-outside` warn — at snapshot time
//!   the document's active element was NOT inside the open
//!   dialog. Initial focus should land inside the modal when it
//!   opens.
//! * `dialog-trap.positive-tabindex-outside` warn — focusable
//!   element OUTSIDE the dialog uses `tabindex` ≥ 1, which jumps
//!   the natural focus order ahead of the modal entirely.
//!
//! Out of scope:
//!
//! * Closed dialogs — `<dialog>` without `open` attribute is
//!   `display: none` by browser default and not in focus order.
//! * Focus-return-on-close — the page state after the modal
//!   closes is a separate axis (`focus_return_on_close`, future).
//! * Multiple stacked modals — detector treats each open dialog
//!   independently; ordering across z-indexed modals is a wider
//!   layout axis.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-effect
//!   channel.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const MAX_EXAMPLES: usize = 5;

/// One open dialog (or `role="dialog"` polyfill) with its focus-
/// management signals captured at snapshot time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OpenDialogEntry {
    /// CSS-ish selector pointing at the dialog.
    pub selector: String,
    /// `native` for `<dialog open>`, `polyfill` for
    /// `<div role="dialog">` shapes.
    pub kind: DialogKind,
    /// Count of focusable descendants inside the dialog at
    /// snapshot time (links / buttons / inputs / textarea /
    /// select / `[tabindex]`≥0).
    pub focusable_inside_count: u32,
    /// Count of tab-reachable focusable elements OUTSIDE the
    /// dialog subtree that are NOT inside an `inert` ancestor and
    /// are NOT inside an `aria-hidden="true"` ancestor.
    pub focusable_outside_count: u32,
    /// Count of `tabindex` values ≥ 1 OUTSIDE the dialog. Each
    /// is its own focus-order jump regardless of inert state.
    pub positive_tabindex_outside_count: u32,
    /// Whether `document.activeElement` was inside the dialog at
    /// snapshot time.
    pub active_element_inside: bool,
}

/// Shape of dialog: native `<dialog>` element vs. ARIA polyfill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogKind {
    /// `<dialog open>` (native element).
    Native,
    /// `<div role="dialog">` / `<div role="alertdialog">` (ARIA).
    Polyfill,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DialogTrapFocusSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every open modal dialog observed at snapshot time.
    pub open_dialogs: Vec<OpenDialogEntry>,
}

/// Detector. Buckets findings by defect kind so consumers see at
/// most one finding per kind (with up to `MAX_EXAMPLES` selectors
/// surfaced in `detail`).
#[must_use]
pub fn detect_dialog_trap_focus(snap: &DialogTrapFocusSnapshot) -> Vec<AxisFinding> {
    let mut no_focusable_inside: Vec<&str> = Vec::new();
    let mut background_not_inert: Vec<&str> = Vec::new();
    let mut initial_focus_outside: Vec<&str> = Vec::new();
    let mut positive_tabindex_outside: Vec<&str> = Vec::new();

    for entry in &snap.open_dialogs {
        if entry.focusable_inside_count == 0 {
            no_focusable_inside.push(entry.selector.as_str());
        }
        if entry.focusable_outside_count > 0 {
            background_not_inert.push(entry.selector.as_str());
        }
        if !entry.active_element_inside {
            initial_focus_outside.push(entry.selector.as_str());
        }
        if entry.positive_tabindex_outside_count > 0 {
            positive_tabindex_outside.push(entry.selector.as_str());
        }
    }

    let mut findings = Vec::new();
    let total = snap.open_dialogs.len();

    if !no_focusable_inside.is_empty() {
        let preview = preview_examples(&no_focusable_inside);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "dialog-trap.no-focusable-inside".to_owned(),
            detail: format!(
                "{} of {} open modal(s) have zero focusable descendants — keyboard focus cannot be trapped inside. Examples: {}",
                no_focusable_inside.len(),
                total,
                preview
            ),
        });
    }

    if !background_not_inert.is_empty() {
        let preview = preview_examples(&background_not_inert);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "dialog-trap.background-not-inert".to_owned(),
            detail: format!(
                "{} of {} open modal(s) leave background-page focusable elements tab-reachable (no inert / aria-hidden). Examples: {}",
                background_not_inert.len(),
                total,
                preview
            ),
        });
    }

    if !positive_tabindex_outside.is_empty() {
        let preview = preview_examples(&positive_tabindex_outside);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "dialog-trap.positive-tabindex-outside".to_owned(),
            detail: format!(
                "{} of {} open modal(s) have outside elements with tabindex>=1 jumping focus order past the modal. Examples: {}",
                positive_tabindex_outside.len(),
                total,
                preview
            ),
        });
    }

    if !initial_focus_outside.is_empty() {
        let preview = preview_examples(&initial_focus_outside);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "dialog-trap.initial-focus-outside".to_owned(),
            detail: format!(
                "{} of {} open modal(s) had document.activeElement OUTSIDE the dialog at snapshot. Examples: {}",
                initial_focus_outside.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn preview_examples(selectors: &[&str]) -> String {
    let mut buf = String::new();
    let n = selectors.len().min(MAX_EXAMPLES);
    for (i, sel) in selectors.iter().take(n).enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        buf.push_str(sel);
    }
    if selectors.len() > MAX_EXAMPLES {
        buf.push_str(&format!(" (+{} more)", selectors.len() - MAX_EXAMPLES));
    }
    buf
}

/// Page-side eval. Walks open `<dialog>` and `role="dialog"`
/// subtrees, counts focusable elements inside vs outside, checks
/// `inert` + `aria-hidden` ancestors on every outside candidate,
/// and reports whether `document.activeElement` is inside the
/// dialog.
pub const DIALOG_TRAP_FOCUS_JS: &str = r##"(() => {
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

    const FOCUSABLE_QUERY = 'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]';

    const isFocusable = function(el) {
      if (!el || el.nodeType !== 1) return false;
      if (el.hasAttribute('disabled')) return false;
      if (el.tagName === 'INPUT' && el.type === 'hidden') return false;
      const ti = el.getAttribute('tabindex');
      if (ti !== null && parseInt(ti, 10) < 0) return false;
      const tag = el.tagName.toLowerCase();
      if (tag === 'a' && !el.hasAttribute('href')) return false;
      return el.matches(FOCUSABLE_QUERY);
    };

    const hasInertAncestor = function(el) {
      let node = el;
      while (node && node !== document.body) {
        if (node.hasAttribute && node.hasAttribute('inert')) return true;
        node = node.parentElement;
      }
      return false;
    };

    const hasAriaHiddenAncestor = function(el) {
      let node = el;
      while (node && node !== document.body) {
        if (node.getAttribute && node.getAttribute('aria-hidden') === 'true') return true;
        node = node.parentElement;
      }
      return false;
    };

    const positiveTabindex = function(el) {
      const ti = el.getAttribute && el.getAttribute('tabindex');
      if (ti === null || ti === undefined) return false;
      const n = parseInt(ti, 10);
      return Number.isFinite(n) && n >= 1;
    };

    const collectDialogs = function() {
      const out = [];
      const native = Array.from(document.querySelectorAll('dialog[open]'));
      native.forEach(function(d) { out.push({ el: d, kind: 'native' }); });
      const polyfill = Array.from(document.querySelectorAll('[role="dialog"], [role="alertdialog"]'));
      polyfill.forEach(function(d) {
        if (d.tagName.toLowerCase() === 'dialog') return; // already captured
        // Only treat as open if visible.
        const style = (typeof getComputedStyle === 'function') ? getComputedStyle(d) : null;
        if (style && (style.display === 'none' || style.visibility === 'hidden')) return;
        out.push({ el: d, kind: 'polyfill' });
      });
      return out;
    };

    const activeEl = document.activeElement;
    const openDialogs = collectDialogs().map(function(rec) {
      const dialogEl = rec.el;
      const insideAll = Array.from(dialogEl.querySelectorAll(FOCUSABLE_QUERY)).filter(isFocusable);
      const allFocusables = Array.from(document.querySelectorAll(FOCUSABLE_QUERY)).filter(isFocusable);
      let outsideCount = 0;
      let positiveTiOutside = 0;
      allFocusables.forEach(function(el) {
        if (dialogEl.contains(el)) return;
        if (hasInertAncestor(el)) return;
        if (hasAriaHiddenAncestor(el)) return;
        outsideCount += 1;
        if (positiveTabindex(el)) positiveTiOutside += 1;
      });
      const activeInside = !!(activeEl && dialogEl.contains(activeEl));
      return {
        selector: selectorOf(dialogEl),
        kind: rec.kind,
        focusableInsideCount: insideAll.length,
        focusableOutsideCount: outsideCount,
        positiveTabindexOutsideCount: positiveTiOutside,
        activeElementInside: activeInside
      };
    });

    return {
      pageUrl: location.href,
      openDialogs: openDialogs
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        sel: &str,
        kind: DialogKind,
        inside: u32,
        outside: u32,
        positive_ti: u32,
        active_inside: bool,
    ) -> OpenDialogEntry {
        OpenDialogEntry {
            selector: sel.to_owned(),
            kind,
            focusable_inside_count: inside,
            focusable_outside_count: outside,
            positive_tabindex_outside_count: positive_ti,
            active_element_inside: active_inside,
        }
    }

    fn snap(open_dialogs: Vec<OpenDialogEntry>) -> DialogTrapFocusSnapshot {
        DialogTrapFocusSnapshot {
            page_url: "https://example.test/".to_owned(),
            open_dialogs,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let findings = detect_dialog_trap_focus(&snap(vec![]));
        assert!(findings.is_empty());
    }

    #[test]
    fn well_managed_modal_is_clean() {
        let findings = detect_dialog_trap_focus(&snap(vec![entry(
            "dialog.checkout",
            DialogKind::Native,
            3,
            0,
            0,
            true,
        )]));
        assert!(findings.is_empty());
    }

    #[test]
    fn dialog_with_no_focusable_inside_is_strict() {
        let findings = detect_dialog_trap_focus(&snap(vec![entry(
            "dialog.empty",
            DialogKind::Native,
            0,
            0,
            0,
            false,
        )]));
        // No focusable inside AND active outside → 2 findings.
        let by_kind: Vec<_> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(by_kind.contains(&"dialog-trap.no-focusable-inside"));
        let strict = findings
            .iter()
            .find(|f| f.kind == "dialog-trap.no-focusable-inside")
            .unwrap();
        assert_eq!(strict.severity, AxisSeverity::Strict);
        assert!(strict.detail.contains("dialog.empty"));
        assert!(strict.detail.contains("1 of 1"));
    }

    #[test]
    fn polyfill_with_background_focusables_flags_background_not_inert() {
        let findings = detect_dialog_trap_focus(&snap(vec![entry(
            "div[role=\"dialog\"].login-modal",
            DialogKind::Polyfill,
            4,
            12,
            0,
            true,
        )]));
        let f = findings
            .iter()
            .find(|f| f.kind == "dialog-trap.background-not-inert")
            .expect("background-not-inert expected");
        assert_eq!(f.severity, AxisSeverity::Strict);
        assert!(f.detail.contains("login-modal"));
        assert!(f.detail.contains("1 of 1"));
        // No no-focusable-inside (this one has 4 inside).
        assert!(!findings
            .iter()
            .any(|f| f.kind == "dialog-trap.no-focusable-inside"));
    }

    #[test]
    fn positive_tabindex_outside_is_warn() {
        let findings = detect_dialog_trap_focus(&snap(vec![entry(
            "dialog.cart",
            DialogKind::Native,
            5,
            0,
            2,
            true,
        )]));
        let f = findings
            .iter()
            .find(|f| f.kind == "dialog-trap.positive-tabindex-outside")
            .expect("positive-tabindex-outside expected");
        assert_eq!(f.severity, AxisSeverity::Warn);
        assert!(f.detail.contains("dialog.cart"));
        // Background-not-inert NOT emitted (outside count is 0).
        assert!(!findings
            .iter()
            .any(|f| f.kind == "dialog-trap.background-not-inert"));
    }

    #[test]
    fn initial_focus_outside_is_warn_when_otherwise_clean() {
        let findings = detect_dialog_trap_focus(&snap(vec![entry(
            "dialog.modal",
            DialogKind::Native,
            3,
            0,
            0,
            false,
        )]));
        // Only one finding — initial-focus-outside warn.
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "dialog-trap.initial-focus-outside");
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert!(findings[0].detail.contains("dialog.modal"));
    }

    #[test]
    fn multiple_dialogs_bucket_into_single_finding_per_kind() {
        let findings = detect_dialog_trap_focus(&snap(vec![
            entry("dialog.a", DialogKind::Native, 0, 0, 0, false),
            entry("dialog.b", DialogKind::Native, 0, 0, 0, false),
            entry("dialog.c", DialogKind::Native, 0, 0, 0, false),
        ]));
        let nfi = findings
            .iter()
            .find(|f| f.kind == "dialog-trap.no-focusable-inside")
            .unwrap();
        // One finding aggregates all three modal selectors.
        assert!(nfi.detail.contains("3 of 3"));
        assert!(nfi.detail.contains("dialog.a"));
        assert!(nfi.detail.contains("dialog.b"));
        assert!(nfi.detail.contains("dialog.c"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<OpenDialogEntry> = (0..8)
            .map(|i| {
                entry(
                    &format!("dialog#m{i}"),
                    DialogKind::Native,
                    3,
                    9,
                    0,
                    true,
                )
            })
            .collect();
        let findings = detect_dialog_trap_focus(&snap(entries));
        let bni = findings
            .iter()
            .find(|f| f.kind == "dialog-trap.background-not-inert")
            .unwrap();
        // 8 selectors, MAX_EXAMPLES = 5, so "(+3 more)" trailer.
        assert!(bni.detail.contains("(+3 more)"), "{}", bni.detail);
        // First 5 selectors appear.
        for i in 0..5 {
            assert!(bni.detail.contains(&format!("dialog#m{i}")));
        }
        // Trailing 3 do NOT appear.
        for i in 5..8 {
            assert!(
                !bni.detail.contains(&format!("dialog#m{i}")),
                "selector m{i} should be hidden"
            );
        }
    }

    #[test]
    fn mixed_kinds_share_bucket() {
        let findings = detect_dialog_trap_focus(&snap(vec![
            entry("dialog.native-one", DialogKind::Native, 4, 7, 0, true),
            entry(
                "div[role=\"alertdialog\"].polyfill-two",
                DialogKind::Polyfill,
                3,
                4,
                0,
                true,
            ),
        ]));
        let bni = findings
            .iter()
            .find(|f| f.kind == "dialog-trap.background-not-inert")
            .unwrap();
        // Both modals folded into one finding.
        assert!(bni.detail.contains("2 of 2"));
        assert!(bni.detail.contains("native-one"));
        assert!(bni.detail.contains("polyfill-two"));
    }

    #[test]
    fn js_const_is_non_empty_and_iife_shaped() {
        assert!(DIALOG_TRAP_FOCUS_JS.starts_with("(() => {"));
        assert!(DIALOG_TRAP_FOCUS_JS.ends_with(")()"));
        // Touches both native + polyfill collection paths.
        assert!(DIALOG_TRAP_FOCUS_JS.contains("dialog[open]"));
        assert!(DIALOG_TRAP_FOCUS_JS.contains("role=\"dialog\""));
        assert!(DIALOG_TRAP_FOCUS_JS.contains("inert"));
        assert!(DIALOG_TRAP_FOCUS_JS.contains("aria-hidden"));
    }
}
