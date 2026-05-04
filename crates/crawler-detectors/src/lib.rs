//! Crawler — detector eval-functions + typed result structs.
//!
//! Each detector axis is a string of JavaScript that runs inside
//! the page (via `page.evaluate` from Playwright today, via
//! `chromiumoxide::Page::evaluate_function` from the future Rust
//! Crawler). The result of each eval is a structured object that
//! deserialises into one of the typed structs in this crate.
//!
//! Phase 1 of the TS→Rust port (CRAWLER_STACK_AUDIT.md): consolidate
//! the eval-function strings here so both the current TS path and
//! the future Rust path consume from a single source of truth.
//!
//! Pinned axes (T100):
//!
//! * `runtime_contrast` — WCAG 2.1 contrast on every visible text
//!   node, walking the parent chain to compute the effective
//!   background (handles translucent stacking and bails on
//!   gradient backgrounds).
//!
//! Remaining axes (T100.X follow-ups, one tick each):
//!
//! * css_health
//! * ui_overflow
//! * runtime_images
//! * runtime_focus
//! * web_vitals
//! * aria_drift
//!
//! AVP-2 invariants:
//!
//! * `unsafe_code = "deny"`.
//! * Zero `unwrap`/`expect` in non-test code.
//! * Every public type carries a `BUG ASSUMPTION` comment.
//! * Every result struct is `#[non_exhaustive]` so adding fields
//!   in a future minor isn't a breaking change.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

pub mod css_health;
pub mod heading_order;
pub mod runtime_contrast;
pub mod runtime_focus;
pub mod runtime_images;
pub mod ui_overflow;
pub mod web_vitals;

/// Severity bucket shared across every detector axis. Mirrors the
/// TS string-literal `'strict' | 'warn'` exactly (lowercase wire
/// format) and matches `crawler_report::Severity` for round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisSeverity {
    /// Gate-blocking. Build / journey fails on any strict.
    Strict,
    /// Within budget. Surfaces but doesn't block.
    Warn,
}

/// One detector finding — the normalized output every per-axis
/// `detect_*_issues` function produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisFinding {
    /// Strict (gate-blocking) or warn (within budget).
    pub severity: AxisSeverity,
    /// Machine-grepable kind id (e.g. `overflow.text-clipped`).
    pub kind: String,
    /// Human-readable explanation.
    pub detail: String,
}
