//! `crawler-debug-capture` — typed debug-mode capture metadata.
//!
//! When the Crawler runs with `--debug` (or the
//! `debug_capture: {…}` config block), it records additional
//! artifacts per journey step:
//!
//!   * Screen recording (WebM, codec-tagged)
//!   * HAR file (HTTP Archive 1.2 — every request, header,
//!     timing, response body up to a size cap)
//!   * Console log capture (level, text, source location)
//!   * Optional WebSocket frame capture
//!
//! Why these specific artifacts:
//!   * Screen recording is the only honest evidence of what the
//!     reader actually saw at each step. Frame-by-frame replay
//!     surfaces CLS, flicker, layout-shift mid-load that no
//!     after-the-fact detector can catch.
//!   * HAR is the wire-truth — every request, every redirect,
//!     every header. Required for security review +
//!     supply-chain audits.
//!   * Console logs surface runtime errors the screenshots miss
//!     (silent failures, unhandled rejections).
//!
//! ### Scope (this crate)
//!
//! Typed metadata + on-disk layout + HAR parsing/emission only.
//! The actual recording (CDP commands, ffmpeg, WebM mux) lives
//! in the runner harness. This crate is the cross-runner
//! contract — the Rust + the TS Crawler can both emit the same
//! debug-bundle shape, the same admin UI can replay either.
//!
//! ### Public surface
//!
//! - [`DebugCaptureConfig`] — what to record
//! - [`DebugSession`]      — top-level bundle per journey run
//! - [`StepArtifacts`]     — per-step recording + HAR + logs
//! - [`Har`]               — RFC-shaped HAR 1.2 root
//! - [`HarEntry`]          — one request/response pair
//! - [`ConsoleEvent`]      — one console log event
//! - [`WebSocketFrame`]    — one WS frame when captured
//!
//! ### Why the runner does IO, not this crate
//!
//! Per `crawler_stays_general_purpose`: the same metadata shape
//! must work for the Rust runner (chromiumoxide), the legacy TS
//! runner (Playwright), and any future runner (WebDriver-BiDi).
//! Keeping IO out of this crate means every runner can produce
//! the same artifact layout without coordinating implementation
//! detail.

#![deny(unsafe_code)]
#![deny(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Operator-facing config for what to record in debug mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DebugCaptureConfig {
    /// Whether debug capture is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Capture screen recording per step.
    #[serde(default = "default_true")]
    pub video: bool,
    /// Capture HAR (HTTP Archive) per step.
    #[serde(default = "default_true")]
    pub har: bool,
    /// Capture console events per step.
    #[serde(default = "default_true")]
    pub console: bool,
    /// Capture WebSocket frames per step. Default false — large.
    #[serde(default)]
    pub websocket: bool,
    /// Output directory the runner writes artifacts under.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    /// Max response body bytes per HAR entry. 0 = headers only.
    /// Default 256 KiB.
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: u64,
}

fn default_true() -> bool {
    true
}
fn default_output_dir() -> String {
    "crawler-debug".to_string()
}
fn default_max_response_bytes() -> u64 {
    256 * 1024
}

impl Default for DebugCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            video: true,
            har: true,
            console: true,
            websocket: false,
            output_dir: default_output_dir(),
            max_response_bytes: default_max_response_bytes(),
        }
    }
}

impl DebugCaptureConfig {
    /// Convenience: enabled with the documented defaults.
    pub fn enabled_defaults() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// Per-step on-disk artifact pointers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StepArtifacts {
    /// Step index within the journey (0-based).
    pub step_index: u32,
    /// Step name (e.g. `"navigate"`, `"click signin"`).
    pub step_name: String,
    /// Screen-recording WebM path, relative to
    /// [`DebugSession::output_dir`]. None if `video=false` or no
    /// frames captured (e.g. zero-duration step).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_path: Option<String>,
    /// Recording codec slug (`"vp9"` / `"vp8"` / `"av1"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    /// Recording duration in milliseconds.
    #[serde(default)]
    pub video_duration_ms: u32,
    /// HAR file path, relative to [`DebugSession::output_dir`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub har_path: Option<String>,
    /// Console events captured during the step.
    #[serde(default)]
    pub console: Vec<ConsoleEvent>,
    /// WebSocket frames captured (when enabled).
    #[serde(default)]
    pub websocket_frames: Vec<WebSocketFrame>,
}

