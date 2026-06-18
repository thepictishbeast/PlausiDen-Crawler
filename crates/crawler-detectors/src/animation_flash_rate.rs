//! `animation_flash_rate` — flags elements with CSS animations
//! that complete > 3 cycles per second (the WCAG 2.3.1
//! photosensitive-epilepsy threshold).
//!
//! WCAG 2.1 SC 2.3.1 ("Three Flashes or Below Threshold",
//! Level A): "Web pages do not contain anything that flashes
//! more than three times in any one second period." Failing
//! this can trigger seizures in users with photosensitive
//! epilepsy.
//!
//! Defect class: a CSS animation with very short duration +
//! infinite (or many) iterations effectively flashes. Common
//! shapes:
//!
//! 1. `@keyframes blink { 0%, 50% { opacity: 0; } 51%, 100% {
//!    opacity: 1; } }` with `animation-duration: 200ms;
//!    animation-iteration-count: infinite` → 5 flashes/sec.
//! 2. A loading spinner with very short duration — usually
//!    fine because there's no high-contrast flash inside the
//!    rotation, but flag for audit.
//! 3. Hand-coded "attention" pulsing widgets — neon-sign
//!    blink, "ALERT" text flashing red.
//!
//! ## Heuristic
//!
//! Pure static — no time-series sampling. The JS reads
//! `getComputedStyle()` for each element with a non-default
//! `animation-name`, parses `animation-duration` +
//! `animation-iteration-count`, and computes
//! `flashes_per_second = 1000 / duration_ms` when iterations
//! ≥ infinite or > 3.
//!
//! Caveats:
//!
//! * **False positives are possible** because a fast-cycling
//!   animation might not actually create a flash (it could
//!   rotate without changing luminance). The detector errs
//!   toward catching all fast-cycling motion and lets the
//!   operator opt out per-element where the animation is
//!   audited safe.
//! * **JS-driven animations** (`Web Animations API`,
//!   `requestAnimationFrame` loops, GSAP / Framer Motion)
//!   aren't reflected in computed style. Those need a
//!   runtime probe — out of scope for this static axis.
//!
//! Skip `data-flash-allow="true"` on a verified-safe element.
//!
//! ## Severity
//!
//! * **Strict** — flashes_per_second > 3 AND
//!   (iteration_count = infinite OR iteration_count > 3).
//!   Direct WCAG 2.3.1 violation candidate.
//! * **Warn** — flashes_per_second > 1 AND
//!   iteration_count = infinite. Below the seizure threshold
//!   but still photosensitive concern (vestibular triggers,
//!   anxiety triggers).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnimationFlashRateHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Visible text (capped 40 chars) for context.
    pub text: String,
    /// `animation-name` value verbatim.
    pub animation_name: String,
    /// `animation-duration` in milliseconds (single cycle).
    pub animation_duration_ms: u32,
    /// True iff `animation-iteration-count: infinite`.
    pub iteration_count_is_infinite: bool,
    /// Parsed iteration count when finite (0 when infinite
    /// or unparseable).
    pub iteration_count: u32,
    /// `1000 / animation_duration_ms` — cycles per second.
    /// Defect when > FLASH_RATE_HZ_STRICT.
    pub flashes_per_second: f32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnimationFlashRateSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending elements (pre-filtered to fast-cycling).
    pub hits: Vec<AnimationFlashRateHit>,
    /// Total elements walked with non-default animation-name.
    pub scanned_animated_elements: u32,
}

/// Threshold above which a flashing animation triggers a
/// Strict finding. WCAG 2.3.1 explicitly states 3 Hz.
pub const FLASH_RATE_HZ_STRICT: f32 = 3.0;

/// Threshold above which a flashing animation triggers a Warn
/// finding (below WCAG threshold but still photosensitive
/// concern).
pub const FLASH_RATE_HZ_WARN: f32 = 1.0;

