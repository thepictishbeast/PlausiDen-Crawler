//! `reduced_motion_compliance` — flags elements that animate even
//! when the page has been rendered under `prefers-reduced-motion:
//! reduce`. WCAG 2.3.3 ("Animation from interactions") + 2.2.2
//! ("Pause, stop, hide") put a hard accessibility floor under
//! continuous / vestibular-trigger motion; a page that ignores
//! the OS-level reduce preference fails users who configured it.
//!
//! Defect class: a CSS animation / transition keeps running even
//! when `@media (prefers-reduced-motion: reduce)` should have
//! disabled or shortened it. Common causes:
//!
//! 1. Hand-authored CSS that never wrapped its `@keyframes` use
//!    in a reduced-motion media query.
//! 2. A third-party JS animation library (GSAP / Framer Motion
//!    / Lottie) that doesn't respect the media query.
//! 3. Inline `style="animation: ..."` set by JS at runtime
//!    bypassing the CSS gate entirely.
//! 4. Looped video / GIF with no playback control while the user
//!    has reduce set.
//!
//! ## Heuristic
//!
//! Caller emulates `prefers-reduced-motion: reduce` (via
//! `Emulation.setEmulatedMedia` over CDP), waits for styles to
//! settle, then walks every element. For each visible element
//! whose computed style declares any of:
//!
//! * `animation-name` not in {`none`, empty}
//! * `animation-duration` > `MIN_DURATION_MS`
//! * `transition-duration` > `MIN_DURATION_MS`
//!
//! …the snapshot captures one `ReducedMotionHit`. Skip elements
//! marked `aria-hidden="true"` or `role="presentation"` so legit
//! decorative motion (which won't reach users with reduce set
//! anyway because aria-hidden screen-reader users don't see it)
//! doesn't trigger.
//!
//! Severity tiers:
//!
//! * **Strict** — `animation-iteration-count: infinite` OR
//!   `animation-duration > STRICT_DURATION_MS`. Continuous /
//!   long-running motion is the worst vestibular trigger.
//! * **Warn** — any other animation or transition exceeding
//!   `MIN_DURATION_MS`. Short transitions on hover / focus are
//!   common UX patterns; reduce-motion users still benefit from
//!   them being flagged but they're not gate-blocking.
//!
//! When the caller did NOT emulate reduce (i.e. the snapshot's
//! `reduced_motion_emulated` is `false`), the detector returns
//! an empty Vec — no false positives from unconfigured runs.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — an element animating under reduced-
/// motion emulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReducedMotionHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped at 80 chars) for context.
    pub text: String,
    /// `animation-name` computed value (e.g. `"spin"`, `"none"`).
    pub animation_name: String,
    /// `animation-duration` in milliseconds (sum if comma-list).
    pub animation_duration_ms: u32,
    /// True iff `animation-iteration-count: infinite` is set.
    pub animation_infinite: bool,
    /// `transition-duration` in milliseconds (sum if comma-list).
    pub transition_duration_ms: u32,
}

/// Captured page state the detector consumes.
///
/// `reduced_motion_emulated` is the contract that lets the
/// detector know the caller set up the emulation before
/// capturing — otherwise the entire run is meaningless and the
/// detector returns no findings (avoids a false-positive flood
/// from real animations the user opted into).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReducedMotionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// True iff caller set the emulated media query to
    /// `prefers-reduced-motion: reduce` before capture.
    pub reduced_motion_emulated: bool,
    /// Every element whose computed style indicates motion under
    /// the emulated query.
    pub hits: Vec<ReducedMotionHit>,
    /// Total elements walked.
    pub scanned_elements: u32,
}

/// Minimum animation / transition duration to consider. Anything
/// at or below this threshold is assumed to be an instantaneous
/// state change (e.g. focus-ring fade-in) rather than motion.
pub const MIN_DURATION_MS: u32 = 100;

/// Duration above which a single animation is treated as Strict.
/// Long-running motion is the worst vestibular trigger.
pub const STRICT_DURATION_MS: u32 = 1000;

