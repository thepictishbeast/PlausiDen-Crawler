//! `layout_thrash` — counts long-tasks during page load, a proxy
//! for forced-sync-layout / style-recalc storms.
//!
//! The Performance Long Tasks API surfaces any main-thread task
//! >50ms. Common causes for a long task during initial load are:
//!
//! - JS that reads `offsetWidth` / `clientHeight` / `getBoundingClientRect`
//!   immediately after writing inline styles (forced sync layout).
//! - Style recalc storms — `class` toggles on hundreds of nodes
//!   while the browser is mid-paint.
//! - Synchronous XHR (rare in 2026, still possible).
//! - Layout shift cascades from late-loading fonts.
//!
//! Distinct from `web_vitals` (which measures CLS / LCP / INP):
//! this detector counts the COUNT of long tasks, not the
//! cumulative impact. A page can have low CLS but many long
//! tasks (animation-heavy ad scripts, etc.) — both are bad signals
//! and warrant separate findings.
//!
//! ## Heuristic
//!
//! Capture `PerformanceObserver.observe({ type: 'longtask',
//! buffered: true })` results for the first 5 seconds after
//! navigation, plus the running total during scroll.
//!
//! Severity (count thresholds chosen empirically against
//! Lighthouse "Avoid long main-thread tasks" guidance):
//!
//! - `strict` — `total_count >= 5` OR `max_duration_ms >= 500`.
//!   Page reliably stalls under typical interaction.
//! - `warn` — `total_count >= 2` OR `max_duration_ms >= 200`.
//!   Page may stall under interaction; investigate.
//!
//! AVP-2: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured long-task entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LongTaskHit {
    /// Task start time relative to navigation start (CSS ms).
    pub start_ms: f64,
    /// Task duration (CSS ms).
    pub duration_ms: f64,
    /// Attribution name from PerformanceLongTaskTiming.attribution
    /// when present (e.g. "script", "iframe", "same-origin-self").
    pub attribution: String,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LayoutThrashSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Observation window (ms after navigationStart).
    pub window_ms: u32,
    /// Every captured long-task entry.
    pub hits: Vec<LongTaskHit>,
}

impl LayoutThrashSnapshot {
    /// Max duration_ms across hits (0.0 if none).
    pub fn max_duration_ms(&self) -> f64 {
        self.hits
            .iter()
            .map(|h| h.duration_ms)
            .fold(0.0_f64, f64::max)
    }

    /// Count of long-task entries.
    pub fn count(&self) -> usize {
        self.hits.len()
    }
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_layout_thrash(snap: &LayoutThrashSnapshot) -> Vec<AxisFinding> {
    let count = snap.count();
    let max = snap.max_duration_ms();
    if count == 0 {
        return Vec::new();
    }

    let strict_count = count >= 5;
    let strict_max = max >= 500.0;
    let warn_count = count >= 2;
    let warn_max = max >= 200.0;

    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(5)
        .map(|h| {
            format!(
                "+{:.0}ms × {:.0}ms ({})",
                h.start_ms, h.duration_ms, h.attribution
            )
        })
        .collect();

    if strict_count || strict_max {
        return vec![AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "layout-thrash.long-tasks".to_owned(),
            detail: format!(
                "{} long-task(s) observed in the first {}ms after navigation; max duration {:.0}ms. Page stalls under typical interaction. Common causes: forced sync layout (JS reading offsetWidth/getBoundingClientRect after a style write), class-toggle storms on many nodes, late-font layout shifts. Examples: {}",
                count, snap.window_ms, max, examples.join("; ")
            ),
        }];
    }

    if warn_count || warn_max {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "layout-thrash.long-tasks".to_owned(),
            detail: format!(
                "{} long-task(s) observed in the first {}ms after navigation; max duration {:.0}ms. Page may stall under interaction. Investigate forced-sync-layout patterns + class-toggle storms. Examples: {}",
                count, snap.window_ms, max, examples.join("; ")
            ),
        }];
    }

    Vec::new()
}

/// Browser-side capture script. Runner is expected to await the
/// 5s observation window before extracting the snapshot.
pub const LAYOUT_THRASH_DOM_CAPTURE_JS: &str = r#"
(async () => {
    const WINDOW_MS = 5000;
    const hits = [];

    if (typeof PerformanceObserver === 'undefined') {
      return {
        pageUrl: window.location.href,
        windowMs: WINDOW_MS,
        hits: [],
      };
    }

    // Buffered grab of any long-tasks that already ran (e.g. during
    // the initial parse before the observer existed).
    try {
      const observer = new PerformanceObserver(function(list) {
        const entries = list.getEntries();
        for (let i = 0; i < entries.length; i++) {
          const e = entries[i];
          let attr = '';
          if (e.attribution && e.attribution.length > 0) {
            attr = e.attribution[0].name || '';
          }
          hits.push({
            startMs: e.startTime,
            durationMs: e.duration,
            attribution: attr,
          });
        }
      });
      observer.observe({ type: 'longtask', buffered: true });
      await new Promise(function(resolve) {
        setTimeout(resolve, WINDOW_MS);
      });
      observer.disconnect();
    } catch (_) {
      // Long Tasks API not supported in this browser.
    }

    return {
      pageUrl: window.location.href,
      windowMs: WINDOW_MS,
      hits: hits,
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn lt(start: f64, dur: f64) -> LongTaskHit {
        LongTaskHit {
            start_ms: start,
            duration_ms: dur,
            attribution: "script".into(),
        }
    }

    fn snap(hits: Vec<LongTaskHit>) -> LayoutThrashSnapshot {
        LayoutThrashSnapshot {
            page_url: "https://x".into(),
            window_ms: 5000,
            hits,
        }
    }

    #[test]
    fn no_long_tasks_produces_no_findings() {
        assert!(detect_layout_thrash(&snap(vec![])).is_empty());
    }

    #[test]
    fn single_short_long_task_does_not_fire() {
        // count=1, max=80ms → below both warn thresholds.
        assert!(detect_layout_thrash(&snap(vec![lt(100.0, 80.0)])).is_empty());
    }

    #[test]
    fn two_short_long_tasks_produce_warn() {
        let findings = detect_layout_thrash(&snap(vec![lt(100.0, 80.0), lt(500.0, 90.0)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn five_long_tasks_produce_strict() {
        let findings = detect_layout_thrash(&snap(vec![
            lt(100.0, 80.0),
            lt(300.0, 70.0),
            lt(700.0, 60.0),
            lt(1500.0, 90.0),
            lt(2400.0, 80.0),
        ]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn single_huge_long_task_produces_strict() {
        let findings = detect_layout_thrash(&snap(vec![lt(100.0, 600.0)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn single_medium_long_task_produces_warn() {
        let findings = detect_layout_thrash(&snap(vec![lt(100.0, 250.0)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn snapshot_max_duration_and_count_helpers() {
        let s = snap(vec![lt(100.0, 80.0), lt(500.0, 150.0), lt(800.0, 60.0)]);
        assert_eq!(s.count(), 3);
        assert!((s.max_duration_ms() - 150.0).abs() < 0.0001);
    }

    #[test]
    fn js_capture_constant_is_sensible() {
        assert!(LAYOUT_THRASH_DOM_CAPTURE_JS.contains("PerformanceObserver"));
        assert!(LAYOUT_THRASH_DOM_CAPTURE_JS.contains("longtask"));
    }
}
