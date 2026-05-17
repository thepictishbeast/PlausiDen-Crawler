//! `long_tasks` — main-thread blocking events.
//!
//! Per the W3C Long Tasks API: any synchronous main-thread work
//! exceeding 50ms blocks the renderer from processing input,
//! frame work, or animation callbacks. Web Vitals INP
//! (Interaction to Next Paint) — the third Core Web Vital
//! starting March 2024 — is directly driven by long-task count
//! + duration.
//!
//! Thresholds match the Lighthouse Total Blocking Time scoring:
//!
//!   * single task `> 50ms` (the spec definition)        → log
//!   * single task `> 250ms`                              → warn
//!   * single task `> 500ms`                              → strict
//!     (any one task this long fails INP for that interaction)
//!   * cumulative TBT `> 200ms` (Lighthouse "Good" cutoff) → warn
//!   * cumulative TBT `> 600ms` (Lighthouse "Poor" cutoff) → strict
//!
//! TBT = sum of `(task_duration - 50ms)` across the period; only
//! the over-budget portion of each long task contributes.
//!
//! Findings:
//!   * `long-tasks.slowest-strict`     strict   slowest > 500ms
//!   * `long-tasks.slowest-warn`       warn     slowest 250..=500
//!   * `long-tasks.tbt-strict`         strict   TBT > 600ms
//!   * `long-tasks.tbt-warn`           warn     TBT 200..=600
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Long-task definition threshold (W3C spec).
pub const LONG_TASK_MS: f64 = 50.0;

/// Per-task warn threshold (ms).
pub const TASK_WARN_MS: f64 = 250.0;

/// Per-task strict threshold (ms).
pub const TASK_STRICT_MS: f64 = 500.0;

/// TBT warn threshold (Lighthouse "Good" cutoff, ms).
pub const TBT_WARN_MS: f64 = 200.0;

/// TBT strict threshold (Lighthouse "Poor" cutoff, ms).
pub const TBT_STRICT_MS: f64 = 600.0;

/// One captured long task entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LongTaskEntry {
    /// Task duration in milliseconds.
    pub duration_ms: f64,
    /// `PerformanceLongTaskTiming.attribution[0].name` if present
    /// (typically `"unknown"` or `"self"` on most engines).
    pub attribution: String,
}

/// Captured long-tasks observation window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LongTasksSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Captured long tasks observed in the snapshot window.
    pub tasks: Vec<LongTaskEntry>,
}

/// Page-side eval. Registers a PerformanceObserver for `longtask`
/// entries, lets the page run for `windowMs` (default 5000), then
/// returns the recorded entries.
///
/// Consumers can use this directly or pass a pre-collected
/// snapshot (e.g. from a longer crawl session).
pub const LONG_TASKS_JS: &str = r##"(async () => {
    const windowMs = 5000;
    const entries = [];
    let observer = null;
    try {
        observer = new PerformanceObserver(function(list) {
            const out = list.getEntries();
            for (let i = 0; i < out.length; i++) {
                const e = out[i];
                const attr = (e.attribution && e.attribution[0] && e.attribution[0].name)
                    ? e.attribution[0].name : 'unknown';
                entries.push({ durationMs: e.duration, attribution: attr });
            }
        });
        observer.observe({ type: 'longtask', buffered: true });
    } catch (e) {}
    await new Promise(function(r) { setTimeout(r, windowMs); });
    if (observer) try { observer.disconnect(); } catch (e) {}
    return { pageUrl: window.location.href, tasks: entries };
})()"##;

/// Compute Total Blocking Time from a slice of task durations
/// (each in ms). TBT is the sum of the over-50ms portion of each
/// task — anything ≤ 50ms contributes 0.
pub fn total_blocking_time(durations: &[f64]) -> f64 {
    durations.iter().map(|d| (d - LONG_TASK_MS).max(0.0)).sum()
}