/// Maximum number of examples reported per finding to keep
/// reports readable.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
///
/// Returns empty when emulation was off (sanity gate) or when
/// no hits were captured. Findings are split into Strict
/// (infinite / long animations) and Warn (everything else).
///
/// Examples are capped at `MAX_EXAMPLES` per finding; the
/// finding's `detail` always reports the total hit count even
/// when truncating.
#[must_use]
pub fn detect_reduced_motion_violations(snap: &ReducedMotionSnapshot) -> Vec<AxisFinding> {
    if !snap.reduced_motion_emulated {
        return Vec::new();
    }
    if snap.hits.is_empty() {
        return Vec::new();
    }

    let mut severe: Vec<&ReducedMotionHit> = Vec::new();
    let mut warn: Vec<&ReducedMotionHit> = Vec::new();

    for hit in &snap.hits {
        let runs_motion = hit.animation_name != "none"
            && !hit.animation_name.is_empty()
            && hit.animation_duration_ms > MIN_DURATION_MS;
        let has_transition = hit.transition_duration_ms > MIN_DURATION_MS;
        if !runs_motion && !has_transition {
            continue;
        }
        let strict = (runs_motion
            && (hit.animation_infinite || hit.animation_duration_ms > STRICT_DURATION_MS));
        if strict {
            severe.push(hit);
        } else {
            warn.push(hit);
        }
    }

    let mut out = Vec::new();

    if !severe.is_empty() {
        let examples: Vec<String> = severe
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                format!(
                    "{} (\"{}\" · animation={} {}ms{})",
                    h.selector,
                    h.text,
                    h.animation_name,
                    h.animation_duration_ms,
                    if h.animation_infinite {
                        " · infinite"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "reduced-motion.continuous".to_owned(),
            detail: format!(
                "{} element(s) running continuous / long-duration motion under prefers-reduced-motion:reduce at viewport {}px — fails WCAG 2.3.3. Wrap @keyframes use in a prefers-reduced-motion media query with `animation: none !important` or pause infinite animations. Examples: {}",
                severe.len(),
                snap.viewport_width,
                examples.join("; ")
            ),
        });
    }

    if !warn.is_empty() {
        let examples: Vec<String> = warn
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                if h.transition_duration_ms > MIN_DURATION_MS && h.animation_duration_ms <= MIN_DURATION_MS {
                    format!(
                        "{} (\"{}\" · transition {}ms)",
                        h.selector, h.text, h.transition_duration_ms
                    )
                } else {
                    format!(
                        "{} (\"{}\" · animation={} {}ms)",
                        h.selector, h.text, h.animation_name, h.animation_duration_ms
                    )
                }
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "reduced-motion.short-motion".to_owned(),
            detail: format!(
                "{} element(s) with non-trivial motion ({}ms+) under prefers-reduced-motion:reduce. Short transitions on hover/focus are common but should still be shortened or removed for reduce users. Examples: {}",
                warn.len(),
                MIN_DURATION_MS,
                examples.join("; ")
            ),
        });
    }

    out
}

