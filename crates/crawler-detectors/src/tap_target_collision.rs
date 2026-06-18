//! `tap_target_collision` — flags pairs of touch targets whose
//! bounding boxes sit too close together to be tapped reliably
//! on a touch device.
//!
//! Complements [`crate::tap_targets`] which audits individual
//! target SIZE (≥24×24 / ≥44×44 CSS px). This axis audits the
//! SPACING BETWEEN targets — two ≥44px buttons separated by 2px
//! still cause thumb-collision misfires.
//!
//! WCAG 2.5.8 ("Target Size Minimum", AA) explicitly admits an
//! "equivalent target" exception when targets are tightly
//! packed; the practical user-facing failure is still real and
//! costs taps. Apple HIG + Material Design both recommend ≥8px
//! gap between interactive elements.
//!
//! Defect class: visual designs put two link/button targets
//! flush against each other (icon row, nav strip, button group
//! with no internal gap). The two targets each meet the size
//! floor individually but the thumb tap zone overlaps so the
//! wrong one fires ~30% of the time on real devices.
//!
//! ## Heuristic
//!
//! Caller emulates a narrow viewport (e.g. 390px) before
//! capture — the collision class only matters on touch
//! devices, and a desktop-width capture would let normally-
//! adjacent elements drift apart. The JS walks every
//! `<a>`, `<button>`, `<input type=button|submit|reset>`,
//! `<summary>`, `[role="button"]`, `[role="link"]`, capping at
//! `MAX_TARGETS` (200) to keep the O(n²) pair walk bounded.
//!
//! For each pair, compute the closest-edge distance in CSS px
//! using a standard rect-distance formula (zero when rects
//! overlap, positive otherwise). Bucket as Strict / Warn:
//!
//! * **Strict** — `distance ≤ STRICT_DISTANCE_PX` (4 px).
//!   Effectively flush; thumb taps will misfire constantly.
//! * **Warn** — `distance ≤ WARN_DISTANCE_PX` (8 px). Apple
//!   HIG / Material recommend ≥8 px gap; below that, accuracy
//!   degrades on small thumbs.
//!
//! Skip pairs sharing a common ancestor with `role="tablist"` /
//! `role="radiogroup"` — those are semantically a single group
//! where adjacency is the desired pattern (tab strips are
//! supposed to be flush).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending pair — two tap targets sitting
/// within the spacing floor of each other.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TapTargetCollisionHit {
    /// CSS-ish path of the first target.
    pub selector_a: String,
    /// Best-effort role label (tag or aria role).
    pub role_a: String,
    /// Visible accessible-name guess for target A (capped at
    /// 40 chars), for example context in the finding detail.
    pub label_a: String,
    /// CSS-ish path of the second target.
    pub selector_b: String,
    /// Best-effort role label for B.
    pub role_b: String,
    /// Visible accessible-name guess for B (capped at 40 chars).
    pub label_b: String,
    /// Closest-edge distance in CSS px (0 when rects overlap).
    pub distance_px: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TapTargetCollisionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px). Touch-relevant
    /// runs typically emulate 390 px.
    pub viewport_width: u32,
    /// Pairs whose distance ≤ `WARN_DISTANCE_PX`.
    pub hits: Vec<TapTargetCollisionHit>,
    /// Total tap targets the JS considered (after capping at
    /// `MAX_TARGETS`).
    pub scanned_targets: u32,
}

/// Distance at or below which a pair is Strict.
pub const STRICT_DISTANCE_PX: u32 = 4;

/// Distance at or below which a pair is Warn.
pub const WARN_DISTANCE_PX: u32 = 8;

/// Max number of tap targets the JS considers — caps the
/// pair walk at `MAX_TARGETS²/2` comparisons. Pages with more
/// real targets are unusual and the first 200 are typically
/// enough to surface the design issue.
pub const MAX_TARGETS: u32 = 200;

