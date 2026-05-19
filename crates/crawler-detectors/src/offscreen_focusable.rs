//! `offscreen_focusable` — flags focusable elements positioned
//! off-screen, the "phantom focus" bug class.
//!
//! Bug class: an author hides content from sighted users by
//! moving the element off-screen with absolute / fixed positioning
//! (e.g. `left: -9999px`, `transform: translateX(-200%)`,
//! `top: -10000px`) — but the element remains in the focus
//! order. Keyboard users tab into invisible space; the screen
//! shows no focus ring; the user is lost.
//!
//! Legitimate uses exist:
//!
//! - Skip-links — `<a href="#content" class="loom-skip">Skip to content</a>`
//!   positioned off-screen until focused, then snapped back into
//!   the viewport. Loom's `.loom-skip` follows this pattern.
//! - Pure-AT content via `.sr-only` — but per WCAG 2.1 the
//!   correct pattern combines `position: absolute; width: 1px;
//!   height: 1px; overflow: hidden; clip: rect(0,0,0,0);` —
//!   keeping the element visible to assistive tech AND off-screen
//!   for sighted users, BUT _not focusable_ unless explicitly
//!   needed (skip-link exception).
//!
//! ## Heuristic
//!
//! For every focusable element (natural-focusable tag OR
//! `tabindex >= 0`):
//!
//! 1. Get `getBoundingClientRect()` + `getComputedStyle()`.
//! 2. Off-screen test: rect.right < -50 OR rect.left > viewport+50
//!    OR rect.bottom < -50 OR rect.top > document-height+200.
//! 3. Skip if the element OR ancestor has `class*="skip"` (skip-link
//!    convention — Loom's `.loom-skip` + author-friendly variants).
//! 4. Skip if rect.width == 0 AND rect.height == 0 (display:none
//!    descendants — covered by other detectors).
//!
//! Severity:
//!
//! - `strict` — element is focusable AND off-screen AND not a
//!   skip-link. Keyboard users can land here with no visible
//!   indicator.
//! - `warn` — focusable, off-screen, IS a skip-link but doesn't
//!   appear to have a `:focus` rule that brings it back. Best-
//!   effort heuristic (can't easily check pseudo-class rules
//!   without a full style table).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a focusable element positioned off-screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OffscreenFocusableHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped at 80 chars). May be empty for
    /// non-text focusables like form controls.
    pub text: String,
    /// Tag name (uppercase, e.g. `BUTTON`, `A`, `INPUT`).
    pub tag: String,
    /// Bounding-box left edge (CSS px from viewport).
    pub rect_left: i32,
    /// Bounding-box top edge (CSS px from viewport).
    pub rect_top: i32,
    /// Bounding-box right edge.
    pub rect_right: i32,
    /// Bounding-box bottom edge.
    pub rect_bottom: i32,
    /// True iff the element OR an ancestor declares
    /// `class*="skip"` (skip-link convention).
    pub looks_like_skip_link: bool,
    /// Tabindex value as integer (-1 if absent or non-numeric).
    pub tabindex: i32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OffscreenFocusableSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture (CSS px).
    pub viewport_width: u32,
    /// Viewport height at capture (CSS px).
    pub viewport_height: u32,
    /// Total scrollable document height (CSS px).
    pub document_height: u32,
    /// Every focusable element that fired the heuristic.
    pub hits: Vec<OffscreenFocusableHit>,
    /// Total focusable elements walked — noise floor.
    pub scanned_elements: u32,
}

