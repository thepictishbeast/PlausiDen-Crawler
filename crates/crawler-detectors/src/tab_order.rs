//! `tab_order` — DOM tab traversal vs. visual reading order.
//!
//! WCAG 2.4.3 Focus Order (A): focusable components receive focus
//! in an order that preserves meaning + operability. The canonical
//! failure mode is a page where tab order leaps around because CSS
//! flexbox / grid reorder visually-positioned elements but does NOT
//! change the underlying DOM order.
//!
//! ## Heuristic
//!
//! For every focusable element (natural-focusable tag or
//! `tabindex >= 0`):
//!
//! 1. Record `(domIndex, centerX, centerY)`.
//! 2. Sort by tabindex first (positive tabindex > 0 forms its own
//!    sub-sequence), then by `domIndex`. This is the actual focus
//!    traversal order the browser will use.
//! 3. Compare against the natural reading order: rows first
//!    (top-to-bottom), within a row left-to-right (or right-to-left
//!    if `document.documentElement.dir === 'rtl'`). Elements share a
//!    row when their vertical centers are within a `ROW_TOLERANCE`
//!    of each other (defaults to 24px ≈ one line of body text).
//! 4. Walk the actual traversal — flag every element whose next
//!    element by traversal is visually *before* it in reading order
//!    (a "backward jump").
//!
//! Out of scope:
//! * Tabindex > 0 (positive tabindex is its own bug class — covered
//!   separately if needed; many auditors flag any positive tabindex
//!   unconditionally regardless of order).
//! * Modal / drawer focus traps with `aria-modal` (their tab order
//!   is intentionally scoped to the modal subtree).
//! * Disabled / hidden focusables (skipped at scan time).
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; the JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const TAB_ORDER_JS: &str = r##"(() => {
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
      const op = parseFloat(cs.opacity);
      if (!isNaN(op) && op === 0) return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    const naturallyFocusable = function(tag) {
      return tag === 'a' || tag === 'button' || tag === 'input' ||
             tag === 'select' || tag === 'textarea' || tag === 'summary';
    };

    const isFocusable = function(el) {
      const tag = el.tagName.toLowerCase();
      if (naturallyFocusable(tag)) {
        if (tag === 'a' && !el.hasAttribute('href')) return false;
        if (el.hasAttribute('disabled')) return false;
        return true;
      }
      const ti = el.getAttribute('tabindex');
      if (ti === null) return false;
      const n = parseInt(ti, 10);
      return !isNaN(n) && n >= 0;
    };

    // Skip focusables that are inside a closed dialog / aria-modal
    // scope. We only flag the document-level traversal; modals get
    // their own auditing axis.
    const insideAriaModal = function(el) {
      let p = el.parentElement;
      while (p && p !== document.body) {
        if (p.getAttribute('aria-modal') === 'true') return true;
        p = p.parentElement;
      }
      return false;
    };

    // Skip elements that are explicitly removed from sequential
    // navigation via tabindex=-1.
    const explicitlyOutOfOrder = function(el) {
      const ti = el.getAttribute('tabindex');
      if (ti === null) return false;
      const n = parseInt(ti, 10);
      return !isNaN(n) && n < 0;
    };

    const ROW_TOLERANCE = 24;
    const dir = (document.documentElement.getAttribute('dir') || 'ltr').toLowerCase();
    const rtl = dir === 'rtl';

    // Collect focusables in DOM order.
    const all = Array.from(document.querySelectorAll('*'));
    const focusables = [];
    let domIndex = 0;
    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      domIndex += 1;
      if (!isVisible(el)) continue;
      if (!isFocusable(el)) continue;
      if (explicitlyOutOfOrder(el)) continue;
      if (insideAriaModal(el)) continue;
      const rect = el.getBoundingClientRect();
      const tiRaw = el.getAttribute('tabindex');
      const ti = tiRaw === null ? 0 : (parseInt(tiRaw, 10) || 0);
      focusables.push({
        el: el,
        domIndex: domIndex,
        tabindex: ti,
        cx: rect.left + rect.width / 2,
        cy: rect.top + rect.height / 2,
        tag: el.tagName.toLowerCase(),
        text: (el.textContent || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 40)
      });
    }

    // Sort by tabindex (positive groups go first in their own
    // sub-order), then DOM index within a group. tabindex=0 ties
    // with implicit-focusable elements.
    const traversal = focusables.slice().sort(function(a, b) {
      const ag = a.tabindex > 0 ? a.tabindex : Number.MAX_SAFE_INTEGER;
      const bg = b.tabindex > 0 ? b.tabindex : Number.MAX_SAFE_INTEGER;
      if (ag !== bg) return ag - bg;
      return a.domIndex - b.domIndex;
    });

    // Walk the traversal; flag every adjacent pair where the next
    // element is visually before its predecessor in reading order.
    const jumps = [];
    for (let i = 0; i < traversal.length - 1; i++) {
      const a = traversal[i];
      const b = traversal[i + 1];
      const dy = b.cy - a.cy;
      const sameRow = Math.abs(dy) <= ROW_TOLERANCE;
      let backward;
      if (sameRow) {
        // Same row → check horizontal direction matches dir.
        backward = rtl ? (b.cx > a.cx) : (b.cx < a.cx);
      } else {
        // Different row → backward means cy decreases.
        backward = dy < 0;
      }
      if (backward) {
        jumps.push({
          fromSelector: selectorOf(a.el),
          fromTag: a.tag,
          fromText: a.text,
          fromCx: Math.round(a.cx),
          fromCy: Math.round(a.cy),
          toSelector: selectorOf(b.el),
          toTag: b.tag,
          toText: b.text,
          toCx: Math.round(b.cx),
          toCy: Math.round(b.cy),
          sameRow: sameRow
        });
        if (jumps.length >= 50) break;
      }
    }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      dir: dir,
      focusableCount: focusables.length,
      jumps: jumps
    };
})()"##;

