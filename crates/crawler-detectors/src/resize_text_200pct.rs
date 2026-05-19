//! `resize_text_200pct` — verify the page remains functional at
//! 200% zoom.
//!
//! WCAG 2.1 SC 1.4.4 (Resize text, Level AA). Users must be able
//! to scale text to 200% without loss of content or function.
//!
//! ## Heuristic
//!
//! Snapshot:
//!
//! 1. Counts visible horizontally-overflowing elements at 100%
//!    zoom (baseline).
//! 2. Applies `document.documentElement.style.zoom = '200%'`,
//!    settles for ~50ms, then re-counts horizontally-overflowing
//!    elements.
//! 3. Restores zoom to default.
//!
//! Classifier emits one finding per element that overflows at
//! 200% but did NOT at 100% — those are layouts that broke
//! solely due to the zoom.
//!
//! ## Severity
//!
//! Strict. SC 1.4.4 is Level AA; layout-fragility at 200% is a
//! hard fail.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — an element that overflows at 200%
/// zoom but didn't at 100%.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ResizeOverflowHit {
    /// Selector path.
    pub selector: String,
    /// Text snippet for context.
    pub text_preview: String,
    /// Rect width at 200% zoom.
    pub width_at_200: u32,
    /// Viewport width at capture time.
    pub viewport_width: u32,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ResizeText200pctSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at 100% zoom.
    pub viewport_width: u32,
    /// Elements that overflowed at 200% but not at 100%.
    pub new_overflowers: Vec<ResizeOverflowHit>,
    /// Total visible elements walked.
    pub scanned: u32,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_resize_text_200pct(snap: &ResizeText200pctSnapshot) -> Vec<AxisFinding> {
    if snap.new_overflowers.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .new_overflowers
        .iter()
        .take(5)
        .map(|h| {
            format!(
                "{} (\"{}\", {}px wide)",
                h.selector, h.text_preview, h.width_at_200
            )
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Strict,
        kind: "resize-text.200pct-overflow".to_owned(),
        detail: format!(
            "WCAG 1.4.4 — {} element(s) overflow the viewport at 200% zoom but not at 100%. Layouts must remain functional at 200% zoom; the page-level horizontal scroll caused by these elements blocks readers who rely on browser-zoom. Examples: {}",
            snap.new_overflowers.len(),
            examples.join("; ")
        ),
    }]
}

/// Browser-side capture. Sets zoom on `<html>`, lets layout
/// settle, counts overflowers, restores zoom.
pub const RESIZE_TEXT_200PCT_DOM_CAPTURE_JS: &str = r#"
(async () => {
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

    const collectOverflowers = function(vpW) {
      const offenders = new Set();
      const all = document.querySelectorAll('body *');
      for (let i = 0; i < all.length; i++) {
        const el = all[i];
        const rect = el.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) continue;
        // Element extends past the viewport's right edge.
        if (rect.right > vpW + 4 && rect.left >= -100) {
          const parent = el.parentElement;
          const cs = parent ? window.getComputedStyle(parent) : null;
          const ox = cs ? cs.overflowX : 'visible';
          if (ox !== 'auto' && ox !== 'scroll' && ox !== 'hidden') {
            offenders.add(selectorOf(el));
          }
        }
      }
      return offenders;
    };

    const vp100 = window.innerWidth;
    const baseline = collectOverflowers(vp100);
    const docEl = document.documentElement;
    const priorZoom = docEl.style.zoom;
    docEl.style.zoom = '200%';
    // Settle: one rAF tick is enough for layout reflow.
    await new Promise(function(r) { requestAnimationFrame(function() { setTimeout(r, 50); }); });
    const vp200 = window.innerWidth;
    const zoomed = collectOverflowers(vp200);
    docEl.style.zoom = priorZoom || '';

    const newHits = [];
    let scanned = 0;
    const all = document.querySelectorAll('body *');
    zoomed.forEach(function(sel) {
      if (baseline.has(sel)) return;
      // Re-resolve the element to capture text/width.
      const probe = document.querySelectorAll(sel.replace(/^body > /, 'body > '));
      const el = probe[0];
      if (!el) return;
      scanned += 1;
      const rect = el.getBoundingClientRect();
      const text = (el.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 60);
      newHits.push({
        selector: sel,
        textPreview: text,
        widthAt200: Math.round(rect.width),
        viewportWidth: vp100,
      });
      if (newHits.length >= 50) return;
    });

    return {
      pageUrl: window.location.href,
      viewportWidth: vp100,
      newOverflowers: newHits,
      scanned: all.length,
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(new: Vec<ResizeOverflowHit>) -> ResizeText200pctSnapshot {
        ResizeText200pctSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            viewport_width: 1280,
            new_overflowers: new,
            scanned: 100,
        }
    }

    fn hit(selector: &str, text: &str, width: u32) -> ResizeOverflowHit {
        ResizeOverflowHit {
            selector: selector.to_owned(),
            text_preview: text.to_owned(),
            width_at_200: width,
            viewport_width: 1280,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_resize_text_200pct(&s).is_empty());
    }

    #[test]
    fn one_hit_strict() {
        let s = snap(vec![hit("body > div > h1", "Long title", 1800)]);
        let f = detect_resize_text_200pct(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert_eq!(f[0].kind, "resize-text.200pct-overflow");
        assert!(f[0].detail.contains("WCAG 1.4.4"));
    }

    #[test]
    fn multiple_hits_aggregate() {
        let s = snap(vec![
            hit("body > h1", "T", 1500),
            hit("body > nav", "N", 1700),
        ]);
        let f = detect_resize_text_200pct(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("2 element"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!("body > div:nth-of-type({})", i + 1),
                "T",
                1500,
            ));
        }
        let s = snap(hits);
        let f = detect_resize_text_200pct(&s);
        assert!(f[0].detail.contains("10 element"));
        let arrows = f[0].detail.matches(" (\"").count();
        assert_eq!(arrows, 5);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![hit("body > p", "P", 1500)]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: ResizeText200pctSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.new_overflowers[0].width_at_200, 1500);
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in RESIZE_TEXT_200PCT_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }

    #[test]
    fn js_includes_zoom_restore() {
        // Guarantee the JS restores zoom after measurement; otherwise
        // subsequent detectors run against a zoomed page.
        assert!(
            RESIZE_TEXT_200PCT_DOM_CAPTURE_JS.contains("docEl.style.zoom = priorZoom"),
            "capture JS missing zoom-restore"
        );
    }
}