/// Browser-side DOM-capture script. Caller MUST set
/// `Emulation.setEmulatedMedia` with
/// `{name: "prefers-reduced-motion", value: "reduce"}` before
/// invoking this script; the script trusts the caller and sets
/// `reduced_motion_emulated: true` in the returned snapshot so
/// the Rust detector knows it can act on the hits.
///
/// Mirror any change in this file's `ReducedMotionHit` +
/// `ReducedMotionSnapshot` fields.
pub const REDUCED_MOTION_DOM_CAPTURE_JS: &str = r#"
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

    const isExcluded = function(el) {
      if (!el || el.getAttribute == null) return true;
      if (el.getAttribute('aria-hidden') === 'true') return true;
      if (el.getAttribute('role') === 'presentation') return true;
      return false;
    };

    // Parse a `1s, 250ms` style duration list into total ms.
    const parseDurationList = function(value) {
      if (!value) return 0;
      let total = 0;
      const parts = value.split(',');
      for (const raw of parts) {
        const v = raw.trim();
        if (!v) continue;
        if (v.endsWith('ms')) {
          const n = parseFloat(v.slice(0, -2));
          if (Number.isFinite(n)) total += n;
        } else if (v.endsWith('s')) {
          const n = parseFloat(v.slice(0, -1));
          if (Number.isFinite(n)) total += n * 1000;
        }
      }
      return Math.round(total);
    };

    const MIN_DURATION_MS = 100;
    const hits = [];
    let scanned = 0;
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    let node = walk.currentNode;
    while (node) {
      if (node.nodeType === 1 && !isExcluded(node)) {
        scanned += 1;
        const cs = window.getComputedStyle(node);
        const animationName = (cs.animationName || 'none').trim();
        const animationDuration = parseDurationList(cs.animationDuration);
        const transitionDuration = parseDurationList(cs.transitionDuration);
        const iterRaw = (cs.animationIterationCount || '').trim();
        const animationInfinite = iterRaw === 'infinite' || iterRaw.split(',').map(function(s){return s.trim();}).indexOf('infinite') !== -1;
        const runs = animationName !== 'none' && animationName !== '' && animationDuration > MIN_DURATION_MS;
        const transitions = transitionDuration > MIN_DURATION_MS;
        if (runs || transitions) {
          const text = (node.textContent || '').trim().substring(0, 80);
          hits.push({
            selector: selectorOf(node),
            text: text,
            animationName: animationName,
            animationDurationMs: animationDuration,
            animationInfinite: animationInfinite,
            transitionDurationMs: transitionDuration
          });
        }
      }
      node = walk.nextNode();
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      reducedMotionEmulated: true,
      hits: hits,
      scannedElements: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, name: &str, anim_ms: u32, infinite: bool, trans_ms: u32) -> ReducedMotionHit {
        ReducedMotionHit {
            selector: selector.into(),
            text: "X".into(),
            animation_name: name.into(),
            animation_duration_ms: anim_ms,
            animation_infinite: infinite,
            transition_duration_ms: trans_ms,
        }
    }

    fn snap(emulated: bool, hits: Vec<ReducedMotionHit>) -> ReducedMotionSnapshot {
        ReducedMotionSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            reduced_motion_emulated: emulated,
            hits,
            scanned_elements: 100,
        }
    }

    #[test]
    fn emulation_off_returns_no_findings() {
        // Sanity gate: hits present but emulation off → empty.
        // Otherwise a baseline run (without emulation) would
        // flag legit user-requested animations as violations.
        let s = snap(false, vec![hit(".bad", "spin", 5000, true, 0)]);
        let findings = detect_reduced_motion_violations(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn empty_hits_returns_no_findings() {
        let s = snap(true, vec![]);
        let findings = detect_reduced_motion_violations(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn infinite_animation_is_strict() {
        let s = snap(
            true,
            vec![hit(".loader", "spin", 800, true, 0)],
        );
        let findings = detect_reduced_motion_violations(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "reduced-motion.continuous");
        assert!(findings[0].detail.contains(".loader"));
        assert!(findings[0].detail.contains("infinite"));
    }

    #[test]
    fn long_finite_animation_is_strict() {
        // 2-second animation, finite — still strict because it
        // exceeds STRICT_DURATION_MS.
        let s = snap(
            true,
            vec![hit(".banner", "slide-in", 2000, false, 0)],
        );
        let findings = detect_reduced_motion_violations(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn short_animation_is_warn() {
        // 300ms animation, finite — warn tier.
        let s = snap(
            true,
            vec![hit(".chip", "pop", 300, false, 0)],
        );
        let findings = detect_reduced_motion_violations(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "reduced-motion.short-motion");
    }

    #[test]
    fn transition_only_is_warn() {
        // Pure transition over 100ms with no animation → warn.
        let s = snap(
            true,
            vec![hit(".hover", "none", 0, false, 250)],
        );
        let findings = detect_reduced_motion_violations(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert!(findings[0].detail.contains("transition 250ms"));
    }

    #[test]
    fn below_threshold_motion_skipped() {
        // 50ms animation + 50ms transition → both below
        // MIN_DURATION_MS, not flagged.
        let s = snap(
            true,
            vec![hit(".focus-ring", "ring", 50, false, 50)],
        );
        let findings = detect_reduced_motion_violations(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(
            true,
            vec![
                hit(".spinner", "spin", 1500, true, 0),
                hit(".btn", "none", 0, false, 200),
                hit(".tooltip", "fade", 400, false, 0),
            ],
        );
        let findings = detect_reduced_motion_violations(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"reduced-motion.continuous"));
        assert!(kinds.contains(&"reduced-motion.short-motion"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".bad-{i}"),
                "spin",
                2000,
                true,
                0,
            ));
        }
        let s = snap(true, hits);
        let findings = detect_reduced_motion_violations(&s);
        assert_eq!(findings.len(), 1);
        // Total count reported.
        assert!(findings[0].detail.contains("10 element(s)"));
        // Examples capped at MAX_EXAMPLES (5); joined by "; " separator
        // → 4 semicolons between 5 items.
        let semicolons = findings[0].detail.matches(';').count();
        assert_eq!(semicolons, 4, "5 examples joined by 4 semicolons");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: the JS body is a self-invoking function returning a
        // shape with the documented fields. Assert syntactic
        // signatures only — no JS execution in unit tests.
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("reducedMotionEmulated"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("hits"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("scannedElements"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("animationName"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("animationDurationMs"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("transitionDurationMs"));
        assert!(REDUCED_MOTION_DOM_CAPTURE_JS.contains("animationInfinite"));
    }
}
