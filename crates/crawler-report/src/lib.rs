//! Crawler — typed report shape.
//!
//! Phase 2 of the TS→Rust port (CRAWLER_STACK_AUDIT.md). The TS
//! Crawler currently emits `report.json` with a hand-typed
//! interface in `src/report.ts`; this crate is the canonical
//! shape every Rust consumer (the future Rust Crawler runner,
//! `forge-replay`, future analytics tools) reads.
//!
//! Shape decisions:
//!
//! * `CapturedEvent` — one event captured during a journey
//!   (console, page error, network failure, axe violation,
//!   detector finding, csp violation, aria drift). Kind tag is
//!   the existing TS string-literal union, mapped into a Rust
//!   `EventKind` enum with `#[non_exhaustive]`.
//! * `Report`        — full structured run output. Mirrors TS
//!   `Report` field-for-field (target / journey / viewport /
//!   started / durationMs / counts / events / steps).
//! * `Diff`          — newConsoleErrors / newPageErrors / etc.
//!   Mirrors TS `Diff` field-for-field, including the
//!   newAriaDriftFindings field that was back-filled by
//!   crawler/main.ts after diffReports() runs.
//!
//! Wire-compat is verified by tests: a TS-shape JSON
//! (camelCase, lowercase severity strings) deserializes
//! directly into the Rust types.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// Every kind of event the Crawler can emit during a journey.
///
/// BUG ASSUMPTION: `#[non_exhaustive]` so adding a future kind
/// (e.g. a new detector axis) is non-breaking. Match arms in
/// downstream code MUST include `_ =>` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    /// `console.*()` call from the page.
    Console,
    /// Uncaught exception in the page (`window.onerror`).
    Pageerror,
    /// Network request that never returned a response.
    RequestFailed,
    /// Response with status >= 400.
    ResponseError,
    /// CSP block via `securitypolicyviolation` event or
    /// CDP `Audits.issueAdded`.
    CspViolation,
    /// axe-core static a11y violation.
    A11yViolation,
    /// cssHealth detector.
    CssHealth,
    /// uiOverflow detector.
    UiOverflow,
    /// runtimeContrast detector.
    RuntimeContrast,
    /// runtimeImages detector.
    RuntimeImages,
    /// runtimeFocus detector.
    RuntimeFocus,
    /// webVitals threshold breach (LCP/CLS/INP poor).
    WebVitals,
    /// aria-tree drift between consecutive runs.
    AriaDrift,
    /// headingOrder detector — h1 count + level skips.
    HeadingOrder,
    /// runtimeLandmarks detector — main/banner/contentinfo
    /// uniqueness + same-role nesting.
    RuntimeLandmarks,
    /// linkText detector — WCAG 2.4.4 link purpose: empty +
    /// generic link text.
    LinkText,
    /// formLabels detector — every form control has an accessible
    /// label (WCAG 1.3.1 + 3.3.2).
    FormLabels,
    /// skipLink detector — first focusable link is a same-page
    /// jump to #main / #content.
    SkipLink,
    /// tapTargets detector — interactive elements meet the
    /// 24×24 px AAA / 44×44 px iOS minimum size.
    TapTargets,
    /// docTitle detector — `<title>` present, non-empty, unique
    /// across the journey.
    DocTitle,
    /// placeholderText detector — TODO / FIXME / Lorem ipsum /
    /// template instructions leaked into the rendered DOM.
    PlaceholderText,
    /// viewportMeta detector — `<meta name="viewport">` present
    /// + sane (initial-scale, no user-scalable=no).
    ViewportMeta,
    /// htmlLang detector — `<html lang>` present + valid BCP-47.
    HtmlLang,
    /// favicon detector — `<link rel="icon">` present with at least
    /// one resolvable href.
    Favicon,
    /// hsts detector — Strict-Transport-Security response header.
    /// First detector in the response-header batch (cycle 2026-05-17).
    Hsts,
    /// referrerPolicy detector — Referrer-Policy response header.
    ReferrerPolicy,
    /// xFrameOptions detector — X-Frame-Options response header.
    XFrameOptions,
    /// permissionsPolicy detector — Permissions-Policy response header.
    PermissionsPolicy,
    /// varyHeader detector — Vary response header correctness.
    VaryHeader,
    /// contentSecurityPolicy detector — full CSP audit.
    ContentSecurityPolicy,
    /// cookieSecurity detector — Secure / HttpOnly / SameSite.
    CookieSecurity,
    /// coep detector — Cross-Origin-Embedder-Policy.
    Coep,
    /// coop detector — Cross-Origin-Opener-Policy.
    Coop,
    /// documentPolicy detector — Document-Policy header.
    DocumentPolicy,
    /// infoLeakHeaders detector — Server / X-Powered-By etc.
    InfoLeakHeaders,
    /// originAgentCluster detector — Origin-Agent-Cluster header.
    OriginAgentCluster,
    /// cacheControl detector — Cache-Control directive sanity.
    CacheControl,
    /// networkErrorLogging detector — NEL response header.
    Nel,
    /// reportingEndpoints detector — Reporting-Endpoints header.
    ReportingEndpoints,
    /// speculationRules detector — speculation-rules script blocks.
    SpeculationRules,
    /// autocomplete detector — `<input autocomplete>` quality.
    Autocomplete,
    /// linkUnderline detector — visible affordance for link recognition.
    LinkUnderline,
    /// mixedContent detector — http resources on an https page.
    MixedContent,
    /// outboundLinks detector — target=_blank rel=noopener integrity.
    OutboundLinks,
    /// metaDescription detector — `<meta name="description">` present + length.
    MetaDescription,
}