/// Max examples reported per finding to keep reports readable.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
///
/// Splits hits into Strict and Warn buckets per the documented
/// thresholds. Empty hits → empty findings.
#[must_use]
pub fn detect_tap_target_collision(snap: &TapTargetCollisionSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut strict: Vec<&TapTargetCollisionHit> = Vec::new();
    let mut warn: Vec<&TapTargetCollisionHit> = Vec::new();
    for h in &snap.hits {
        if h.distance_px <= STRICT_DISTANCE_PX {
            strict.push(h);
        } else if h.distance_px <= WARN_DISTANCE_PX {
            warn.push(h);
        }
    }

    let format_example = |h: &TapTargetCollisionHit| -> String {
        let a = if h.label_a.is_empty() {
            h.selector_a.clone()
        } else {
            format!("{} \"{}\"", h.selector_a, h.label_a)
        };
        let b = if h.label_b.is_empty() {
            h.selector_b.clone()
        } else {
            format!("{} \"{}\"", h.selector_b, h.label_b)
        };
        format!("[{}] vs [{}] @ {}px", a, b, h.distance_px)
    };

    let mut out = Vec::new();
    if !strict.is_empty() {
        let examples: Vec<String> = strict.iter().take(MAX_EXAMPLES).map(|h| format_example(h)).collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "tap-target.collision-flush".to_owned(),
            detail: format!(
                "{} tap-target pair(s) within {}px of each other at viewport {}px — thumb taps will misfire onto the wrong target. Add at least an 8px gap (Apple HIG / Material Design baseline). Examples: {}",
                strict.len(),
                STRICT_DISTANCE_PX,
                snap.viewport_width,
                examples.join("; ")
            ),
        });
    }
    if !warn.is_empty() {
        let examples: Vec<String> = warn.iter().take(MAX_EXAMPLES).map(|h| format_example(h)).collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tap-target.collision-cramped".to_owned(),
            detail: format!(
                "{} tap-target pair(s) within {}px at viewport {}px — accuracy degrades on small thumbs. Apple HIG / Material Design recommend ≥8px gap. Examples: {}",
                warn.len(),
                WARN_DISTANCE_PX,
                snap.viewport_width,
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Mirror any change in this
/// file's `TapTargetCollisionHit` + snapshot fields.
///
/// Caller should emulate a touch viewport (e.g. 390 px width)
/// before invoking — this detector's value is touch-device-
/// specific spacing, not desktop spacing.
pub const TAP_TARGET_COLLISION_DOM_CAPTURE_JS: &str = r#"
(() => {
    const SELECTOR = 'a, button, summary, input[type="button"], input[type="submit"], input[type="reset"], [role="button"], [role="link"]';
    const MAX_TARGETS = 200;
    const WARN_DISTANCE_PX = 8;

    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
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

    // Returns true iff a + b share an ancestor with role tablist
    // / radiogroup — adjacency is desired for those groups.
    const inSamePackedGroup = function(a, b) {
      const PACKED = new Set(['tablist', 'radiogroup', 'menubar', 'group']);
      const ancestorsOf = function(el) {
        const out = new Set();
        let p = el;
        let d = 0;
        while (p && d < 8) {
          out.add(p);
          p = p.parentElement;
          d += 1;
        }
        return out;
      };
      const aa = ancestorsOf(a);
      let p = b;
      let d = 0;
      while (p && d < 8) {
        if (aa.has(p)) {
          const role = p.getAttribute ? p.getAttribute('role') : null;
          if (role && PACKED.has(role)) return true;
        }
        p = p.parentElement;
        d += 1;
      }
      return false;
    };

    const labelOf = function(el) {
      const aria = el.getAttribute && el.getAttribute('aria-label');
      if (aria) return aria.trim().substring(0, 40);
      const t = (el.textContent || '').trim();
      if (t) return t.substring(0, 40);
      const alt = el.querySelector ? el.querySelector('img[alt]') : null;
      if (alt) return (alt.getAttribute('alt') || '').trim().substring(0, 40);
      return '';
    };

    const roleOf = function(el) {
      const r = el.getAttribute && el.getAttribute('role');
      if (r) return r;
      const tag = el.tagName.toLowerCase();
      if (tag === 'a') return 'link';
      if (tag === 'button' || tag === 'summary') return tag;
      return tag;
    };

    const all = Array.from(document.querySelectorAll(SELECTOR)).slice(0, MAX_TARGETS);
    const visible = [];
    for (const el of all) {
      const r = el.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) continue;
      const cs = window.getComputedStyle(el);
      if (cs.visibility === 'hidden' || cs.display === 'none' || parseFloat(cs.opacity || '1') === 0) continue;
      visible.push({ el: el, rect: r });
    }

    const hits = [];
    for (let i = 0; i < visible.length; i += 1) {
      const a = visible[i];
      for (let j = i + 1; j < visible.length; j += 1) {
        const b = visible[j];
        // Closest-edge distance on axis-aligned rectangles.
        let dx = 0;
        if (b.rect.left > a.rect.right) dx = b.rect.left - a.rect.right;
        else if (a.rect.left > b.rect.right) dx = a.rect.left - b.rect.right;
        let dy = 0;
        if (b.rect.top > a.rect.bottom) dy = b.rect.top - a.rect.bottom;
        else if (a.rect.top > b.rect.bottom) dy = a.rect.top - b.rect.bottom;
        // Chebyshev distance approximation — picking the smaller of
        // dx / dy when one rect is offset on both axes (corner
        // touch). For most UI cases dx OR dy is 0 so this resolves
        // to the obvious axial gap.
        const dist = (dx === 0 || dy === 0) ? Math.max(dx, dy) : Math.min(dx, dy);
        if (dist > WARN_DISTANCE_PX) continue;
        if (inSamePackedGroup(a.el, b.el)) continue;
        hits.push({
          selectorA: selectorOf(a.el),
          roleA: roleOf(a.el),
          labelA: labelOf(a.el),
          selectorB: selectorOf(b.el),
          roleB: roleOf(b.el),
          labelB: labelOf(b.el),
          distancePx: Math.round(dist)
        });
      }
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedTargets: visible.length
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(a: &str, b: &str, distance_px: u32) -> TapTargetCollisionHit {
        TapTargetCollisionHit {
            selector_a: a.into(),
            role_a: "button".into(),
            label_a: String::new(),
            selector_b: b.into(),
            role_b: "link".into(),
            label_b: String::new(),
            distance_px,
        }
    }

    fn snap(hits: Vec<TapTargetCollisionHit>) -> TapTargetCollisionSnapshot {
        TapTargetCollisionSnapshot {
            page_url: "https://x".into(),
            viewport_width: 390,
            hits,
            scanned_targets: 50,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_tap_target_collision(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn flush_pair_zero_distance_is_strict() {
        let s = snap(vec![hit(".a", ".b", 0)]);
        let findings = detect_tap_target_collision(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "tap-target.collision-flush");
        assert!(findings[0].detail.contains(".a"));
        assert!(findings[0].detail.contains(".b"));
    }

    #[test]
    fn near_flush_within_strict_threshold_is_strict() {
        let s = snap(vec![hit(".a", ".b", STRICT_DISTANCE_PX)]);
        let findings = detect_tap_target_collision(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn five_pixel_gap_is_warn() {
        let s = snap(vec![hit(".a", ".b", 5)]);
        let findings = detect_tap_target_collision(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "tap-target.collision-cramped");
    }

    #[test]
    fn beyond_warn_threshold_skipped() {
        // 9 px gap > WARN_DISTANCE_PX (8 px) — no finding.
        let s = snap(vec![hit(".a", ".b", 9)]);
        let findings = detect_tap_target_collision(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(vec![
            hit(".a", ".b", 0),
            hit(".c", ".d", 5),
            hit(".e", ".f", 7),
        ]);
        let findings = detect_tap_target_collision(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"tap-target.collision-flush"));
        assert!(kinds.contains(&"tap-target.collision-cramped"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit(".prev", ".next", 0);
        h.label_a = "Previous".into();
        h.label_b = "Next".into();
        let s = snap(vec![h]);
        let findings = detect_tap_target_collision(&s);
        assert_eq!(findings.len(), 1);
        // Labels appear in quotes inside the example string.
        assert!(findings[0].detail.contains(r#"\"Previous\""#) || findings[0].detail.contains("\"Previous\""));
        assert!(findings[0].detail.contains("\"Next\""));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".a-{i}"), &format!(".b-{i}"), 0));
        }
        let s = snap(hits);
        let findings = detect_tap_target_collision(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 tap-target pair(s)"));
        // 5 examples joined by "; " → 4 "; " separators.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: the JS body returns the documented shape.
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("hits"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("scannedTargets"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("distancePx"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("selectorA"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("selectorB"));
        // Selector contract — covers anchors, buttons, summaries, role-buttons.
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("'a, button, summary"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("[role=\"button\"]"));
        // Packed-group exemption contract.
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("tablist"));
        assert!(TAP_TARGET_COLLISION_DOM_CAPTURE_JS.contains("radiogroup"));
    }
}
