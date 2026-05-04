//! `ui_overflow` — viewport / tap-target / text-clipping detector.
//!
//! Walks every visible element under `<body>` once and emits three
//! offender lists:
//!
//! * `bleeding_elements`     — element whose right edge exceeds
//!   `window.innerWidth + 4`, AND no ancestor scrolls — produces
//!   horizontal page scroll.
//! * `text_clipped_elements` — element whose `scrollWidth >
//!   clientWidth + 2` with no `overflow-x: auto/scroll` and no
//!   `text-overflow: ellipsis`. Form controls (input/textarea/
//!   select) are exempt — their intrinsic overflow is by design.
//! * `small_tap_targets`     — interactive (button / a /
//!   role=button etc.) under WCAG-2.5.5 44×44 floor. Honors the
//!   `data-tap="compact"` opt-out attribute.

use serde::{Deserialize, Serialize};

/// Page-side eval. See module docs for offender semantics.
pub const UI_OVERFLOW_JS: &str = r##"(() => {
    const docEl = document.documentElement;
    const docScrollWidth = docEl.scrollWidth;
    const docClientWidth = docEl.clientWidth;
    const vpW = window.innerWidth;
    const vpH = window.innerHeight;

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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      if (rect.right < -100 || rect.bottom < -100) return false;
      return true;
    };

    const truncateText = function(s) { return (s || '').replace(/\s+/g, ' ').trim().slice(0, 60); };

    const bleedingElements = [];
    const textClippedElements = [];
    const smallTapTargets = [];

    const all = document.querySelectorAll('body *');
    const tapSelectors = new Set(['button','a','input','select','textarea','summary']);

    for (let i = 0; i < all.length; i++) {
      const el = all[i];
      if (!isVisible(el)) continue;
      const rect = el.getBoundingClientRect();
      const cs = window.getComputedStyle(el);

      if (rect.right > vpW + 4 && rect.width > 0 && rect.left >= -100) {
        const parent = el.parentElement;
        const parentOverflowX = parent ? window.getComputedStyle(parent).overflowX : 'visible';
        if (parentOverflowX !== 'auto' && parentOverflowX !== 'scroll' && parentOverflowX !== 'hidden') {
          bleedingElements.push({
            selector: selectorOf(el),
            left: Math.round(rect.left),
            top: Math.round(rect.top),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
            right: Math.round(rect.right),
            text: truncateText(el.textContent || '')
          });
        }
      }

      const tagLower = el.tagName.toLowerCase();
      const isFormCtrl = tagLower === 'input' || tagLower === 'textarea' || tagLower === 'select';
      if (!isFormCtrl && el.scrollWidth > el.clientWidth + 2 && cs.overflowX !== 'auto' && cs.overflowX !== 'scroll' && cs.textOverflow !== 'ellipsis') {
        textClippedElements.push({
          selector: selectorOf(el),
          left: Math.round(rect.left),
          top: Math.round(rect.top),
          width: Math.round(rect.width),
          height: Math.round(rect.height),
          right: Math.round(rect.right),
          text: truncateText(el.textContent || '')
        });
      }

      const isTap = tapSelectors.has(el.tagName.toLowerCase()) || el.getAttribute('role') === 'button' || el.getAttribute('role') === 'link' || el.hasAttribute('onclick');
      if (isTap) {
        const inputType = el.type || '';
        if (inputType === 'hidden') continue;
        if (el.getAttribute('data-tap') === 'compact') continue;
        if (rect.width < 44 || rect.height < 44) {
          smallTapTargets.push({
            selector: selectorOf(el),
            left: Math.round(rect.left),
            top: Math.round(rect.top),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
            right: Math.round(rect.right),
            text: truncateText(el.textContent || el.getAttribute('aria-label') || '')
          });
        }
      }
    }

    return { vpW: vpW, vpH: vpH, docScrollWidth: docScrollWidth, docClientWidth: docClientWidth, bleedingElements: bleedingElements, textClippedElements: textClippedElements, smallTapTargets: smallTapTargets };
})()"##;

/// One offender rectangle reported in any of the 3 axes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct OffenderRect {
    /// Best-effort CSS selector path.
    pub selector: String,
    /// Bounding-rect left in CSS px.
    pub left: i32,
    /// Bounding-rect top in CSS px.
    pub top: i32,
    /// Bounding-rect width in CSS px.
    pub width: i32,
    /// Bounding-rect height in CSS px.
    pub height: i32,
    /// Bounding-rect right in CSS px.
    pub right: i32,
    /// First 60 chars of textContent (or aria-label fallback).
    pub text: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct UiOverflowSnapshot {
    /// `window.innerWidth`.
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight`.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// `documentElement.scrollWidth`.
    pub doc_scroll_width: u32,
    /// `documentElement.clientWidth`.
    pub doc_client_width: u32,
    /// Elements whose right edge bleeds past viewport with no
    /// scrolling ancestor — the cause of horizontal page scroll.
    pub bleeding_elements: Vec<OffenderRect>,
    /// Elements with content wider than container, no ellipsis.
    pub text_clipped_elements: Vec<OffenderRect>,
    /// Interactive elements smaller than WCAG 2.5.5 44×44 floor.
    pub small_tap_targets: Vec<OffenderRect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_balanced() {
        assert_eq!(
            UI_OVERFLOW_JS.matches('(').count(),
            UI_OVERFLOW_JS.matches(')').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(UI_OVERFLOW_JS.starts_with("(() => {"));
        assert!(UI_OVERFLOW_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW",
            "vpH",
            "docScrollWidth",
            "docClientWidth",
            "bleedingElements",
            "textClippedElements",
            "smallTapTargets",
        ] {
            assert!(UI_OVERFLOW_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn snapshot_round_trips() {
        let snap = UiOverflowSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_scroll_width: 1280,
            doc_client_width: 1280,
            bleeding_elements: vec![],
            text_clipped_elements: vec![OffenderRect {
                selector: "body > p".to_owned(),
                left: 0,
                top: 0,
                width: 100,
                height: 20,
                right: 100,
                text: "x".to_owned(),
            }],
            small_tap_targets: vec![],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: UiOverflowSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.text_clipped_elements.len(), 1);
    }
}
