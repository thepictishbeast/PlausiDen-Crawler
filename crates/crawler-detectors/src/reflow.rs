//! `reflow` — WCAG 1.4.10 Reflow AA detector.
//!
//! WCAG 2.1 Success Criterion 1.4.10 (Reflow, AA): content can be
//! presented at 320 CSS pixels wide without requiring horizontal
//! scrolling, except for parts of the content that require a
//! two-dimensional layout for usage or meaning (tables, images,
//! maps, diagrams, video, presentations, data tables, toolbars
//! that have parallel toolbars in the same view).
//!
//! Distinct from `ui_overflow` — that detector finds content
//! clipped by its container. This detector finds the document-
//! level horizontal scroll bar at small viewports: the page itself
//! is wider than the viewport.
//!
//! HEURISTIC
//! ---------
//! The caller is responsible for setting the viewport to the test
//! width (typically 320 CSS px) BEFORE invoking this detector. We
//! observe whatever viewport is currently set and compare to the
//! document's scroll width.
//!
//! 1. Read `documentElement.scrollWidth` and `window.innerWidth`.
//! 2. If `scrollWidth > innerWidth + slack`, the page requires
//!    horizontal scrolling at this viewport.
//! 3. Walk visible elements and find those whose
//!    `getBoundingClientRect().right > innerWidth + slack`. These
//!    are the offenders contributing to the overflow.
//! 4. Skip WCAG-exempt elements:
//!    * `<table>` / descendants of `<table>` (data tables).
//!    * `<img>` / `<picture>` / `<svg>` / `<canvas>`.
//!    * `[role="img"]` / `[role="figure"]`.
//!    * `[data-loom-exempt-reflow]` — operator opt-out for the rare
//!      legitimate 2D-layout exception (e.g., a code diagram).
//! 5. Emit one strict finding listing up to 3 offenders.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const REFLOW_JS: &str = r##"(() => {
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
      return true;
    };

    // WCAG 1.4.10 exempts content that requires 2D layout for
    // usage or meaning. Implementation: tag-based exempt list +
    // an operator opt-out attribute for the rare legitimate case
    // not covered by tag heuristics.
    const isExempt = function(el) {
      const tag = el.tagName.toLowerCase();
      if (tag === 'table' || tag === 'img' || tag === 'picture' ||
          tag === 'svg' || tag === 'canvas' || tag === 'video') return true;
      const role = el.getAttribute('role');
      if (role === 'img' || role === 'figure' || role === 'table') return true;
      if (el.hasAttribute('data-loom-exempt-reflow')) return true;
      // Ancestor in an exempt subtree (e.g., a <td> inside a <table>).
      let p = el.parentElement;
      while (p && p !== document.body) {
        const pt = p.tagName.toLowerCase();
        if (pt === 'table' || pt === 'svg') return true;
        if (p.hasAttribute('data-loom-exempt-reflow')) return true;
        p = p.parentElement;
      }
      return false;
    };

    // Slack for sub-pixel rounding + scrollbar gutter.
    const SLACK = 1;
    const docEl = document.documentElement;
    const vpW = window.innerWidth;
    const scrollW = Math.max(docEl.scrollWidth, docEl.clientWidth || 0);
    const overflowed = scrollW > vpW + SLACK;

    const offenders = [];
    if (overflowed) {
      // Walk all elements under <body>, find those whose right edge
      // exceeds vpW. Cap scan + offender count for runtime safety.
      const SCAN_CAP = 5000;
      let scanned = 0;
      const stack = [document.body];
      const seen = new Set();
      while (stack.length > 0 && scanned < SCAN_CAP) {
        const el = stack.pop();
        if (!el || seen.has(el)) continue;
        seen.add(el);
        scanned += 1;
        // Descend regardless so we find the deepest offender.
        const kids = el.children;
        for (let i = 0; i < kids.length; i++) stack.push(kids[i]);
        if (el === document.body) continue;
        if (!isVisible(el)) continue;
        if (isExempt(el)) continue;
        const rect = el.getBoundingClientRect();
        if (rect.right > vpW + SLACK) {
          offenders.push({
            selector: selectorOf(el),
            tag: el.tagName.toLowerCase(),
            text: (el.textContent || '').trim().slice(0, 60),
            rightPx: Math.round(rect.right),
            widthPx: Math.round(rect.width),
            overflowPx: Math.round(rect.right - vpW)
          });
          if (offenders.length >= 20) break;
        }
      }
      // Sort by largest overflow first — the worst offender gets
      // surfaced in the finding detail.
      offenders.sort(function(a, b) { return b.overflowPx - a.overflowPx; });
    }

    return {
      vpW: vpW,
      vpH: window.innerHeight,
      scrollW: scrollW,
      overflowed: overflowed,
      offenders: offenders.slice(0, 10)
    };
})()"##;