/// Severity of a finding-bucketed event (cssHealth / uiOverflow
/// / runtimeContrast / runtimeImages / runtimeFocus / webVitals
/// / ariaDrift). Wire-compat with `forge_core::Severity` —
/// accepts both lower-case (`"strict"`) and upper-case
/// (`"STRICT"`) JSON values. Same alias trick as forge-core
/// (T38: bash-era reports use upper-case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Ship-blocking.
    #[serde(alias = "STRICT")]
    Strict,
    /// Suppressible in PoC, escalates to strict in production.
    #[serde(alias = "WARN")]
    Warn,
}

/// axe-core impact level — borrowed verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
pub enum Impact {
    /// Minor a11y violation.
    Minor,
    /// Moderate.
    Moderate,
    /// Serious.
    Serious,
    /// Critical.
    Critical,
}

/// One event captured during a journey. Field shape mirrors the
/// TS `CapturedEvent` interface 1:1 — the JSON the TS runner
/// writes deserializes here without translation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedEvent {
    /// Wall time (ms since journey start).
    pub t: u64,
    /// Event kind.
    pub kind: EventKind,
    /// Console level (`"log"`, `"warn"`, `"error"`, `"info"`).
    /// Only meaningful for `EventKind::Console`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub level: Option<String>,
    /// Free-form description.
    pub text: String,
    /// Asset URL (set for network / CSS health / SRI).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    /// HTTP status (set for response-error).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<u16>,
    /// Stack trace (set for pageerror).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stack: Option<String>,
    /// axe-core impact level (a11y-violation only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub impact: Option<Impact>,
    /// axe-core rule id (a11y-violation only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rule_id: Option<String>,
    /// Severity bucket (set for finding-bucketed kinds).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub severity: Option<Severity>,
}

