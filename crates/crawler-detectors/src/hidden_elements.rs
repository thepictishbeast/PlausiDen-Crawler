//! `hidden_elements` — flags accessibility-broken hidden content.
//!
//! Bug classes this detector catches:
//!
//! 1. **aria-hidden on a focusable element** — the element gets
//!    keyboard focus but screen readers report nothing. The user
//!    lands on an "empty" focus stop. Most-common cause: an icon
//!    button has `aria-hidden="true"` to hide the decorative SVG,
//!    and that attribute was placed on the `<button>` instead of
//!    the inner `<svg>`.
//!
//! 2. **display:none / visibility:hidden on a focusable element
//!    WITH tabindex >= 0** — author wanted to disable the control
//!    but used display tricks instead of `disabled` / `hidden`
//!    attribute. Behaviour varies across browsers and ATs.
//!
//! 3. **Zero-sized content-bearing element** — `width: 0; height: 0`
//!    or `clip: rect(0,0,0,0)` on an element with visible text
//!    content. Either the author is doing the sr-only pattern
//!    wrong, or the layout collapsed unintentionally.
//!
//! 4. **opacity:0 with text** — text rendered transparent. Used
//!    in older sr-only patterns; also used by spammers / cloaking.
//!    Surface as a warning so the author can confirm intent.
//!
//! ## Heuristic
//!
//! Walk the DOM. For each element, capture:
//!   - tag, selector path, text content (capped 80 chars)
//!   - aria-hidden value
//!   - tabindex value (-1 if absent)
//!   - rect (left/top/right/bottom from getBoundingClientRect)
//!   - computed display, visibility, opacity, clip
//!   - is_focusable per the standard list (a[href], button, input,
//!     select, textarea, summary, [tabindex >= 0])
//!
//! Then classify each hit into one of the four bug classes.
//! Severity:
//!   - 1 (aria-hidden on focusable)        → strict
//!   - 2 (display-tricks on focusable)     → strict
//!   - 3 (zero-sized content-bearing)      → warn
//!   - 4 (opacity:0 with text)             → warn
//!
//! AVP-2 invariants: pure detector, no I/O. Browser-side DOM
//! capture lives in [`HIDDEN_ELEMENTS_DOM_CAPTURE_JS`].

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Why a single element fired the detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenElementCause {
    /// `aria-hidden="true"` on a natively focusable element.
    AriaHiddenFocusable,
    /// `display:none` / `visibility:hidden` on an element with
    /// `tabindex >= 0` (focusable via keyboard).
    DisplayHiddenFocusable,
    /// Zero-sized rect (width = 0 AND height = 0) on an element
    /// with non-empty text content.
    ZeroSizedContentBearing,
    /// `opacity:0` (or near-zero) on an element with non-empty
    /// text content.
    OpacityZeroWithText,
}

/// One captured offender.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HiddenElementHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Tag name (uppercase, e.g. `BUTTON`).
    pub tag: String,
    /// Visible text (capped at 80 chars).
    pub text: String,
    /// Why this element fired.
    pub cause: HiddenElementCause,
    /// Tabindex value as integer (-1 if absent or non-numeric).
    pub tabindex: i32,
    /// `aria-hidden` attribute as a string (`"true"`, `"false"`,
    /// or empty when absent).
    pub aria_hidden: String,
    /// Computed `display` value.
    pub display: String,
    /// Computed `visibility` value.
    pub visibility: String,
    /// Computed `opacity` value (0.0..=1.0).
    pub opacity: f32,
    /// Bounding-box width (CSS px).
    pub rect_width: i32,
    /// Bounding-box height (CSS px).
    pub rect_height: i32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HiddenElementsSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element that fired the heuristic.
    pub hits: Vec<HiddenElementHit>,
    /// Total elements walked — noise floor.
    pub scanned_elements: u32,
}