/// Top-level debug bundle for one journey run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DebugSession {
    /// Journey identifier (operator-defined slug).
    pub journey_id: String,
    /// ISO-8601 UTC timestamp when the journey began.
    pub started_at: String,
    /// Runner backend slug (`"chromiumoxide"` / `"playwright"`
    /// / `"webdriver-bidi"`).
    pub runner: String,
    /// Output directory the artifact paths are relative to
    /// (operator filesystem path).
    pub output_dir: String,
    /// Config that produced this session.
    pub config: DebugCaptureConfig,
    /// Per-step artifacts in journey order.
    pub steps: Vec<StepArtifacts>,
}

/// One console log event captured during a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConsoleEvent {
    /// Console level (`"log"` / `"info"` / `"warn"` / `"error"` /
    /// `"debug"` / `"trace"`).
    pub level: String,
    /// Concatenated message text.
    pub text: String,
    /// Source URL the event originated from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Source line (when available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// One captured WebSocket frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct WebSocketFrame {
    /// `"send"` or `"receive"`.
    pub direction: String,
    /// Frame opcode (`"text"` / `"binary"` / `"ping"` / `"pong"`
    /// / `"close"`).
    pub opcode: String,
    /// Text payload (when opcode is `"text"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Payload byte length.
    pub byte_length: u64,
    /// ISO-8601 timestamp.
    pub at: String,
}

// ============================================================
// HAR types (HTTP Archive 1.2 subset).
// ============================================================

/// HAR 1.2 root document.
///
/// No `Eq` derive because nested `HarTimings` carries f64 (which
/// is not `Eq`). Use `PartialEq` for tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Har {
    /// The single `log` member per spec.
    pub log: HarLog,
}

/// HAR `log` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HarLog {
    /// Version string (always `"1.2"` for this crate).
    #[serde(default = "default_har_version")]
    pub version: String,
    /// Creator block — engine name + version.
    pub creator: HarCreator,
    /// Captured entries in chronological order.
    #[serde(default)]
    pub entries: Vec<HarEntry>,
}

fn default_har_version() -> String {
    "1.2".to_string()
}

/// HAR `creator` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HarCreator {
    /// Creator name (e.g. `"PlausiDen-Crawler"`).
    pub name: String,
    /// Creator version.
    pub version: String,
}

/// One HAR entry — request + response + timings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarEntry {
    /// Started ISO-8601 timestamp.
    pub started_date_time: String,
    /// Total elapsed time in milliseconds.
    pub time: f64,
    /// Request details.
    pub request: HarRequest,
    /// Response details.
    pub response: HarResponse,
    /// Optional cache block (omitted — Crawler doesn't model
    /// cache state from the browser side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<serde_json::Value>,
    /// Per-phase timings.
    pub timings: HarTimings,
}

/// HAR `request` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarRequest {
    /// HTTP method.
    pub method: String,
    /// Full request URL.
    pub url: String,
    /// HTTP version (e.g. `"HTTP/2"`).
    pub http_version: String,
    /// Request headers.
    #[serde(default)]
    pub headers: Vec<HarHeader>,
    /// Query-string parameters.
    #[serde(default)]
    pub query_string: Vec<HarHeader>,
    /// Approximate header section size (bytes). -1 if unknown.
    #[serde(default = "minus_one_i64")]
    pub headers_size: i64,
    /// Body size (bytes). -1 if unknown.
    #[serde(default = "minus_one_i64")]
    pub body_size: i64,
}

/// HAR `response` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarResponse {
    /// HTTP status code.
    pub status: u16,
    /// HTTP status text (e.g. `"OK"`).
    pub status_text: String,
    /// HTTP version.
    pub http_version: String,
    /// Response headers.
    #[serde(default)]
    pub headers: Vec<HarHeader>,
    /// Content block.
    pub content: HarContent,
    /// Redirect URL (empty if none).
    #[serde(default)]
    pub redirect_url: String,
    /// Approximate header section size (bytes). -1 if unknown.
    #[serde(default = "minus_one_i64")]
    pub headers_size: i64,
    /// Body size (bytes). -1 if unknown.
    #[serde(default = "minus_one_i64")]
    pub body_size: i64,
}