/// Per-axis finding counts. Mirrors `Report.counts` in TS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCounts {
    /// `console.error` count.
    pub console_errors: u32,
    /// `pageerror` count.
    pub page_errors: u32,
    /// Failed network requests.
    pub failed_requests: u32,
    /// axe-core violations.
    pub a11y_violations: u32,
    /// cssHealth total.
    pub css_health_findings: u32,
    /// cssHealth strict subset.
    pub css_health_findings_strict: u32,
    /// uiOverflow total.
    pub ui_overflow_findings: u32,
    /// uiOverflow strict subset.
    pub ui_overflow_findings_strict: u32,
    /// runtimeContrast total.
    pub runtime_contrast_findings: u32,
    /// runtimeContrast strict subset.
    pub runtime_contrast_findings_strict: u32,
    /// runtimeImages total.
    pub runtime_images_findings: u32,
    /// runtimeImages strict subset.
    pub runtime_images_findings_strict: u32,
    /// runtimeFocus total.
    pub runtime_focus_findings: u32,
    /// runtimeFocus strict subset.
    pub runtime_focus_findings_strict: u32,
    /// webVitals total.
    pub web_vitals_findings: u32,
    /// webVitals strict subset.
    pub web_vitals_findings_strict: u32,
    /// CSP violations.
    pub csp_violations: u32,
    /// Total events captured.
    pub total: u32,
    /// Step count that ran successfully.
    pub steps_ok: u32,
    /// Step count that failed.
    pub steps_failed: u32,
}

/// Viewport dimensions reported in the run header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    /// CSS pixel width.
    pub w: u32,
    /// CSS pixel height.
    pub h: u32,
}

/// Per-step result. `Step` is opaque to this crate (different
/// crates own the journey schema); kept as an untyped JSON value
/// for round-trip preservation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    /// 0-based index in the journey.
    #[serde(default)]
    pub index: u32,
    /// Step succeeded.
    pub ok: bool,
    /// Wall time spent in this step (ms).
    pub duration_ms: u64,
    /// Original step config (kind, label, etc.). Opaque to the
    /// report crate — the journey schema lives in a future
    /// `crawler-journey` crate.
    pub step: serde_json::Value,
    /// Failure message if `!ok`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// Top-level run output. Wire-compat with the TS `Report`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Target URL (the `--url` flag or `journey.baseUrl`).
    pub target: String,
    /// Journey name.
    pub journey: String,
    /// Render viewport.
    pub viewport: Viewport,
    /// ISO 8601 timestamp.
    pub started: String,
    /// Total wall-clock ms across the journey.
    pub duration_ms: u64,
    /// Per-axis counts.
    pub counts: ReportCounts,
    /// All events, in capture order.
    pub events: Vec<CapturedEvent>,
    /// Step results, in run order.
    pub steps: Vec<StepResult>,
}

