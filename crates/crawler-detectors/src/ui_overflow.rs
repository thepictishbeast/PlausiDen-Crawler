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

/// One uiOverflow finding produced by the pure detection logic.
/// Mirrors the TS `UIOverflowFinding` interface — severity, kind,
/// detail, evidence (opaque structured map).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiOverflowFinding {
    /// Strict (gate-blocking) or warn (within budget).
    pub severity: Severity,
    /// Machine-grepable id — `overflow.page-horizontal-scroll`,
    /// `overflow.element-bleeds-viewport`, `overflow.text-clipped`,
    /// `overflow.tap-target-too-small`.
    pub kind: String,
    /// Human-readable explanation; safe to surface to the operator
    /// without further formatting.
    pub detail: String,
}

/// Severity bucket for a finding. Matches the TS string literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Gate-blocking — fails the build / journey.
    Strict,
    /// Within budget — surfaces but doesn't block.
    Warn,
}

/// Apply detection heuristics to a snapshot. Pure function — no I/O.
///
/// Mirrors the TS `detectUIOverflowIssues` — same kinds, same
/// severity rules, same description text. The Rust port will be
/// regression-tested against TS Crawler outputs.
///
/// BUG ASSUMPTION: tap-target severity is mobile-strict / desktop-
/// warn at the 1024 px breakpoint, matching the TS heuristic.
/// A future refinement might use journey.viewport directly, but
/// snapshot.viewport (what the page rendered at) is what the user
/// actually saw, so we use that.
#[must_use]
pub fn detect_ui_overflow_issues(snap: &UiOverflowSnapshot) -> Vec<UiOverflowFinding> {
    let mut out = Vec::new();
    let vp = (snap.vp_w, snap.vp_h);

    // 1. Page-level horizontal scroll.
    let page_h_scroll = snap.doc_scroll_width > snap.doc_client_width + 2;
    if page_h_scroll {
        let delta = snap
            .doc_scroll_width
            .saturating_sub(snap.doc_client_width);
        out.push(UiOverflowFinding {
            severity: Severity::Strict,
            kind: "overflow.page-horizontal-scroll".to_owned(),
            detail: format!(
                "Page produces horizontal scrollbar — documentElement scrollWidth {}px > clientWidth {}px (delta {delta}px).",
                snap.doc_scroll_width, snap.doc_client_width
            ),
        });
    }

    // 2. Bleeding elements — collapse to one finding.
    if !snap.bleeding_elements.is_empty() {
        out.push(UiOverflowFinding {
            severity: Severity::Strict,
            kind: "overflow.element-bleeds-viewport".to_owned(),
            detail: format!(
                "{} element(s) extend past the viewport's right edge with no overflow-x scroll affordance.",
                snap.bleeding_elements.len()
            ),
        });
    }

    // 3. Text-clipped elements.
    if !snap.text_clipped_elements.is_empty() {
        out.push(UiOverflowFinding {
            severity: Severity::Strict,
            kind: "overflow.text-clipped".to_owned(),
            detail: format!(
                "{} element(s) have content wider than their container with no scroll affordance and no text-overflow:ellipsis.",
                snap.text_clipped_elements.len()
            ),
        });
    }

    // 4. Small tap targets — strict on mobile (<= 1024 px), warn on desktop.
    if !snap.small_tap_targets.is_empty() {
        let severity = if vp.0 <= 1024 {
            Severity::Strict
        } else {
            Severity::Warn
        };
        out.push(UiOverflowFinding {
            severity,
            kind: "overflow.tap-target-too-small".to_owned(),
            detail: format!(
                "{} interactive element(s) smaller than 44×44 px (WCAG 2.5.5 / iOS HIG floor) at viewport {}×{}.",
                snap.small_tap_targets.len(),
                vp.0,
                vp.1
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snap() -> UiOverflowSnapshot {
        UiOverflowSnapshot {
            vp_w: 1280,
            vp_h: 800,
            doc_scroll_width: 1280,
            doc_client_width: 1280,
            bleeding_elements: vec![],
            text_clipped_elements: vec![],
            small_tap_targets: vec![],
        }
    }

    fn rect(text: &str) -> OffenderRect {
        OffenderRect {
            selector: "body > div".to_owned(),
            left: 0,
            top: 0,
            width: 100,
            height: 20,
            right: 100,
            text: text.to_owned(),
        }
    }

    #[test]
    fn detect_clean_snapshot_silent() {
        let f = detect_ui_overflow_issues(&empty_snap());
        assert!(f.is_empty());
    }

    #[test]
    fn detect_page_horizontal_scroll() {
        let mut s = empty_snap();
        s.doc_scroll_width = 1500;
        s.doc_client_width = 1280;
        let f = detect_ui_overflow_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "overflow.page-horizontal-scroll");
        assert_eq!(f[0].severity, Severity::Strict);
        assert!(f[0].detail.contains("delta 220px"));
    }

    #[test]
    fn detect_bleed_collapses_to_one() {
        let mut s = empty_snap();
        s.bleeding_elements = vec![rect("a"), rect("b"), rect("c")];
        let f = detect_ui_overflow_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "overflow.element-bleeds-viewport");
        assert!(f[0].detail.starts_with("3 element"));
    }

    #[test]
    fn detect_tap_target_mobile_strict() {
        let mut s = empty_snap();
        s.vp_w = 375;
        s.small_tap_targets = vec![rect("close")];
        let f = detect_ui_overflow_issues(&s);
        assert_eq!(f[0].severity, Severity::Strict);
    }

    #[test]
    fn detect_tap_target_desktop_warn() {
        let mut s = empty_snap();
        s.vp_w = 1280;
        s.small_tap_targets = vec![rect("close")];
        let f = detect_ui_overflow_issues(&s);
        assert_eq!(f[0].severity, Severity::Warn);
    }

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
