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

pub mod aria_required_attrs;
pub mod autocomplete;
pub mod autoplay_media;
pub mod cache_control;
pub mod canonical_url;
pub mod coep;
pub mod content_security_policy;
pub mod cookie_security;
pub mod coop;
pub mod corp;
pub mod cross_page_meta_description;
pub mod cross_page_title;
pub mod css_health;
pub mod doc_title;
pub mod doctype_charset;
pub mod document_policy;
pub mod dom_size;
pub mod favicon;
pub mod font_loading;
pub mod form_labels;
pub mod heading_order;
pub mod hreflang;
pub mod hsts;
pub mod html_lang;
pub mod iframe_sandbox;
pub mod image_dimensions;
pub mod info_leak_headers;
pub mod inline_script;
pub mod link_color_only;
pub mod link_target_blank_safety;
pub mod link_text;
pub mod link_underline;
pub mod local_storage_use;
pub mod long_tasks;
pub mod meta_description;
pub mod mixed_content;
pub mod modern_image_formats;
pub mod multiple_ways;
pub mod network_error_logging;
pub mod noscript_fallback;
pub mod origin_agent_cluster;
pub mod outbound_links;
pub mod permissions_policy;
pub mod placeholder_text;
pub mod referrer_policy;
pub mod render_blocking_resources;
pub mod reporting_endpoints;
pub mod runtime_contrast;
pub mod runtime_focus;
pub mod runtime_images;
pub mod runtime_landmarks;
pub mod skip_link;
pub mod speculation_rules;
pub mod sri;
pub mod status_messages;
pub mod tap_targets;
pub mod text_wrap_collapse;
pub mod trusted_types_runtime;
pub mod ui_overflow;
pub(crate) mod url_helpers;
pub mod vary_header;
pub mod viewport_meta;
pub mod web_manifest;
pub mod web_vitals;
pub mod x_frame_options;

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