/// Pure detector: snapshot → findings. Splits hits by cause and
/// emits one finding per bug class (so an operator can fix the
/// whole class at once rather than chasing N individual sites).
#[must_use]
pub fn detect_hidden_elements(snap: &HiddenElementsSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let mut by_cause: [(HiddenElementCause, Vec<&HiddenElementHit>); 4] = [
        (HiddenElementCause::AriaHiddenFocusable, Vec::new()),
        (HiddenElementCause::DisplayHiddenFocusable, Vec::new()),
        (HiddenElementCause::ZeroSizedContentBearing, Vec::new()),
        (HiddenElementCause::OpacityZeroWithText, Vec::new()),
    ];
    for hit in &snap.hits {
        for entry in &mut by_cause {
            if entry.0 == hit.cause {
                entry.1.push(hit);
                break;
            }
        }
    }

    let mut out = Vec::new();
    for (cause, bucket) in &by_cause {
        if bucket.is_empty() {
            continue;
        }
        let examples: Vec<String> = bucket
            .iter()
            .take(5)
            .map(|h| format_hit(h, *cause))
            .collect();
        let (severity, kind, headline) = match cause {
            HiddenElementCause::AriaHiddenFocusable => (
                AxisSeverity::Strict,
                "hidden-elements.aria-hidden-focusable",
                "aria-hidden=\"true\" on focusable element(s) — keyboard users land on an empty focus stop (no AT label, no perceivable purpose). Move aria-hidden to the inner decorative child (typically the <svg>) or replace with `hidden` / `aria-label`.",
            ),
            HiddenElementCause::DisplayHiddenFocusable => (
                AxisSeverity::Strict,
                "hidden-elements.display-hidden-focusable",
                "focusable element(s) hidden via display:none / visibility:hidden but still in the tab order — use the `disabled` attribute or the `hidden` attribute instead; both keep semantics consistent across browsers + ATs.",
            ),
            HiddenElementCause::ZeroSizedContentBearing => (
                AxisSeverity::Warn,
                "hidden-elements.zero-sized-content",
                "element(s) with text content rendered at 0×0 — either the layout collapsed unintentionally (broken flex/grid?) or the sr-only pattern is missing `clip: rect(0,0,0,0)` + `overflow: hidden`. Verify intent.",
            ),
            HiddenElementCause::OpacityZeroWithText => (
                AxisSeverity::Warn,
                "hidden-elements.opacity-zero-text",
                "text content rendered with opacity:0 — invisible to sighted users but still in the AT tree, and indexable by search engines. Spam-cloaking pattern; legitimate uses are rare. Confirm intent.",
            ),
        };
        out.push(AxisFinding {
            severity,
            kind: kind.to_owned(),
            detail: format!(
                "{}: {} site(s). {}",
                kind,
                bucket.len(),
                headline,
            ) + " Examples: "
                + &examples.join("; "),
        });
    }
    out
}

fn format_hit(h: &HiddenElementHit, cause: HiddenElementCause) -> String {
    let suffix = match cause {
        HiddenElementCause::AriaHiddenFocusable => {
            format!("aria-hidden={:?} tabindex={}", h.aria_hidden, h.tabindex)
        }
        HiddenElementCause::DisplayHiddenFocusable => {
            format!(
                "display={} visibility={} tabindex={}",
                h.display, h.visibility, h.tabindex
            )
        }
        HiddenElementCause::ZeroSizedContentBearing => {
            format!("rect={}x{}", h.rect_width, h.rect_height)
        }
        HiddenElementCause::OpacityZeroWithText => {
            format!("opacity={}", h.opacity)
        }
    };
    format!(
        "{} <{}> text={:?} {}",
        h.selector, h.tag, h.text, suffix
    )
}

