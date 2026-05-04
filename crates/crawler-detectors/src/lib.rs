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

pub mod css_health;
pub mod runtime_contrast;
pub mod runtime_focus;
pub mod runtime_images;
pub mod ui_overflow;
pub mod web_vitals;