/// Pure detector: snapshot → findings. Splits the hits into
/// `strict` (phantom focus) and `warn` (skip-link without
/// observable :focus snap-back).
#[must_use]
pub fn detect_offscreen_focusable(snap: &OffscreenFocusableSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let (skips, phantoms): (Vec<&OffscreenFocusableHit>, Vec<&OffscreenFocusableHit>) =
        snap.hits.iter().partition(|h| h.looks_like_skip_link);

    let mut out = Vec::new();
    if !phantoms.is_empty() {
        let examples: Vec<String> = phantoms
            .iter()
            .take(5)
            .map(|h| {
                format!(
                    "{} <{}> tabindex={} rect=({},{},{},{}) text=\"{}\"",
                    h.selector,
                    h.tag,
                    h.tabindex,
                    h.rect_left,
                    h.rect_top,
                    h.rect_right,
                    h.rect_bottom,
                    h.text
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "offscreen-focusable.phantom".to_owned(),
            detail: format!(
                "{} focusable element(s) positioned off-screen without a skip-link class — keyboard users tab into invisible space (phantom focus). Either remove from focus order (`tabindex=\"-1\"`), restore on-screen at narrow viewports, or apply the typed `loom-skip` pattern. Examples: {}",
                phantoms.len(),
                examples.join("; ")
            ),
        });
    }
    if !skips.is_empty() {
        let examples: Vec<String> = skips
            .iter()
            .take(5)
            .map(|h| format!("{} <{}>", h.selector, h.tag))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "offscreen-focusable.skip-link".to_owned(),
            detail: format!(
                "{} likely skip-link(s) detected — confirm each has a `:focus`/`:focus-visible` rule that brings it back into the viewport when keyboard-focused. Loom's `.loom-skip` ships with that rule; check author-friendly variants. Examples: {}",
                skips.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Pinned for the future
/// chromiumoxide path; mirror any change in this file's hit +
/// snapshot fields.
pub const OFFSCREEN_FOCUSABLE_DOM_CAPTURE_JS: &str = r#"
(() => {
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

    const NATURAL_FOCUSABLE = new Set([
      'A','AREA','BUTTON','INPUT','SELECT','TEXTAREA','SUMMARY','IFRAME','DETAILS','AUDIO','VIDEO'
    ]);

    const isFocusable = function(el) {
      if (el.disabled) return false;
      if (NATURAL_FOCUSABLE.has(el.tagName)) {
        if (el.tagName === 'A' && !el.hasAttribute('href')) return false;
        return true;
      }
      const ti = el.getAttribute('tabindex');
      if (ti === null) return false;
      const tin = parseInt(ti, 10);
      if (!isFinite(tin)) return false;
      return tin >= 0;
    };

    const looksLikeSkipLink = function(el) {
      let node = el;
      while (node && node.nodeType === 1) {
        const cls = (node.getAttribute('class') || '').toLowerCase();
        if (cls.indexOf('skip') !== -1) return true;
        node = node.parentElement;
        if (node === document.body) break;
      }
      return false;
    };

    const docHeight = Math.max(
      document.documentElement.scrollHeight,
      document.body.scrollHeight,
      window.innerHeight
    );
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    const hits = [];
    let scanned = 0;
    const candidates = document.querySelectorAll('a, area, button, input, select, textarea, summary, iframe, details, audio, video, [tabindex]');
    for (let i = 0; i < candidates.length; i++) {
      const el = candidates[i];
      if (!isFocusable(el)) continue;
      scanned += 1;
      const rect = el.getBoundingClientRect();
      // Skip display:none / collapsed
      if (rect.width === 0 && rect.height === 0) continue;
      const offLeft   = rect.right < -50;
      const offRight  = rect.left  > vw + 50;
      const offTop    = rect.bottom < -50;
      const offBottom = rect.top   > docHeight + 200;
      if (!(offLeft || offRight || offTop || offBottom)) continue;
      const ti = el.getAttribute('tabindex');
      const tabidx = ti === null ? -1 : parseInt(ti, 10);
      const text = (el.textContent || el.value || '').trim().slice(0, 80);
      hits.push({
        selector: selectorOf(el),
        text: text,
        tag: el.tagName,
        rectLeft: Math.round(rect.left),
        rectTop: Math.round(rect.top),
        rectRight: Math.round(rect.right),
        rectBottom: Math.round(rect.bottom),
        looksLikeSkipLink: looksLikeSkipLink(el),
        tabindex: isFinite(tabidx) ? tabidx : -1,
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: vw,
      viewportHeight: vh,
      documentHeight: docHeight,
      hits: hits,
      scannedElements: scanned,
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn phantom(selector: &str, tag: &str, looks_like_skip: bool) -> OffscreenFocusableHit {
        OffscreenFocusableHit {
            selector: selector.into(),
            text: "Hidden".into(),
            tag: tag.into(),
            rect_left: -10000,
            rect_top: 100,
            rect_right: -9900,
            rect_bottom: 130,
            looks_like_skip_link: looks_like_skip,
            tabindex: 0,
        }
    }

    #[test]
    fn empty_snapshot_produces_no_findings() {
        let snap = OffscreenFocusableSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            viewport_height: 800,
            document_height: 3000,
            hits: vec![],
            scanned_elements: 0,
        };
        assert!(detect_offscreen_focusable(&snap).is_empty());
    }

    #[test]
    fn phantom_focus_produces_strict_finding() {
        let snap = OffscreenFocusableSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            viewport_height: 800,
            document_height: 3000,
            hits: vec![phantom("body > button.hidden-search", "BUTTON", false)],
            scanned_elements: 1,
        };
        let findings = detect_offscreen_focusable(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "offscreen-focusable.phantom");
    }

    #[test]
    fn skip_link_produces_warn_finding() {
        let snap = OffscreenFocusableSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            viewport_height: 800,
            document_height: 3000,
            hits: vec![phantom("body > a.loom-skip", "A", true)],
            scanned_elements: 1,
        };
        let findings = detect_offscreen_focusable(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "offscreen-focusable.skip-link");
    }

    #[test]
    fn mixed_hits_produce_two_findings() {
        let snap = OffscreenFocusableSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            viewport_height: 800,
            document_height: 3000,
            hits: vec![
                phantom("body > a.loom-skip", "A", true),
                phantom("body > button.search-hidden", "BUTTON", false),
            ],
            scanned_elements: 2,
        };
        let findings = detect_offscreen_focusable(&snap);
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().any(|f| f.severity == AxisSeverity::Strict));
        assert!(findings.iter().any(|f| f.severity == AxisSeverity::Warn));
    }

    #[test]
    fn js_capture_constant_is_sensible() {
        assert!(OFFSCREEN_FOCUSABLE_DOM_CAPTURE_JS.contains("getBoundingClientRect"));
        assert!(OFFSCREEN_FOCUSABLE_DOM_CAPTURE_JS.contains("looksLikeSkipLink"));
        assert!(OFFSCREEN_FOCUSABLE_DOM_CAPTURE_JS.contains("isFocusable"));
    }
}