/// Browser-side DOM-capture script. Pinned for the chromiumoxide
/// path; mirror any change in this file's hit + snapshot fields.
pub const HIDDEN_ELEMENTS_DOM_CAPTURE_JS: &str = r#"
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
      if (el.hasAttribute('hidden')) return false;
      const ti = parseInt(el.getAttribute('tabindex') || '', 10);
      if (Number.isFinite(ti) && ti >= 0) return true;
      if (NATURAL_FOCUSABLE.has(el.tagName)) {
        if (el.tagName === 'A' && !el.hasAttribute('href')) return false;
        return true;
      }
      return false;
    };

    const visibleTextOf = function(el) {
      const t = (el.textContent || '').replace(/\s+/g, ' ').trim();
      return t.length > 80 ? t.slice(0, 77) + '...' : t;
    };

    const hits = [];
    let scanned = 0;
    const all = document.querySelectorAll('body *');
    for (const el of all) {
      scanned += 1;
      const cs = getComputedStyle(el);
      const ariaHidden = el.getAttribute('aria-hidden') || '';
      const display = cs.display;
      const visibility = cs.visibility;
      const opacity = parseFloat(cs.opacity);
      const ti = parseInt(el.getAttribute('tabindex') || '', 10);
      const tabindex = Number.isFinite(ti) ? ti : -1;
      const rect = el.getBoundingClientRect();
      const w = Math.round(rect.width);
      const h = Math.round(rect.height);
      const focusable = isFocusable(el);
      const text = visibleTextOf(el);

      // Cause 1: aria-hidden on focusable element.
      if (focusable && ariaHidden === 'true') {
        hits.push({
          selector: selectorOf(el), tag: el.tagName, text, cause: 'aria_hidden_focusable',
          tabindex, ariaHidden, display, visibility, opacity, rectWidth: w, rectHeight: h
        });
        continue;
      }
      // Cause 2: display:none / visibility:hidden on focusable element with tabindex.
      // (Natural-focusable buttons hidden via display:none are usually intentional —
      // we only flag the tabindex>=0 case where the author explicitly opted in to
      // the focus order and then hid the element via CSS.)
      if (tabindex >= 0 && (display === 'none' || visibility === 'hidden')) {
        hits.push({
          selector: selectorOf(el), tag: el.tagName, text, cause: 'display_hidden_focusable',
          tabindex, ariaHidden, display, visibility, opacity, rectWidth: w, rectHeight: h
        });
        continue;
      }
      // Cause 3: 0×0 size with non-empty text content.
      if (w === 0 && h === 0 && text.length > 0 && display !== 'none') {
        hits.push({
          selector: selectorOf(el), tag: el.tagName, text, cause: 'zero_sized_content_bearing',
          tabindex, ariaHidden, display, visibility, opacity, rectWidth: w, rectHeight: h
        });
        continue;
      }
      // Cause 4: opacity:0 (or <0.05) with non-empty text content.
      // Skip when display:none — covered by other detectors and the element
      // isn't rendered at all.
      if (opacity < 0.05 && text.length > 0 && display !== 'none') {
        hits.push({
          selector: selectorOf(el), tag: el.tagName, text, cause: 'opacity_zero_with_text',
          tabindex, ariaHidden, display, visibility, opacity, rectWidth: w, rectHeight: h
        });
      }
    }

    return {
      pageUrl: location.href,
      hits,
      scannedElements: scanned
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        cause: HiddenElementCause,
        text: &str,
        tabindex: i32,
        aria_hidden: &str,
    ) -> HiddenElementHit {
        HiddenElementHit {
            selector: "body > div > button".into(),
            tag: "BUTTON".into(),
            text: text.into(),
            cause,
            tabindex,
            aria_hidden: aria_hidden.into(),
            display: "block".into(),
            visibility: "visible".into(),
            opacity: 1.0,
            rect_width: 100,
            rect_height: 40,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: vec![],
            scanned_elements: 1500,
        };
        assert!(detect_hidden_elements(&s).is_empty());
    }

    #[test]
    fn aria_hidden_focusable_emits_strict() {
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: vec![hit(HiddenElementCause::AriaHiddenFocusable, "Submit", 0, "true")],
            scanned_elements: 1,
        };
        let f = detect_hidden_elements(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert!(f[0].kind.contains("aria-hidden-focusable"));
        assert!(f[0].detail.contains("Submit"));
    }

    #[test]
    fn display_hidden_focusable_emits_strict() {
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: vec![hit(HiddenElementCause::DisplayHiddenFocusable, "Hidden CTA", 0, "")],
            scanned_elements: 1,
        };
        let f = detect_hidden_elements(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn zero_sized_emits_warn() {
        let mut h = hit(HiddenElementCause::ZeroSizedContentBearing, "Cloaked", -1, "");
        h.rect_width = 0;
        h.rect_height = 0;
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: vec![h],
            scanned_elements: 1,
        };
        let f = detect_hidden_elements(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert!(f[0].kind.contains("zero-sized-content"));
    }

    #[test]
    fn opacity_zero_emits_warn() {
        let mut h = hit(HiddenElementCause::OpacityZeroWithText, "Cloaked link", -1, "");
        h.opacity = 0.0;
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: vec![h],
            scanned_elements: 1,
        };
        let f = detect_hidden_elements(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Warn);
        assert!(f[0].kind.contains("opacity-zero-text"));
    }

    #[test]
    fn mixed_causes_emit_one_finding_per_cause() {
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: vec![
                hit(HiddenElementCause::AriaHiddenFocusable, "X", 0, "true"),
                hit(HiddenElementCause::AriaHiddenFocusable, "Y", 0, "true"),
                hit(HiddenElementCause::DisplayHiddenFocusable, "Z", 0, ""),
            ],
            scanned_elements: 3,
        };
        let f = detect_hidden_elements(&s);
        assert_eq!(f.len(), 2); // aria-hidden (2 hits) + display-hidden (1 hit)
        // Verify both bug classes report their respective counts.
        let aria = f.iter().find(|x| x.kind.contains("aria-hidden")).expect("aria");
        let disp = f.iter().find(|x| x.kind.contains("display-hidden")).expect("disp");
        assert!(aria.detail.contains("2 site(s)"));
        assert!(disp.detail.contains("1 site(s)"));
    }

    #[test]
    fn examples_capped_at_five() {
        let many: Vec<HiddenElementHit> = (0..10)
            .map(|i| {
                let mut h = hit(
                    HiddenElementCause::AriaHiddenFocusable,
                    &format!("hit-{i}"),
                    0,
                    "true",
                );
                h.selector = format!("body > div:nth-of-type({})", i + 1);
                h
            })
            .collect();
        let s = HiddenElementsSnapshot {
            page_url: "https://x.example/".into(),
            hits: many,
            scanned_elements: 10,
        };
        let f = detect_hidden_elements(&s);
        assert_eq!(f.len(), 1);
        // 10 hits but only the first 5 appear in the detail string.
        for i in 0..5 {
            assert!(f[0].detail.contains(&format!("hit-{i}")));
        }
        assert!(!f[0].detail.contains("hit-7"));
    }

    #[test]
    fn dom_capture_js_is_non_trivial() {
        // Sanity: the embedded JS should reference the load-bearing
        // browser APIs. Catches refactors that accidentally strip
        // the runtime hooks while keeping the Rust side compiling.
        assert!(HIDDEN_ELEMENTS_DOM_CAPTURE_JS.contains("getBoundingClientRect"));
        assert!(HIDDEN_ELEMENTS_DOM_CAPTURE_JS.contains("getComputedStyle"));
        assert!(HIDDEN_ELEMENTS_DOM_CAPTURE_JS.contains("aria-hidden"));
        assert!(HIDDEN_ELEMENTS_DOM_CAPTURE_JS.contains("tabindex"));
    }
}