/// Generic header (request / response / query).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarHeader {
    /// Header name (preserved-case).
    pub name: String,
    /// Header value.
    pub value: String,
}

/// HAR `content` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarContent {
    /// Length of the response body in bytes.
    pub size: i64,
    /// MIME type with optional charset.
    pub mime_type: String,
    /// Body text (truncated to
    /// [`DebugCaptureConfig::max_response_bytes`] by the runner).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Encoding when body is base64 (`"base64"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

/// HAR `timings` block — per-phase ms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HarTimings {
    /// DNS time. -1 if not applicable.
    #[serde(default = "minus_one_f64")]
    pub dns: f64,
    /// Connect time.
    #[serde(default = "minus_one_f64")]
    pub connect: f64,
    /// TLS time.
    #[serde(default = "minus_one_f64")]
    pub ssl: f64,
    /// Time from request-sent to first byte.
    #[serde(default = "minus_one_f64")]
    pub send: f64,
    /// Wait time (server processing).
    pub wait: f64,
    /// Receive time.
    pub receive: f64,
}

fn minus_one_i64() -> i64 {
    -1
}
fn minus_one_f64() -> f64 {
    -1.0
}

impl Har {
    /// Build a fresh HAR with the Crawler creator stamped + no
    /// entries.
    pub fn new(creator_name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            log: HarLog {
                version: default_har_version(),
                creator: HarCreator {
                    name: creator_name.into(),
                    version: version.into(),
                },
                entries: Vec::new(),
            },
        }
    }

    /// Parse from JSON.
    pub fn from_json(s: &str) -> Result<Self, DebugCaptureError> {
        serde_json::from_str(s).map_err(DebugCaptureError::Json)
    }

    /// Emit as JSON. Pretty-printed for human reviewability.
    pub fn to_json(&self) -> Result<String, DebugCaptureError> {
        serde_json::to_string_pretty(self).map_err(DebugCaptureError::Json)
    }

    /// Total response bytes captured across all entries
    /// (request body bytes excluded; for that, see `request_body_bytes`).
    pub fn response_body_bytes(&self) -> i64 {
        self.log
            .entries
            .iter()
            .map(|e| e.response.content.size)
            .sum()
    }

    /// Entries grouped by response status class (2xx / 3xx /
    /// 4xx / 5xx / other).
    pub fn by_status_class(&self) -> BTreeMap<&'static str, Vec<&HarEntry>> {
        let mut out: BTreeMap<&'static str, Vec<&HarEntry>> = BTreeMap::new();
        for e in &self.log.entries {
            let class = match e.response.status {
                200..=299 => "2xx",
                300..=399 => "3xx",
                400..=499 => "4xx",
                500..=599 => "5xx",
                _ => "other",
            };
            out.entry(class).or_default().push(e);
        }
        out
    }
}