/// Diff against the prior baseline run. Mirrors TS `Diff`
/// field-for-field including the back-filled
/// `newAriaDriftFindings` field (T83 fix).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// New `console.error` events.
    pub new_console_errors: Vec<CapturedEvent>,
    /// New page errors.
    pub new_page_errors: Vec<CapturedEvent>,
    /// New failed requests.
    pub new_failed_requests: Vec<CapturedEvent>,
    /// New axe-core violations.
    pub new_a11y_violations: Vec<CapturedEvent>,
    /// New cssHealth findings.
    pub new_css_health_findings: Vec<CapturedEvent>,
    /// New uiOverflow findings.
    pub new_ui_overflow_findings: Vec<CapturedEvent>,
    /// New runtimeContrast findings.
    pub new_runtime_contrast_findings: Vec<CapturedEvent>,
    /// New runtimeImages findings.
    pub new_runtime_images_findings: Vec<CapturedEvent>,
    /// New runtimeFocus findings.
    pub new_runtime_focus_findings: Vec<CapturedEvent>,
    /// New webVitals threshold breaches.
    pub new_web_vitals_findings: Vec<CapturedEvent>,
    /// New CSP violations.
    pub new_csp_violations: Vec<CapturedEvent>,
    /// New ariaDrift findings (back-filled by main.ts after
    /// aria-drift events are pushed onto report.events).
    pub new_aria_drift_findings: Vec<CapturedEvent>,
    /// Steps that newly failed in this run.
    pub newly_broken_steps: Vec<StepResult>,
    /// Steps that were broken but now pass.
    pub fixed_steps: Vec<StepResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire-compat: a JSON blob written by the TS Crawler must
    /// deserialize into Rust types unchanged. This test pins
    /// the alignment via a representative TS-shape sample.
    #[test]
    fn ts_shape_captured_event_round_trips() {
        let ts_json = r#"{
            "t": 1234,
            "kind": "css-health",
            "text": "[css.served-but-not-applied] body has UA defaults",
            "severity": "strict",
            "url": "/loom-skin.css"
        }"#;
        let evt: CapturedEvent = serde_json::from_str(ts_json).expect("deserialize");
        assert_eq!(evt.kind, EventKind::CssHealth);
        assert_eq!(evt.severity, Some(Severity::Strict));
        assert_eq!(evt.url.as_deref(), Some("/loom-skin.css"));
    }

    #[test]
    fn ts_shape_diff_round_trips() {
        let ts_json = r#"{
            "newConsoleErrors": [],
            "newPageErrors": [],
            "newFailedRequests": [],
            "newA11yViolations": [],
            "newCssHealthFindings": [],
            "newUiOverflowFindings": [{
                "t": 100, "kind": "ui-overflow", "text": "tap-target",
                "severity": "warn"
            }],
            "newRuntimeContrastFindings": [],
            "newRuntimeImagesFindings": [],
            "newRuntimeFocusFindings": [],
            "newWebVitalsFindings": [],
            "newCspViolations": [],
            "newAriaDriftFindings": [],
            "newlyBrokenSteps": [],
            "fixedSteps": []
        }"#;
        let diff: Diff = serde_json::from_str(ts_json).expect("deserialize");
        assert_eq!(diff.new_ui_overflow_findings.len(), 1);
        assert_eq!(diff.new_ui_overflow_findings[0].kind, EventKind::UiOverflow);
        assert_eq!(
            diff.new_ui_overflow_findings[0].severity,
            Some(Severity::Warn)
        );
    }

    #[test]
    fn impact_lowercase() {
        let json = r#"{"t":0,"kind":"a11y-violation","text":"x","impact":"serious","ruleId":"color-contrast"}"#;
        let evt: CapturedEvent = serde_json::from_str(json).expect("de");
        assert_eq!(evt.impact, Some(Impact::Serious));
        assert_eq!(evt.rule_id.as_deref(), Some("color-contrast"));
    }

    #[test]
    fn severity_uppercase_alias() {
        // Bash-era / legacy reports may have STRICT in upper-case.
        let json = r#"{"t":0,"kind":"css-health","text":"x","severity":"STRICT"}"#;
        let evt: CapturedEvent = serde_json::from_str(json).expect("de");
        assert_eq!(evt.severity, Some(Severity::Strict));
    }

    #[test]
    fn report_round_trips() {
        let r = Report {
            target: "http://x".to_owned(),
            journey: "smoke".to_owned(),
            viewport: Viewport { w: 1280, h: 800 },
            started: "2026-05-04T17:00:00Z".to_owned(),
            duration_ms: 4321,
            counts: ReportCounts {
                console_errors: 1,
                ..Default::default()
            },
            events: vec![],
            steps: vec![],
        };
        let json = serde_json::to_string(&r).expect("ser");
        let back: Report = serde_json::from_str(&json).expect("de");
        assert_eq!(back.viewport.w, 1280);
        assert_eq!(back.counts.console_errors, 1);
    }

    #[test]
    fn diff_default_is_empty() {
        let d = Diff::default();
        assert!(d.new_console_errors.is_empty());
        assert!(d.new_aria_drift_findings.is_empty());
        assert!(d.fixed_steps.is_empty());
    }

    #[test]
    fn unknown_kind_is_rejected() {
        // `#[non_exhaustive]` doesn't help with deserialize:
        // unknown variants should fail loud, not silently
        // become a fallback. This pins the behaviour.
        let json = r#"{"t":0,"kind":"made-up-kind","text":"x"}"#;
        let result: Result<CapturedEvent, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown kind should reject");
    }
}