/// One reflow offender — an element whose right edge exceeds the
/// viewport width.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ReflowOffender {
    /// Best-effort CSS selector.
    pub selector: String,
    /// `el.tagName.toLowerCase()`.
    pub tag: String,
    /// First 60 chars of textContent — operator-recognisable fingerprint.
    pub text: String,
    /// Right-edge X coordinate (CSS pixels, rounded).
    pub right_px: i32,
    /// Bounding-rect width.
    pub width_px: i32,
    /// How many pixels past the viewport this element extends.
    pub overflow_px: i32,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ReflowSnapshot {
    /// `window.innerWidth` at probe time. Caller is responsible for
    /// setting this to the test width (typically 320 px).
    #[serde(rename = "vpW")]
    pub vp_w: u32,
    /// `window.innerHeight`.
    #[serde(rename = "vpH")]
    pub vp_h: u32,
    /// `documentElement.scrollWidth`.
    pub scroll_w: u32,
    /// `scroll_w > vp_w + 1px slack`.
    pub overflowed: bool,
    /// Top 10 offenders sorted by largest overflow.
    pub offenders: Vec<ReflowOffender>,
}

/// Apply detection rules. Pure function. Emits one strict finding
/// per snapshot that overflowed.
#[must_use]
pub fn detect_reflow_issues(snap: &ReflowSnapshot) -> Vec<crate::AxisFinding> {
    if !snap.overflowed {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(1);
    let detail = if let Some(first) = snap.offenders.first() {
        format!(
            "Document requires horizontal scroll at {}px viewport (scrollWidth={}, overflow={}px). WCAG 2.1 SC 1.4.10 Reflow AA. Worst offender: <{}> \"{}\" extends to {}px (+{}px past viewport). {} non-exempt offender(s) total.",
            snap.vp_w,
            snap.scroll_w,
            snap.scroll_w as i32 - snap.vp_w as i32,
            first.tag,
            first.text,
            first.right_px,
            first.overflow_px,
            snap.offenders.len(),
        )
    } else {
        format!(
            "Document requires horizontal scroll at {}px viewport (scrollWidth={}, overflow={}px). WCAG 2.1 SC 1.4.10 Reflow AA. No non-exempt offender identified — overflow may come from a body / html margin or an exempt 2D-layout element extending past viewport.",
            snap.vp_w,
            snap.scroll_w,
            snap.scroll_w as i32 - snap.vp_w as i32,
        )
    };
    out.push(crate::AxisFinding {
        severity: crate::AxisSeverity::Strict,
        kind: "reflow.horizontal-scroll-required".to_owned(),
        detail,
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
            REFLOW_JS.matches('(').count(),
            REFLOW_JS.matches(')').count()
        );
        assert_eq!(
            REFLOW_JS.matches('{').count(),
            REFLOW_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(REFLOW_JS.starts_with("(() => {"));
        assert!(REFLOW_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "vpW",
            "vpH",
            "scrollW",
            "overflowed",
            "offenders",
            "selector",
            "tag",
            "text",
            "rightPx",
            "widthPx",
            "overflowPx",
        ] {
            assert!(REFLOW_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_implements_wcag_exempt_list() {
        // Exempt tags from WCAG 1.4.10.
        for tag in ["table", "img", "picture", "svg", "canvas", "video"] {
            assert!(
                REFLOW_JS.contains(&format!("=== '{tag}'")),
                "missing exempt tag in JS: {tag}"
            );
        }
        // Operator opt-out attribute.
        assert!(REFLOW_JS.contains("data-loom-exempt-reflow"));
        // Role-based exempts.
        assert!(REFLOW_JS.contains("=== 'img'"));
        assert!(REFLOW_JS.contains("=== 'figure'"));
    }

    #[test]
    fn no_overflow_no_finding() {
        let snap = ReflowSnapshot {
            vp_w: 320,
            vp_h: 800,
            scroll_w: 320,
            overflowed: false,
            offenders: vec![],
        };
        let findings = detect_reflow_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn overflowed_with_offender_emits_strict() {
        let snap = ReflowSnapshot {
            vp_w: 320,
            vp_h: 800,
            scroll_w: 540,
            overflowed: true,
            offenders: vec![ReflowOffender {
                selector: "body > div > pre".to_owned(),
                tag: "pre".to_owned(),
                text: "very long unwrappable string".to_owned(),
                right_px: 540,
                width_px: 540,
                overflow_px: 220,
            }],
        };
        let findings = detect_reflow_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "reflow.horizontal-scroll-required");
        assert!(findings[0].detail.contains("320px viewport"));
        assert!(findings[0].detail.contains("WCAG 2.1 SC 1.4.10"));
        assert!(findings[0].detail.contains("<pre>"));
        assert!(findings[0].detail.contains("very long unwrappable"));
        assert!(findings[0].detail.contains("+220px"));
        assert!(findings[0].detail.contains("1 non-exempt offender"));
    }

    #[test]
    fn overflowed_no_offender_explains_likely_cause() {
        // When the document overflows but no non-exempt element is the
        // direct cause, the finding should suggest the likely source
        // (margin / exempt-element overhang) instead of silently
        // emitting a useless message.
        let snap = ReflowSnapshot {
            vp_w: 320,
            vp_h: 800,
            scroll_w: 360,
            overflowed: true,
            offenders: vec![],
        };
        let findings = detect_reflow_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("No non-exempt offender"));
        assert!(findings[0].detail.contains("margin"));
    }

    #[test]
    fn multiple_offenders_count_surfaces() {
        let mut offenders = Vec::new();
        for i in 0..5 {
            offenders.push(ReflowOffender {
                selector: format!("o[{i}]"),
                tag: "div".to_owned(),
                text: format!("offender {i}"),
                right_px: 400 + i,
                width_px: 400,
                overflow_px: 80 + i,
            });
        }
        let snap = ReflowSnapshot {
            vp_w: 320,
            vp_h: 800,
            scroll_w: 405,
            overflowed: true,
            offenders,
        };
        let findings = detect_reflow_issues(&snap);
        assert!(findings[0].detail.contains("5 non-exempt offender"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = ReflowSnapshot {
            vp_w: 320,
            vp_h: 800,
            scroll_w: 540,
            overflowed: true,
            offenders: vec![ReflowOffender {
                selector: "x".to_owned(),
                tag: "div".to_owned(),
                text: "rt".to_owned(),
                right_px: 540,
                width_px: 540,
                overflow_px: 220,
            }],
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"vpW\":320"));
        assert!(json.contains("\"overflowed\":true"));
        let back: ReflowSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.vp_w, 320);
        assert!(back.overflowed);
        assert_eq!(back.offenders.len(), 1);
        assert_eq!(back.offenders[0].overflow_px, 220);
    }

    #[test]
    fn detail_includes_scroll_width_and_overflow_pixel_count() {
        let snap = ReflowSnapshot {
            vp_w: 320,
            vp_h: 800,
            scroll_w: 1280,
            overflowed: true,
            offenders: vec![ReflowOffender {
                selector: "x".to_owned(),
                tag: "main".to_owned(),
                text: "x".to_owned(),
                right_px: 1280,
                width_px: 1280,
                overflow_px: 960,
            }],
        };
        let findings = detect_reflow_issues(&snap);
        // scrollWidth and overflow size both surface.
        assert!(findings[0].detail.contains("scrollWidth=1280"));
        assert!(findings[0].detail.contains("overflow=960px"));
    }
}