/// Iteration-count threshold above which we consider the
/// animation effectively continuous even when finite.
pub const FINITE_ITERATION_THRESHOLD: u32 = 3;

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_animation_flash_rate(snap: &AnimationFlashRateSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut strict: Vec<&AnimationFlashRateHit> = Vec::new();
    let mut warn: Vec<&AnimationFlashRateHit> = Vec::new();
    for h in &snap.hits {
        let continuous = h.iteration_count_is_infinite
            || h.iteration_count > FINITE_ITERATION_THRESHOLD;
        if !continuous {
            continue;
        }
        if h.flashes_per_second > FLASH_RATE_HZ_STRICT {
            strict.push(h);
        } else if h.flashes_per_second > FLASH_RATE_HZ_WARN {
            warn.push(h);
        }
    }

    let format_example = |h: &AnimationFlashRateHit| -> String {
        let inf = if h.iteration_count_is_infinite {
            "infinite".to_owned()
        } else {
            format!("{} iterations", h.iteration_count)
        };
        format!(
            "{} (animation=`{}` {}ms · {:.1} Hz · {})",
            h.selector, h.animation_name, h.animation_duration_ms, h.flashes_per_second, inf
        )
    };

    let mut out = Vec::new();
    if !strict.is_empty() {
        let examples: Vec<String> = strict
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "animation-flash-rate.above-wcag".to_owned(),
            detail: format!(
                "{} element(s) running animations faster than 3 Hz with infinite or repeated iterations — fails WCAG 2.3.1 photosensitive-epilepsy threshold. Slow the animation or add `data-flash-allow=\"true\"` only after verifying no high-contrast luminance change inside the cycle. Examples: {}",
                strict.len(),
                examples.join("; ")
            ),
        });
    }
    if !warn.is_empty() {
        let examples: Vec<String> = warn
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "animation-flash-rate.fast-continuous".to_owned(),
            detail: format!(
                "{} element(s) running animations between 1-3 Hz with infinite iterations. Below the seizure threshold but still a photosensitive / vestibular / anxiety concern. Audit and shorten the cycle or pause after N iterations. Examples: {}",
                warn.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every element with
/// a non-default `animation-name`, parses duration +
/// iteration-count, captures only fast-cycling animations
/// (warn threshold or above) so the snapshot stays small.
pub const ANIMATION_FLASH_RATE_DOM_CAPTURE_JS: &str = r#"
(() => {
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

    // Parse one duration string ("0.2s" / "200ms" / "1s") into
    // ms. Comma-lists take the first entry.
    const parseDurationMs = function(value) {
      if (!value) return 0;
      const v = value.split(',')[0].trim();
      if (v.endsWith('ms')) {
        const n = parseFloat(v.slice(0, -2));
        if (Number.isFinite(n)) return Math.round(n);
      } else if (v.endsWith('s')) {
        const n = parseFloat(v.slice(0, -1));
        if (Number.isFinite(n)) return Math.round(n * 1000);
      }
      return 0;
    };

    // Returns { infinite, count } where infinite=true means
    // animation runs forever. Comma-lists take first.
    const parseIterationCount = function(value) {
      if (!value) return { infinite: false, count: 1 };
      const v = value.split(',')[0].trim();
      if (v === 'infinite') return { infinite: true, count: 0 };
      const n = parseInt(v, 10);
      if (Number.isFinite(n) && n > 0) return { infinite: false, count: n };
      return { infinite: false, count: 1 };
    };

    const FLASH_RATE_HZ_WARN = 1.0;
    const hits = [];
    let scanned = 0;
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    let node = walk.currentNode;
    while (node) {
      if (node.nodeType === 1) {
        if (node.getAttribute && node.getAttribute('data-flash-allow') === 'true') {
          node = walk.nextNode();
          continue;
        }
        const cs = window.getComputedStyle(node);
        const animationName = (cs.animationName || 'none').trim();
        if (animationName === 'none' || animationName === '') {
          node = walk.nextNode();
          continue;
        }
        scanned += 1;
        const durationMs = parseDurationMs(cs.animationDuration);
        if (durationMs === 0) {
          node = walk.nextNode();
          continue;
        }
        const fps = 1000.0 / durationMs;
        if (fps <= FLASH_RATE_HZ_WARN) {
          node = walk.nextNode();
          continue;
        }
        const it = parseIterationCount(cs.animationIterationCount);
        const text = (node.textContent || '').trim().substring(0, 40);
        hits.push({
          selector: selectorOf(node),
          text: text,
          animationName: animationName,
          animationDurationMs: durationMs,
          iterationCountIsInfinite: it.infinite,
          iterationCount: it.count,
          flashesPerSecond: Math.round(fps * 100) / 100
        });
      }
      node = walk.nextNode();
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedAnimatedElements: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        animation_name: &str,
        duration_ms: u32,
        infinite: bool,
        iteration_count: u32,
    ) -> AnimationFlashRateHit {
        let fps = if duration_ms == 0 {
            0.0
        } else {
            1000.0 / (duration_ms as f32)
        };
        AnimationFlashRateHit {
            selector: selector.into(),
            text: String::new(),
            animation_name: animation_name.into(),
            animation_duration_ms: duration_ms,
            iteration_count_is_infinite: infinite,
            iteration_count,
            flashes_per_second: fps,
        }
    }

    fn snap(hits: Vec<AnimationFlashRateHit>) -> AnimationFlashRateSnapshot {
        AnimationFlashRateSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_animated_elements: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_animation_flash_rate(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn five_hz_infinite_is_strict() {
        // 200ms × infinite = 5 Hz → above WCAG 3 Hz threshold.
        let s = snap(vec![hit(".alert", "blink", 200, true, 0)]);
        let findings = detect_animation_flash_rate(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "animation-flash-rate.above-wcag");
        assert!(findings[0].detail.contains(".alert"));
        assert!(findings[0].detail.contains("WCAG 2.3.1"));
    }

    #[test]
    fn two_hz_infinite_is_warn() {
        // 500ms × infinite = 2 Hz → below WCAG 3 Hz, above warn 1 Hz.
        let s = snap(vec![hit(".pulse", "pulse", 500, true, 0)]);
        let findings = detect_animation_flash_rate(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "animation-flash-rate.fast-continuous");
    }

    #[test]
    fn slow_animation_below_warn_threshold_skipped() {
        // 2000ms × infinite = 0.5 Hz → below warn threshold.
        let s = snap(vec![hit(".slow", "fade", 2000, true, 0)]);
        let findings = detect_animation_flash_rate(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn fast_finite_short_iteration_count_skipped() {
        // 200ms × 2 iterations → not "continuous enough" to flag.
        let s = snap(vec![hit(".bounce", "bounce", 200, false, 2)]);
        let findings = detect_animation_flash_rate(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn fast_finite_long_iteration_count_is_strict() {
        // 200ms × 10 iterations → continuous enough; above WCAG.
        let s = snap(vec![hit(".repeat", "repeat-blink", 200, false, 10)]);
        let findings = detect_animation_flash_rate(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn mixed_severity_emits_two_findings() {
        let s = snap(vec![
            hit(".a", "blink", 200, true, 0),
            hit(".b", "pulse", 500, true, 0),
        ]);
        let findings = detect_animation_flash_rate(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"animation-flash-rate.above-wcag"));
        assert!(kinds.contains(&"animation-flash-rate.fast-continuous"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!(".bad-{i}"), "blink", 200, true, 0));
        }
        let s = snap(hits);
        let findings = detect_animation_flash_rate(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape.
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("hits"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("scannedAnimatedElements"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("animationName"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("animationDurationMs"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("iterationCountIsInfinite"));
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("flashesPerSecond"));
        // Threshold constant present.
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("FLASH_RATE_HZ_WARN"));
        // Opt-out contract.
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("data-flash-allow"));
        // Infinite token detection.
        assert!(ANIMATION_FLASH_RATE_DOM_CAPTURE_JS.contains("'infinite'"));
    }
}
