//! `runtime_focus` — focus-visible detector.
//!
//! WCAG 2.4.7. Walks every interactive element, focuses each via
//! `el.focus({preventScroll:true})`, captures computed style
//! before+after, and emits offenders whose outline / box-shadow /
//! border-top did NOT change on focus — meaning a keyboard user
//! sees no indication the element is focused.
//!
//! Honors `data-focus-skip="true"` opt-out for composite controls
//! that delegate focus to a child.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const RUNTIME_FOCUS_JS: &str = r##"(() => {
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
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      return true;
    };

    const focusSignature = function(el) {
      const cs = window.getComputedStyle(el);
      return {
        outline: cs.outlineWidth + ' ' + cs.outlineStyle + ' ' + cs.outlineColor,
        boxShadow: cs.boxShadow,
        borderTop: cs.borderTopColor + ' ' + cs.borderTopWidth,
      };
    };

    const interactiveSelectors = [
      'button',
      'a[href]',
      'input:not([type="hidden"])',
      'select',
      'textarea',
      'summary',
      '[tabindex]:not([tabindex="-1"])',
      '[role="button"]',
      '[role="link"]',
      '[role="menuitem"]',
      '[role="tab"]',
      '[role="switch"]'
    ];
    const seen = new Set();
    const all = [];
    for (const sel of interactiveSelectors) {
      const els = document.querySelectorAll(sel);
      for (let i = 0; i < els.length; i++) {
        if (!seen.has(els[i])) { seen.add(els[i]); all.push(els[i]); }
      }
    }

    const invisibleFocus = [];
    let checked = 0;
    const previouslyFocused = document.activeElement;

    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (!isVisible(el)) continue;
      if (el.getAttribute('data-focus-skip') === 'true') continue;

      const before = focusSignature(el);
      try {
        el.focus({ preventScroll: true });
      } catch (e) {
        continue;
      }
      const after = focusSignature(el);
      checked += 1;

      const outlineChanged = before.outline !== after.outline;
      const boxShadowChanged = before.boxShadow !== after.boxShadow && (before.boxShadow === 'none' || after.boxShadow !== before.boxShadow);
      const borderChanged = before.borderTop !== after.borderTop;

      if (!outlineChanged && !boxShadowChanged && !borderChanged) {
        invisibleFocus.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          text: (el.textContent || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 40),
          beforeOutline: before.outline,
          afterOutline: after.outline,
          beforeBoxShadow: before.boxShadow,
          afterBoxShadow: after.boxShadow,
          beforeBorderTop: before.borderTop,
          afterBorderTop: after.borderTop
        });
      }
    }

    try {
      if (previouslyFocused && previouslyFocused.focus) previouslyFocused.focus({ preventScroll: true });
    } catch (e) { /* best effort */ }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      totalInteractive: all.length,
      totalChecked: checked,
      invisibleFocus: invisibleFocus
    };
})()"##;

/// One focus-offender row — element with no visible focus indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct FocusOffender {
    /// Best-effort CSS selector.
    pub selector: String,
    /// `el.tagName.toLowerCase()`.
    pub tag: String,
    /// First 40 chars of textContent / value / aria-label.
    pub text: String,
    /// Outline shorthand BEFORE focus.
    pub before_outline: String,
    /// Outline shorthand AFTER focus.
    pub after_outline: String,
    /// box-shadow BEFORE focus.
    pub before_box_shadow: String,
    /// box-shadow AFTER focus.
    pub after_box_shadow: String,
    /// border-top shorthand BEFORE focus.
    pub before_border_top: String,
    /// border-top shorthand AFTER focus.
    pub after_border_top: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFocusSnapshot {
    /// `window.innerWidth`.
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight`.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// Number of interactive elements found by the selector union.
    pub total_interactive: u32,
    /// Number actually focused (subset of total — visible, no opt-out).
    pub total_checked: u32,
    /// Offenders that gained no visible focus indicator.
    pub invisible_focus: Vec<FocusOffender>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_balanced() {
        assert_eq!(
            RUNTIME_FOCUS_JS.matches('(').count(),
            RUNTIME_FOCUS_JS.matches(')').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(RUNTIME_FOCUS_JS.starts_with("(() => {"));
        assert!(RUNTIME_FOCUS_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW",
            "vpH",
            "totalInteractive",
            "totalChecked",
            "invisibleFocus",
        ] {
            assert!(RUNTIME_FOCUS_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn snapshot_round_trips() {
        let snap = RuntimeFocusSnapshot {
            vp_w: 1280,
            vp_h: 800,
            total_interactive: 12,
            total_checked: 12,
            invisible_focus: vec![FocusOffender {
                selector: "body > button".to_owned(),
                tag: "button".to_owned(),
                text: "click me".to_owned(),
                before_outline: "0px none rgb(0, 0, 0)".to_owned(),
                after_outline: "0px none rgb(0, 0, 0)".to_owned(),
                before_box_shadow: "none".to_owned(),
                after_box_shadow: "none".to_owned(),
                before_border_top: "rgb(0, 0, 0) 0px".to_owned(),
                after_border_top: "rgb(0, 0, 0) 0px".to_owned(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: RuntimeFocusSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.invisible_focus.len(), 1);
    }
}