/// One backward-jump pair: tab traversal moves from A to B but the
/// visual reading order would say A is *after* B.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TabOrderJump {
    /// CSS selector for the source (A).
    pub from_selector: String,
    /// `el.tagName.toLowerCase()` for A.
    pub from_tag: String,
    /// First 40 chars of A's text / value / aria-label.
    pub from_text: String,
    /// A's bounding-rect center X (CSS pixels, rounded).
    pub from_cx: i32,
    /// A's bounding-rect center Y.
    pub from_cy: i32,
    /// CSS selector for the destination (B).
    pub to_selector: String,
    /// B's tag.
    pub to_tag: String,
    /// B's text fingerprint.
    pub to_text: String,
    /// B's center X.
    pub to_cx: i32,
    /// B's center Y.
    pub to_cy: i32,
    /// `true` if A and B are on the same visual row (vertical
    /// centers within `ROW_TOLERANCE`).
    pub same_row: bool,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct TabOrderSnapshot {
    /// `window.innerWidth`.
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight`.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// `documentElement.dir` ("ltr" | "rtl") at probe time.
    pub dir: String,
    /// Total focusables scanned (post visibility / modal / negative-tabindex filtering).
    pub focusable_count: u32,
    /// Backward-jump pairs (capped at 50 for runtime safety).
    pub jumps: Vec<TabOrderJump>,
}

/// Apply detection rules. Pure function.
#[must_use]
pub fn detect_tab_order_issues(snap: &TabOrderSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    if snap.jumps.is_empty() {
        return out;
    }
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "tab-order.backward-jump".to_owned(),
        detail: format!(
            "{} tab-order backward jump(s) detected across {} focusable(s) (dir={}). WCAG 2.4.3 Focus Order. First: <{}> \"{}\" @ ({},{}) → <{}> \"{}\" @ ({},{}) {}",
            snap.jumps.len(),
            snap.focusable_count,
            snap.dir,
            snap.jumps[0].from_tag,
            snap.jumps[0].from_text,
            snap.jumps[0].from_cx,
            snap.jumps[0].from_cy,
            snap.jumps[0].to_tag,
            snap.jumps[0].to_text,
            snap.jumps[0].to_cx,
            snap.jumps[0].to_cy,
            if snap.jumps[0].same_row {
                "(same row, wrong-direction)"
            } else {
                "(prior row)"
            },
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
            TAB_ORDER_JS.matches('(').count(),
            TAB_ORDER_JS.matches(')').count()
        );
        assert_eq!(
            TAB_ORDER_JS.matches('{').count(),
            TAB_ORDER_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(TAB_ORDER_JS.starts_with("(() => {"));
        assert!(TAB_ORDER_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW",
            "vpH",
            "dir",
            "focusableCount",
            "jumps",
            "fromSelector",
            "fromTag",
            "fromText",
            "fromCx",
            "fromCy",
            "toSelector",
            "toTag",
            "toText",
            "toCx",
            "toCy",
            "sameRow",
        ] {
            assert!(TAB_ORDER_JS.contains(k), "missing key in JS: {k}");
        }
    }

    #[test]
    fn js_skips_aria_modal_scope() {
        assert!(TAB_ORDER_JS.contains("aria-modal"));
    }

    #[test]
    fn js_skips_negative_tabindex() {
        assert!(TAB_ORDER_JS.contains("explicitlyOutOfOrder"));
    }

    #[test]
    fn js_handles_rtl() {
        assert!(TAB_ORDER_JS.contains("rtl"));
    }

    #[test]
    fn empty_jumps_no_finding() {
        let snap = TabOrderSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "ltr".to_owned(),
            focusable_count: 12,
            jumps: vec![],
        };
        let findings = detect_tab_order_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn one_jump_emits_strict() {
        let snap = TabOrderSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "ltr".to_owned(),
            focusable_count: 6,
            jumps: vec![TabOrderJump {
                from_selector: "body > nav > a:nth-of-type(2)".to_owned(),
                from_tag: "a".to_owned(),
                from_text: "Pricing".to_owned(),
                from_cx: 800,
                from_cy: 32,
                to_selector: "body > nav > a:nth-of-type(1)".to_owned(),
                to_tag: "a".to_owned(),
                to_text: "Home".to_owned(),
                to_cx: 200,
                to_cy: 32,
                same_row: true,
            }],
        };
        let findings = detect_tab_order_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "tab-order.backward-jump");
        assert!(findings[0].detail.contains("WCAG 2.4.3"));
        assert!(findings[0].detail.contains("Pricing"));
        assert!(findings[0].detail.contains("same row"));
        assert!(findings[0].detail.contains("dir=ltr"));
    }

    #[test]
    fn cross_row_jump_marks_prior_row() {
        let snap = TabOrderSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "ltr".to_owned(),
            focusable_count: 8,
            jumps: vec![TabOrderJump {
                from_selector: "body > main > button".to_owned(),
                from_tag: "button".to_owned(),
                from_text: "Submit".to_owned(),
                from_cx: 640,
                from_cy: 400,
                to_selector: "body > header > button".to_owned(),
                to_tag: "button".to_owned(),
                to_text: "Menu".to_owned(),
                to_cx: 50,
                to_cy: 32,
                same_row: false,
            }],
        };
        let findings = detect_tab_order_issues(&snap);
        assert!(findings[0].detail.contains("prior row"));
    }

    #[test]
    fn multiple_jumps_reported_with_count() {
        let mut jumps = Vec::new();
        for i in 0..5 {
            jumps.push(TabOrderJump {
                from_selector: format!("a[{i}]"),
                from_tag: "a".to_owned(),
                from_text: format!("a{i}"),
                from_cx: 100,
                from_cy: 100,
                to_selector: format!("b[{i}]"),
                to_tag: "a".to_owned(),
                to_text: format!("b{i}"),
                to_cx: 50,
                to_cy: 100,
                same_row: true,
            });
        }
        let snap = TabOrderSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "ltr".to_owned(),
            focusable_count: 25,
            jumps,
        };
        let findings = detect_tab_order_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("5 tab-order backward jump"));
        assert!(findings[0].detail.contains("25 focusable"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = TabOrderSnapshot {
            vp_w: 360,
            vp_h: 800,
            dir: "rtl".to_owned(),
            focusable_count: 4,
            jumps: vec![TabOrderJump {
                from_selector: "x".to_owned(),
                from_tag: "button".to_owned(),
                from_text: "ה".to_owned(),
                from_cx: 100,
                from_cy: 200,
                to_selector: "y".to_owned(),
                to_tag: "button".to_owned(),
                to_text: "א".to_owned(),
                to_cx: 200,
                to_cy: 200,
                same_row: true,
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"vpW\":360"));
        assert!(json.contains("\"dir\":\"rtl\""));
        let back: TabOrderSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.dir, "rtl");
        assert_eq!(back.jumps.len(), 1);
        assert_eq!(back.jumps[0].from_text, "ה");
    }

    #[test]
    fn rtl_direction_surfaces_in_finding() {
        let snap = TabOrderSnapshot {
            vp_w: 1280,
            vp_h: 800,
            dir: "rtl".to_owned(),
            focusable_count: 2,
            jumps: vec![TabOrderJump {
                from_selector: "x".to_owned(),
                from_tag: "a".to_owned(),
                from_text: "ראשון".to_owned(),
                from_cx: 200,
                from_cy: 100,
                to_selector: "y".to_owned(),
                to_tag: "a".to_owned(),
                to_text: "שני".to_owned(),
                to_cx: 1000,
                to_cy: 100,
                same_row: true,
            }],
        };
        let findings = detect_tab_order_issues(&snap);
        assert!(findings[0].detail.contains("dir=rtl"));
    }
}