/// Run the detector.
pub fn detect_long_task_issues(snap: &LongTasksSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    if snap.tasks.is_empty() {
        return out;
    }
    let durations: Vec<f64> = snap.tasks.iter().map(|t| t.duration_ms).collect();
    let slowest = durations.iter().fold(0.0_f64, |a, b| a.max(*b));
    let tbt = total_blocking_time(&durations);

    // Per-task severity.
    if slowest > TASK_STRICT_MS {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "long-tasks.slowest-strict".into(),
            detail: format!(
                "slowest main-thread task is {:.1}ms (> {}ms strict; INP fails for any interaction overlapping this task)",
                slowest, TASK_STRICT_MS as u32
            ),
        });
    } else if slowest > TASK_WARN_MS {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "long-tasks.slowest-warn".into(),
            detail: format!(
                "slowest main-thread task is {:.1}ms (> {}ms warn threshold)",
                slowest, TASK_WARN_MS as u32
            ),
        });
    }

    // TBT severity.
    if tbt > TBT_STRICT_MS {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "long-tasks.tbt-strict".into(),
            detail: format!(
                "total blocking time is {:.1}ms across {} long tasks (> {}ms Lighthouse Poor cutoff)",
                tbt,
                snap.tasks.len(),
                TBT_STRICT_MS as u32
            ),
        });
    } else if tbt > TBT_WARN_MS {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "long-tasks.tbt-warn".into(),
            detail: format!(
                "total blocking time is {:.1}ms across {} long tasks (> {}ms Lighthouse Good cutoff)",
                tbt,
                snap.tasks.len(),
                TBT_WARN_MS as u32
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(durations: &[f64]) -> LongTasksSnapshot {
        LongTasksSnapshot {
            page_url: "https://example.com/".into(),
            tasks: durations
                .iter()
                .map(|d| LongTaskEntry {
                    duration_ms: *d,
                    attribution: "unknown".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn no_tasks_is_clean() {
        assert!(detect_long_task_issues(&snap(&[])).is_empty());
    }

    #[test]
    fn short_tasks_below_50ms_are_clean() {
        // Tasks at the long-task threshold itself don't fire — they
        // contribute 0 to TBT and aren't over warn/strict.
        let f = detect_long_task_issues(&snap(&[40.0, 30.0, 49.9]));
        assert!(f.is_empty());
    }

    #[test]
    fn slowest_task_above_warn_warns() {
        let f = detect_long_task_issues(&snap(&[300.0]));
        assert!(f.iter().any(|x| x.kind == "long-tasks.slowest-warn"));
    }

    #[test]
    fn slowest_task_above_strict_is_strict() {
        let f = detect_long_task_issues(&snap(&[600.0]));
        assert!(f.iter().any(|x| x.kind == "long-tasks.slowest-strict"));
    }

    #[test]
    fn tbt_only_counts_over_budget_portion() {
        // 5 tasks of 100ms each: TBT = (100-50)*5 = 250ms
        let tbt = total_blocking_time(&[100.0, 100.0, 100.0, 100.0, 100.0]);
        assert!((tbt - 250.0).abs() < 0.001);
        // Same scenario emits tbt-warn.
        let f = detect_long_task_issues(&snap(&[100.0, 100.0, 100.0, 100.0, 100.0]));
        assert!(f.iter().any(|x| x.kind == "long-tasks.tbt-warn"));
    }

    #[test]
    fn tbt_above_strict_cutoff_is_strict() {
        // Several big tasks: TBT pushes well past 600.
        let f = detect_long_task_issues(&snap(&[200.0, 200.0, 200.0, 200.0, 200.0]));
        // TBT = (200-50)*5 = 750ms
        assert!(f.iter().any(|x| x.kind == "long-tasks.tbt-strict"));
    }

    #[test]
    fn very_slow_single_task_fires_both_strict_findings() {
        let f = detect_long_task_issues(&snap(&[800.0]));
        // Slowest > 500ms AND TBT = 750 > 600.
        assert!(f.iter().any(|x| x.kind == "long-tasks.slowest-strict"));
        assert!(f.iter().any(|x| x.kind == "long-tasks.tbt-strict"));
    }

    #[test]
    fn single_short_task_below_warn_is_clean() {
        // 100ms — long-task by spec but below per-task warn (250)
        // AND TBT = 50 below TBT warn (200).
        let f = detect_long_task_issues(&snap(&[100.0]));
        assert!(f.is_empty());
    }
}