/// Errors from the typed-capture surface.
#[derive(Debug, thiserror::Error)]
pub enum DebugCaptureError {
    /// JSON parse / serialize failure.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// On-disk path was outside the configured output_dir.
    #[error("artifact path escapes output_dir: {0}")]
    PathEscapesOutputDir(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_safe() {
        let c = DebugCaptureConfig::default();
        assert!(!c.enabled); // off by default
        assert!(c.video);
        assert!(c.har);
        assert!(c.console);
        assert!(!c.websocket); // large; opt-in
        assert_eq!(c.max_response_bytes, 256 * 1024);
        assert_eq!(c.output_dir, "crawler-debug");
    }

    #[test]
    fn enabled_defaults_flips_just_the_enabled_flag() {
        let c = DebugCaptureConfig::enabled_defaults();
        assert!(c.enabled);
        assert!(c.video);
        assert_eq!(c.max_response_bytes, 256 * 1024);
    }

    #[test]
    fn config_rejects_unknown_field() {
        let bad = r#"{"enabled":true,"ahem":1}"#;
        let r: Result<DebugCaptureConfig, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }

    #[test]
    fn debug_session_round_trips_serde() {
        let s = DebugSession {
            journey_id: "smoke".into(),
            started_at: "2026-05-17T22:00:00Z".into(),
            runner: "chromiumoxide".into(),
            output_dir: "/tmp/debug".into(),
            config: DebugCaptureConfig::enabled_defaults(),
            steps: vec![StepArtifacts {
                step_index: 0,
                step_name: "navigate".into(),
                video_path: Some("step-0.webm".into()),
                video_codec: Some("vp9".into()),
                video_duration_ms: 1200,
                har_path: Some("step-0.har".into()),
                console: vec![ConsoleEvent {
                    level: "error".into(),
                    text: "uncaught".into(),
                    source: Some("https://example.com/app.js".into()),
                    line: Some(42),
                }],
                websocket_frames: vec![],
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: DebugSession = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn har_new_emits_creator_and_empty_entries() {
        let h = Har::new("PlausiDen-Crawler", "0.1.0");
        assert_eq!(h.log.version, "1.2");
        assert_eq!(h.log.creator.name, "PlausiDen-Crawler");
        assert!(h.log.entries.is_empty());
    }

    #[test]
    fn har_to_json_then_from_json_roundtrips() {
        let h = Har::new("PlausiDen-Crawler", "0.1.0");
        let s = h.to_json().unwrap();
        let back = Har::from_json(&s).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn har_response_body_bytes_sums_content_sizes() {
        let mut h = Har::new("c", "v");
        h.log.entries.push(make_entry(200, 1024));
        h.log.entries.push(make_entry(200, 2048));
        h.log.entries.push(make_entry(404, 500));
        assert_eq!(h.response_body_bytes(), 1024 + 2048 + 500);
    }

    #[test]
    fn har_groups_by_status_class() {
        let mut h = Har::new("c", "v");
        h.log.entries.push(make_entry(200, 0));
        h.log.entries.push(make_entry(301, 0));
        h.log.entries.push(make_entry(404, 0));
        h.log.entries.push(make_entry(500, 0));
        h.log.entries.push(make_entry(999, 0));
        let by = h.by_status_class();
        assert_eq!(by.get("2xx").unwrap().len(), 1);
        assert_eq!(by.get("3xx").unwrap().len(), 1);
        assert_eq!(by.get("4xx").unwrap().len(), 1);
        assert_eq!(by.get("5xx").unwrap().len(), 1);
        assert_eq!(by.get("other").unwrap().len(), 1);
    }

    #[test]
    fn console_event_roundtrips() {
        let e = ConsoleEvent {
            level: "warn".into(),
            text: "deprecation".into(),
            source: None,
            line: None,
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: ConsoleEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn websocket_frame_roundtrips() {
        let f = WebSocketFrame {
            direction: "send".into(),
            opcode: "text".into(),
            text: Some("hello".into()),
            byte_length: 5,
            at: "2026-05-17T22:00:00Z".into(),
        };
        let s = serde_json::to_string(&f).unwrap();
        let back: WebSocketFrame = serde_json::from_str(&s).unwrap();
        assert_eq!(f, back);
    }

    fn make_entry(status: u16, body_size: i64) -> HarEntry {
        HarEntry {
            started_date_time: "2026-05-17T22:00:00Z".into(),
            time: 1.0,
            request: HarRequest {
                method: "GET".into(),
                url: "https://example.com/".into(),
                http_version: "HTTP/2".into(),
                headers: vec![],
                query_string: vec![],
                headers_size: -1,
                body_size: -1,
            },
            response: HarResponse {
                status,
                status_text: "X".into(),
                http_version: "HTTP/2".into(),
                headers: vec![],
                content: HarContent {
                    size: body_size,
                    mime_type: "text/html".into(),
                    text: None,
                    encoding: None,
                },
                redirect_url: String::new(),
                headers_size: -1,
                body_size,
            },
            cache: None,
            timings: HarTimings {
                dns: -1.0,
                connect: -1.0,
                ssl: -1.0,
                send: -1.0,
                wait: 1.0,
                receive: 0.0,
            },
        }
    }
}
