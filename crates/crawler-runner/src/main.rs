//! Crawler — chromiumoxide-based journey runner.
//!
//! Phase 3 of the TS→Rust port (CRAWLER_STACK_AUDIT.md decision).
//! MVP scope this commit:
//!
//! * Launch Chromium via `chromiumoxide::Browser::launch`.
//! * Read journey JSON via `crawler_journey::load`.
//! * Drive `goto` / `wait` / `screenshot` steps.
//! * Capture three always-on axes:
//!   - console errors      (page.subscribe to `Runtime.consoleAPICalled`)
//!   - page errors         (Runtime.exceptionThrown)
//!   - failed requests     (Network.loadingFailed)
//! * Emit a `crawler_report::Report` to `runs/<name>-<ts>/report.json`.
//!
//! Out of scope for this commit (queued):
//! * Detector axes (cssHealth / uiOverflow / runtime* / web-vitals /
//!   ariaDrift) — Phase 4.
//! * `click` / `press` / `scroll` / `waitForSelector` step kinds —
//!   wired but not exercised in the MVP journey.
//! * Throttle / zoom / firstTime / screenReader journey flags —
//!   Phase 5.
//!
//! AVP-2 invariants:
//!
//! * `unsafe_code = "deny"`.
//! * No `unwrap`/`expect` in non-test paths. Errors flow up via
//!   `anyhow::Result`.
//! * Default-deny: only HEAD-able + same-origin events captured;
//!   no exfiltration paths. (Future: TLS via rustls when we
//!   ever fetch external assets.)

#![forbid(unsafe_code)]

mod cdp_raw;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::audits::EnableParams as AuditsEnable;
use chromiumoxide::cdp::browser_protocol::log::EnableParams as LogEnable;
use chromiumoxide::cdp::browser_protocol::network::{
    EnableParams as NetworkEnable, EventLoadingFailed,
};
use chromiumoxide::cdp::browser_protocol::page::{
    AddScriptToEvaluateOnNewDocumentParams, EnableParams as PageEnable,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    EnableParams as RuntimeEnable, EventConsoleApiCalled, EventExceptionThrown,
};
use clap::Parser;
use crawler_detectors::autocomplete::{
    detect_autocomplete_issues, AutocompleteSnapshot, AUTOCOMPLETE_JS,
};
use crawler_detectors::cache_control::{build_cache_control_snapshot, detect_cache_control_issues};
use crawler_detectors::coep::{build_coep_snapshot, detect_coep_issues};
use crawler_detectors::content_security_policy::{build_csp_snapshot, detect_csp_issues};
use crawler_detectors::cookie_security::{
    build_cookie_security_snapshot, detect_cookie_security_issues,
};
use crawler_detectors::coop::{build_coop_snapshot, detect_coop_issues};
use crawler_detectors::corp::{build_corp_snapshot, detect_corp_issues};
use crawler_detectors::cross_page_meta_description::{
    detect_cross_page_meta_description_duplicates, new_cross_page_meta_description_accumulator,
    record_page_meta_description, CrossPageMetaDescriptionAccumulator,
};
use crawler_detectors::cross_page_title::{
    detect_cross_page_title_duplicates, new_cross_page_title_accumulator, record_page_title,
    CrossPageTitleAccumulator,
};
use crawler_detectors::css_health::{
    brace_counts_js, detect_css_health_issues, split_close_braces, BraceCountsRaw, ComputedBody,
    ComputedHtml, CssHealthSnapshot, StylesheetObservation, APPLIED_RULE_COUNT_JS,
    BODY_VISIBLE_TEXT_LENGTH_JS, COMPUTED_STYLES_JS, DECLARED_HREFS_JS,
    INLINE_STYLE_BLOCK_COUNT_JS,
};
use crawler_detectors::doc_title::{detect_doc_title_issues, DocTitleSnapshot, DOC_TITLE_JS};
use crawler_detectors::document_policy::{
    build_document_policy_snapshot, detect_document_policy_issues,
};
use crawler_detectors::favicon::{detect_favicon_issues, FaviconSnapshot, FAVICON_JS};
use crawler_detectors::font_loading::{detect_font_loading_issues, FontLoadingSnapshot};
use crawler_detectors::form_labels::{
    detect_form_label_issues, FormLabelsSnapshot, FORM_LABELS_JS,
};
use crawler_detectors::heading_order::{
    detect_heading_order_issues, HeadingOrderSnapshot, HEADING_ORDER_JS,
};
use crawler_detectors::hsts::{build_hsts_snapshot, detect_hsts_issues};
use crawler_detectors::html_lang::{detect_html_lang_issues, HtmlLangSnapshot, HTML_LANG_JS};
use crawler_detectors::info_leak_headers::{build_info_leak_snapshot, detect_info_leak_issues};
use crawler_detectors::link_text::{detect_link_text_issues, LinkTextSnapshot, LINK_TEXT_JS};
use crawler_detectors::link_underline::{
    detect_link_underline_issues, LinkUnderlineSnapshot, LINK_UNDERLINE_JS,
};
use crawler_detectors::meta_description::{
    detect_meta_description_issues, MetaDescriptionSnapshot, META_DESCRIPTION_JS,
};
use crawler_detectors::mixed_content::{
    detect_mixed_content_issues, MixedContentSnapshot, MIXED_CONTENT_JS,
};
use crawler_detectors::network_error_logging::{build_nel_snapshot, detect_nel_issues};
use crawler_detectors::origin_agent_cluster::{
    build_origin_agent_cluster_snapshot, detect_origin_agent_cluster_issues,
};
use crawler_detectors::outbound_links::{
    detect_outbound_link_issues, OutboundLinksSnapshot, OUTBOUND_LINKS_JS,
};
use crawler_detectors::permissions_policy::{
    build_permissions_policy_snapshot, detect_permissions_policy_issues,
};
use crawler_detectors::placeholder_text::{
    detect_placeholder_text_issues, PlaceholderTextSnapshot, PLACEHOLDER_TEXT_DOM_CAPTURE_JS,
};
use crawler_detectors::referrer_policy::{
    build_referrer_policy_snapshot, detect_referrer_policy_issues,
};
use crawler_detectors::reporting_endpoints::{
    build_reporting_endpoints_snapshot, detect_reporting_endpoints_issues,
};
use crawler_detectors::runtime_contrast::{
    detect_runtime_contrast_issues, RuntimeContrastSnapshot, RUNTIME_CONTRAST_JS,
};
use crawler_detectors::runtime_focus::{
    detect_runtime_focus_issues, RuntimeFocusSnapshot, RUNTIME_FOCUS_JS,
};
use crawler_detectors::runtime_images::{
    detect_runtime_image_issues, RuntimeImagesSnapshot, RUNTIME_IMAGES_JS,
};
use crawler_detectors::runtime_landmarks::{
    detect_runtime_landmarks_issues, RuntimeLandmarksSnapshot, RUNTIME_LANDMARKS_JS,
};
use crawler_detectors::skip_link::{detect_skip_link_issues, SkipLinkSnapshot, SKIP_LINK_JS};
use crawler_detectors::speculation_rules::{
    detect_speculation_rules_issues, SpeculationRulesSnapshot, SPECULATION_RULES_DOM_CAPTURE_JS,
};
use crawler_detectors::sri::{detect_sri_issues, SriSnapshot, SRI_DOM_CAPTURE_JS};
use crawler_detectors::tap_targets::{
    detect_tap_target_issues, TapTargetsSnapshot, TAP_TARGETS_JS,
};
use crawler_detectors::trusted_types_runtime::{detect_trusted_types_issues, TrustedTypesSnapshot};
use crawler_detectors::ui_overflow::{
    detect_ui_overflow_issues, Severity as UiSeverity, UiOverflowSnapshot, UI_OVERFLOW_JS,
};
use crawler_detectors::vary_header::{build_vary_snapshot, detect_vary_issues};
use crawler_detectors::viewport_meta::{
    detect_viewport_meta_issues, ViewportMetaSnapshot, VIEWPORT_META_JS,
};
use crawler_detectors::web_vitals::{classify, RawVitals, COLLECT_JS, WIRE_CALLBACKS_JS};
use crawler_detectors::x_frame_options::{
    build_x_frame_options_snapshot, detect_x_frame_options_issues,
};
use crawler_detectors::{AxisFinding, AxisSeverity};
use crawler_journey::Step;
use crawler_report::{
    CapturedEvent, EventKind, Report, ReportCounts, Severity as ReportSeverity, Viewport,
};
use futures::StreamExt;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "crawler",
    version,
    about = "PlausiDen-Crawler — chromiumoxide-based journey runner."
)]
struct Args {
    /// Journey JSON path.
    #[arg(long)]
    journey: PathBuf,

    /// Override the journey's baseUrl.
    #[arg(long)]
    url: Option<String>,

    /// Output directory (default: runs/).
    #[arg(long, default_value = "runs")]
    out_dir: PathBuf,

    /// Run headless (default true). `--no-headless` for visual debug.
    #[arg(long, default_value_t = true)]
    headless: bool,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();
    match run().await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("crawler: fatal: {e:#}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<ExitCode> {
    let args = Args::parse();
    let journey = crawler_journey::load(&args.journey)
        .with_context(|| format!("loading journey {}", args.journey.display()))?;

    info!(
        "crawler {} journey={} target={}",
        env!("CARGO_PKG_VERSION"),
        journey.name,
        args.url
            .as_deref()
            .or(journey.base_url.as_deref())
            .unwrap_or("<none>"),
    );

    let target = args
        .url
        .clone()
        .or_else(|| journey.base_url.clone())
        .unwrap_or_default();

    // Run output dir.
    let ts = iso_ts();
    let run_dir = args.out_dir.join(format!("{}-{}", journey.name, ts));
    tokio::fs::create_dir_all(&run_dir)
        .await
        .with_context(|| format!("create run dir {}", run_dir.display()))?;

    // Launch Chromium.
    //
    // BUG ASSUMPTION: chromiumoxide 0.9 dropped the `with_head()`
    // builder method and replaced `arg("--no-sandbox")` with the
    // semantic `.no_sandbox()` toggle. We always call no_sandbox
    // for now — Chromium refuses to launch as root otherwise,
    // and this binary is run from claude-code as root. Future
    // hardening: detect uid != 0 and only set when needed (or
    // require an explicit `--allow-no-sandbox` flag).
    //
    // T75 (2026-05-17): honor --no-headless. When the operator
    // passes `--no-headless` the crawler now opens a real browser
    // window for visual debug. Default remains headless (matches
    // the chromiumoxide 0.9 default of HeadlessMode::True).
    let mut builder = BrowserConfig::builder().no_sandbox();
    if !args.headless {
        builder = builder.with_head();
    }
    let config = builder
        .build()
        .map_err(|e| anyhow::anyhow!("BrowserConfig: {e}"))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .context("launching Chromium — is `chromium` on PATH?")?;
    let browser_handle = tokio::spawn(async move {
        // Pump the CDP message loop. Errors here are normal at
        // shutdown; logged at debug only.
        while let Some(h) = handler.next().await {
            if let Err(e) = h {
                tracing::debug!("cdp handler: {e}");
            }
        }
    });

    let started_epoch_ms = epoch_millis();
    let started_iso = ts.clone();
    let run_start = Instant::now();

    // Event accumulator — shared between page subscriptions + main run.
    let events: Arc<Mutex<Vec<CapturedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    // Per-URL network observations — populated by raw CDP, read by
    // css_health detector for status / content-type / body-bytes /
    // error-text. T103.4.
    let network: cdp_raw::NetworkObservations =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    // T75 (2026-05-17): per-journey cross-page accumulators. Detector
    // calls fire at journey end (after all steps), not per-step.
    let cross_page_title_acc: Arc<Mutex<CrossPageTitleAccumulator>> =
        Arc::new(Mutex::new(new_cross_page_title_accumulator()));
    let cross_page_desc_acc: Arc<Mutex<CrossPageMetaDescriptionAccumulator>> =
        Arc::new(Mutex::new(new_cross_page_meta_description_accumulator()));
    let started_at = Instant::now();

    // T102.4: spawn raw-CDP capture BEFORE creating the page so
    // we don't miss the about:blank → user-target attach.
    let ws_url = browser.websocket_address().clone();
    let raw_capture =
        cdp_raw::spawn_raw_cdp_capture(ws_url, events.clone(), network.clone(), started_at)
            .await
            .context("starting raw-CDP capture")?;

    let page = browser
        .new_page("about:blank")
        .await
        .context("creating page")?;

    // T75 (2026-05-17): install the Trusted Types runtime probe via
    // Page.addScriptToEvaluateOnNewDocument BEFORE any user script
    // runs. The probe monkey-patches innerHTML / outerHTML /
    // document.write / setTimeout(string) etc. to record every
    // sink assignment into window.__loomTTSinks for later
    // classification. Best-effort — failure logs at warn and the
    // detector silently emits no findings for the affected page.
    if let Err(e) = install_trusted_types_probe(&page).await {
        warn!("trusted-types probe install failed: {e}");
    }

    // T103.5: web-vitals capture. Inject the vendored
    // web-vitals.iife.js + WIRE_CALLBACKS_JS as an init script so
    // every navigation gets the global `webVitals` object and the
    // window.__lfiVitals accumulator BEFORE any page-side script
    // runs. The script must be concatenated INTO ONE source string
    // because addScriptToEvaluateOnNewDocument runs each registered
    // script in isolation (no shared scope) on each new document.
    //
    // BUG ASSUMPTION: web-vitals.iife.js lives at
    // {crawler-root}/node_modules/web-vitals/dist/web-vitals.iife.js.
    // If absent (production runner without node_modules), we skip
    // the inject step + log a warn — the runner still functions,
    // just without LCP/CLS/INP data.
    if let Some(vitals_js) = load_web_vitals_iife().await {
        let combined = format!("{vitals_js}\n{WIRE_CALLBACKS_JS}");
        if let Err(e) = page.add_init_script(combined).await {
            warn!("web-vitals add_init_script failed: {e}");
        } else {
            tracing::debug!("web-vitals init script registered");
        }
    } else {
        warn!("web-vitals.iife.js not found — vitals capture disabled this run");
    }

    // T102.3: explicit CDP domain enables. chromiumoxide 0.9
    // does NOT auto-enable domains when you subscribe to typed
    // events — the wire stays silent until each domain is
    // turned on via `<Domain>.enable`. We enable everything we
    // intend to listen on, in the order Playwright does:
    //   Network — for loadingFailed / responseReceived
    //   Page    — for frame/document lifecycle (currently
    //             unused but cheap; future-proof)
    //   Runtime — for consoleAPICalled / exceptionThrown
    //   Audits  — for issueAdded (CSP, mixed-content) — T84
    //   Log     — for entryAdded (CSP browser-emitted text) — T84
    //
    // Each enable is best-effort: if a domain is unavailable on
    // this Chromium build (rare), log + continue.
    if let Err(e) = page.execute(NetworkEnable::default()).await {
        warn!("Network.enable failed: {e}");
    }
    if let Err(e) = page.execute(PageEnable::default()).await {
        warn!("Page.enable failed: {e}");
    }
    if let Err(e) = page.execute(RuntimeEnable::default()).await {
        warn!("Runtime.enable failed: {e}");
    }
    if let Err(e) = page.execute(AuditsEnable::default()).await {
        warn!("Audits.enable failed: {e}");
    }
    if let Err(e) = page.execute(LogEnable::default()).await {
        warn!("Log.enable failed: {e}");
    }

    // (typed event_listener subscriptions kept for future use
    // when the chromiumoxide Message enum catches up to Chromium
    // 147; raw-CDP capture handles the actual event flow today)
    // Subscribe to the three MVP axes BEFORE first navigation.
    {
        let events = events.clone();
        let mut stream = page
            .event_listener::<EventConsoleApiCalled>()
            .await
            .context("subscribing to consoleAPICalled")?;
        tokio::spawn(async move {
            while let Some(ev) = stream.next().await {
                let level = format!("{:?}", ev.r#type).to_lowercase();
                let text = ev
                    .args
                    .iter()
                    .filter_map(|a| a.value.clone())
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut g = events.lock().await;
                g.push(CapturedEvent {
                    t: started_at.elapsed().as_millis() as u64,
                    kind: EventKind::Console,
                    level: Some(level),
                    text,
                    url: None,
                    status: None,
                    stack: None,
                    impact: None,
                    rule_id: None,
                    severity: None,
                });
            }
        });
    }
    {
        let events = events.clone();
        let mut stream = page
            .event_listener::<EventExceptionThrown>()
            .await
            .context("subscribing to exceptionThrown")?;
        tokio::spawn(async move {
            while let Some(ev) = stream.next().await {
                let text = ev
                    .exception_details
                    .exception
                    .as_ref()
                    .and_then(|x| x.description.clone())
                    .unwrap_or_else(|| "page error".to_owned());
                let stack = ev
                    .exception_details
                    .stack_trace
                    .as_ref()
                    .map(|s| format!("{:?}", s));
                let mut g = events.lock().await;
                g.push(CapturedEvent {
                    t: started_at.elapsed().as_millis() as u64,
                    kind: EventKind::Pageerror,
                    level: None,
                    text,
                    url: None,
                    status: None,
                    stack,
                    impact: None,
                    rule_id: None,
                    severity: None,
                });
            }
        });
    }
    {
        let events = events.clone();
        let mut stream = page
            .event_listener::<EventLoadingFailed>()
            .await
            .context("subscribing to loadingFailed")?;
        tokio::spawn(async move {
            while let Some(ev) = stream.next().await {
                let mut g = events.lock().await;
                g.push(CapturedEvent {
                    t: started_at.elapsed().as_millis() as u64,
                    kind: EventKind::RequestFailed,
                    level: None,
                    text: ev.error_text.clone(),
                    url: None, // CDP loadingFailed doesn't carry URL — joined via requestId in a follow-up
                    status: None,
                    stack: None,
                    impact: None,
                    rule_id: None,
                    severity: None,
                });
            }
        });
    }

    // Run journey steps. After each `wait` step (or directly
    // after a `goto` if no following wait), run the detector
    // axes — same cadence as the TS Crawler. Snapshot points
    // are explicit so we capture exactly what the operator
    // intended (the wait gives JS time to settle).
    let mut steps_ok = 0u32;
    let mut steps_failed = 0u32;
    // T75 (2026-05-17): track per-step StepResult so Report.steps
    // is no longer hard-coded empty. Wire-compat with the TS
    // Crawler's report.json — the diff tooling already consumes
    // this shape (newlyBrokenSteps / fixedSteps in Diff).
    let mut step_results: Vec<crawler_report::StepResult> = Vec::with_capacity(journey.steps.len());
    for (i, step) in journey.steps.iter().enumerate() {
        let label = step.label().unwrap_or("");
        info!(
            "step {}/{}: {} {}",
            i + 1,
            journey.steps.len(),
            step.kind(),
            label
        );
        let step_started = Instant::now();
        let outcome = run_step(&page, step, &run_dir).await;
        let duration_ms = step_started.elapsed().as_millis() as u64;
        let step_json = serde_json::to_value(step).unwrap_or(serde_json::Value::Null);
        match &outcome {
            Ok(()) => {
                steps_ok += 1;
                step_results.push(crawler_report::StepResult {
                    index: i as u32,
                    ok: true,
                    duration_ms,
                    step: step_json,
                    error: None,
                });
            }
            Err(e) => {
                warn!("step {} failed: {e}", i + 1);
                steps_failed += 1;
                step_results.push(crawler_report::StepResult {
                    index: i as u32,
                    ok: false,
                    duration_ms,
                    step: step_json,
                    error: Some(format!("{e:#}")),
                });
            }
        }
        // T103.2 + T103.3: run detector axes after every `wait`
        // step (DOM-settle moment). Best-effort each — a single
        // detector failure does not fail the run.
        if matches!(step, Step::Wait { .. }) {
            if let Err(e) = capture_ui_overflow(&page, &events, started_at).await {
                tracing::debug!("ui_overflow snapshot failed: {e}");
            }
            if let Err(e) = capture_runtime_contrast(&page, &events, started_at).await {
                tracing::debug!("runtime_contrast snapshot failed: {e}");
            }
            if let Err(e) = capture_runtime_images(&page, &events, started_at).await {
                tracing::debug!("runtime_images snapshot failed: {e}");
            }
            if let Err(e) = capture_runtime_focus(&page, &events, started_at).await {
                tracing::debug!("runtime_focus snapshot failed: {e}");
            }
            if let Err(e) = capture_heading_order(&page, &events, started_at).await {
                tracing::debug!("heading_order snapshot failed: {e}");
            }
            if let Err(e) = capture_runtime_landmarks(&page, &events, started_at).await {
                tracing::debug!("runtime_landmarks snapshot failed: {e}");
            }
            if let Err(e) = capture_link_text(&page, &events, started_at).await {
                tracing::debug!("link_text snapshot failed: {e}");
            }
            // T75 batch wiring 2026-05-17: 4 new detectors
            // (form_labels, skip_link, tap_targets, doc_title)
            if let Err(e) = capture_form_labels(&page, &events, started_at).await {
                tracing::debug!("form_labels snapshot failed: {e}");
            }
            if let Err(e) = capture_skip_link(&page, &events, started_at).await {
                tracing::debug!("skip_link snapshot failed: {e}");
            }
            if let Err(e) = capture_tap_targets(&page, &events, started_at).await {
                tracing::debug!("tap_targets snapshot failed: {e}");
            }
            if let Err(e) = capture_doc_title(&page, &events, started_at).await {
                tracing::debug!("doc_title snapshot failed: {e}");
            }
            if let Err(e) = capture_placeholder_text(&page, &events, started_at).await {
                tracing::debug!("placeholder_text snapshot failed: {e}");
            }
            if let Err(e) = capture_viewport_meta(&page, &events, started_at).await {
                tracing::debug!("viewport_meta snapshot failed: {e}");
            }
            if let Err(e) = capture_html_lang(&page, &events, started_at).await {
                tracing::debug!("html_lang snapshot failed: {e}");
            }
            if let Err(e) = capture_favicon(&page, &events, started_at).await {
                tracing::debug!("favicon snapshot failed: {e}");
            }
            // T75 response-header batch (2026-05-17): hsts is the
            // first detector wired through the new
            // NetworkObservation.headers map. Pattern repeats for
            // csp / vary / sri / corp / etc. in subsequent cycles.
            let cur_url = page.url().await.ok().flatten().unwrap_or_default();
            if let Err(e) = capture_hsts(&cur_url, &network, &events, started_at).await {
                tracing::debug!("hsts snapshot failed: {e}");
            }
            if let Err(e) = capture_referrer_policy(&cur_url, &network, &events, started_at).await {
                tracing::debug!("referrer_policy snapshot failed: {e}");
            }
            if let Err(e) = capture_x_frame_options(&cur_url, &network, &events, started_at).await {
                tracing::debug!("x_frame_options snapshot failed: {e}");
            }
            if let Err(e) =
                capture_permissions_policy(&cur_url, &network, &events, started_at).await
            {
                tracing::debug!("permissions_policy snapshot failed: {e}");
            }
            if let Err(e) = capture_vary(&cur_url, &network, &events, started_at).await {
                tracing::debug!("vary snapshot failed: {e}");
            }
            if let Err(e) = capture_csp(&cur_url, &network, &events, started_at).await {
                tracing::debug!("csp snapshot failed: {e}");
            }
            if let Err(e) = capture_cookie_security(&cur_url, &network, &events, started_at).await {
                tracing::debug!("cookie_security snapshot failed: {e}");
            }
            if let Err(e) = capture_coep(&cur_url, &network, &events, started_at).await {
                tracing::debug!("coep snapshot failed: {e}");
            }
            if let Err(e) = capture_coop(&cur_url, &network, &events, started_at).await {
                tracing::debug!("coop snapshot failed: {e}");
            }
            if let Err(e) = capture_document_policy(&cur_url, &network, &events, started_at).await {
                tracing::debug!("document_policy snapshot failed: {e}");
            }
            if let Err(e) = capture_info_leak(&cur_url, &network, &events, started_at).await {
                tracing::debug!("info_leak snapshot failed: {e}");
            }
            if let Err(e) =
                capture_origin_agent_cluster(&cur_url, &network, &events, started_at).await
            {
                tracing::debug!("origin_agent_cluster snapshot failed: {e}");
            }
            if let Err(e) = capture_cache_control(&cur_url, &network, &events, started_at).await {
                tracing::debug!("cache_control snapshot failed: {e}");
            }
            if let Err(e) = capture_nel(&cur_url, &network, &events, started_at).await {
                tracing::debug!("nel snapshot failed: {e}");
            }
            if let Err(e) =
                capture_reporting_endpoints(&cur_url, &network, &events, started_at).await
            {
                tracing::debug!("reporting_endpoints snapshot failed: {e}");
            }
            if let Err(e) = capture_speculation_rules(&page, &events, started_at).await {
                tracing::debug!("speculation_rules snapshot failed: {e}");
            }
            if let Err(e) = capture_autocomplete(&page, &events, started_at).await {
                tracing::debug!("autocomplete snapshot failed: {e}");
            }
            if let Err(e) = capture_link_underline(&page, &events, started_at).await {
                tracing::debug!("link_underline snapshot failed: {e}");
            }
            if let Err(e) = capture_mixed_content(&page, &events, started_at).await {
                tracing::debug!("mixed_content snapshot failed: {e}");
            }
            if let Err(e) = capture_outbound_links(&page, &events, started_at).await {
                tracing::debug!("outbound_links snapshot failed: {e}");
            }
            if let Err(e) = capture_meta_description(&page, &events, started_at).await {
                tracing::debug!("meta_description snapshot failed: {e}");
            }
            if let Err(e) = capture_corp(&cur_url, &network, &events, started_at).await {
                tracing::debug!("corp snapshot failed: {e}");
            }
            if let Err(e) = capture_sri(&page, &events, started_at).await {
                tracing::debug!("sri snapshot failed: {e}");
            }
            if let Err(e) = capture_font_loading(&page, &events, started_at).await {
                tracing::debug!("font_loading snapshot failed: {e}");
            }
            if let Err(e) =
                capture_trusted_types(&page, &network, &cur_url, &events, started_at).await
            {
                tracing::debug!("trusted_types snapshot failed: {e}");
            }
            // T75 cross-page accumulation (2026-05-17).
            if let Err(e) = record_cross_page_state(
                &page,
                &cur_url,
                &cross_page_title_acc,
                &cross_page_desc_acc,
            )
            .await
            {
                tracing::debug!("cross_page_state record failed: {e}");
            }
            if let Err(e) = capture_css_health(&page, &events, &network, started_at).await {
                tracing::debug!("css_health snapshot failed: {e}");
            }
        }
    }

    // T103.5: collect web-vitals before tearing down the page.
    // CLS / INP finalize on hide; we read the accumulator for
    // whatever the last-loaded page measured. Best-effort — a
    // failure here doesn't fail the run.
    if let Err(e) = capture_web_vitals(&page, &events, started_at).await {
        tracing::debug!("web_vitals collect failed: {e}");
    }

    // T75 cross-page detector fire (2026-05-17). Per-journey: walks
    // the accumulators across all steps and emits any duplicate-
    // title / duplicate-description findings.
    {
        let title_acc = cross_page_title_acc.lock().await;
        let findings = detect_cross_page_title_duplicates(&title_acc);
        push_axis_findings(
            &events,
            findings,
            EventKind::CrossPageTitle,
            started_at.elapsed().as_millis() as u64,
        )
        .await;
    }
    {
        let desc_acc = cross_page_desc_acc.lock().await;
        let findings = detect_cross_page_meta_description_duplicates(&desc_acc);
        push_axis_findings(
            &events,
            findings,
            EventKind::CrossPageMetaDescription,
            started_at.elapsed().as_millis() as u64,
        )
        .await;
    }

    let duration_ms = run_start.elapsed().as_millis() as u64;

    // Drain pending events (CDP messages may still be in flight).
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Build the Report.
    let captured: Vec<CapturedEvent> = {
        let g = events.lock().await;
        g.clone()
    };
    let counts = compute_counts(&captured, steps_ok, steps_failed);
    let viewport = Viewport {
        w: journey.viewport.map(|v| v.w).unwrap_or(1280),
        h: journey.viewport.map(|v| v.h).unwrap_or(800),
    };
    let report = Report {
        target,
        journey: journey.name.clone(),
        viewport,
        started: started_iso,
        duration_ms,
        counts,
        events: captured,
        steps: step_results,
    };
    let report_path = run_dir.join("report.json");
    tokio::fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).context("serialize report")?,
    )
    .await
    .with_context(|| format!("writing {}", report_path.display()))?;

    info!(
        "run complete: {} ({}ms) — events: console={} pageerror={} failed-requests={}",
        report_path.display(),
        duration_ms,
        report.counts.console_errors,
        report.counts.page_errors,
        report.counts.failed_requests,
    );

    // Shutdown.
    raw_capture.abort();
    let _ = browser.close().await;
    let _ = browser_handle.await;
    let _ = started_epoch_ms;

    if steps_failed > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

/// T103.2: capture a uiOverflow snapshot via raw page.evaluate,
/// run the pure detection logic, push CapturedEvents.
///
/// BUG ASSUMPTION: page.evaluate runs in the page's main world.
/// If a journey injects `addInitScript` that hijacks
/// `window.getBoundingClientRect` etc., the snapshot will be
/// corrupted. Today no journey does that; the detector trusts
/// the page-side measurements.
async fn capture_ui_overflow(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(UI_OVERFLOW_JS).await?;
    let snap: UiOverflowSnapshot = result
        .into_value()
        .context("deserialize uiOverflow snapshot")?;
    let findings = detect_ui_overflow_issues(&snap);
    if findings.is_empty() {
        return Ok(());
    }
    let t = started_at.elapsed().as_millis() as u64;
    let mut g = events.lock().await;
    for f in findings {
        let severity = match f.severity {
            UiSeverity::Strict => ReportSeverity::Strict,
            UiSeverity::Warn => ReportSeverity::Warn,
        };
        g.push(CapturedEvent {
            t,
            kind: EventKind::UiOverflow,
            level: None,
            text: format!("[{}] {}", f.kind, f.detail),
            url: None,
            status: None,
            stack: None,
            impact: None,
            rule_id: Some(f.kind),
            severity: Some(severity),
        });
    }
    Ok(())
}

/// Map a per-axis `AxisSeverity` to the report's `Severity`.
fn map_axis_severity(s: AxisSeverity) -> ReportSeverity {
    match s {
        AxisSeverity::Strict => ReportSeverity::Strict,
        AxisSeverity::Warn => ReportSeverity::Warn,
    }
}

/// Push axis findings as `CapturedEvent`s of the given kind.
async fn push_axis_findings(
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    findings: Vec<AxisFinding>,
    kind: EventKind,
    t_ms: u64,
) {
    if findings.is_empty() {
        return;
    }
    let mut g = events.lock().await;
    for f in findings {
        g.push(CapturedEvent {
            t: t_ms,
            kind,
            level: None,
            text: format!("[{}] {}", f.kind, f.detail),
            url: None,
            status: None,
            stack: None,
            impact: None,
            rule_id: Some(f.kind),
            severity: Some(map_axis_severity(f.severity)),
        });
    }
}

/// T103.3: capture a runtime-contrast snapshot via raw page.evaluate.
async fn capture_runtime_contrast(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(RUNTIME_CONTRAST_JS).await?;
    let snap: RuntimeContrastSnapshot = result
        .into_value()
        .context("deserialize runtimeContrast snapshot")?;
    let findings = detect_runtime_contrast_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::RuntimeContrast,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T103.3: capture a runtime-images snapshot via raw page.evaluate.
async fn capture_runtime_images(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(RUNTIME_IMAGES_JS).await?;
    let snap: RuntimeImagesSnapshot = result
        .into_value()
        .context("deserialize runtimeImages snapshot")?;
    let findings = detect_runtime_image_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::RuntimeImages,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T106: capture link-text snapshot. Walks every visible
/// `<a href>`, computes accessible name (textContent + aria-*),
/// flags empty (strict) or generic (warn) link text per WCAG 2.4.4.
async fn capture_link_text(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(LINK_TEXT_JS).await?;
    let snap: LinkTextSnapshot = result
        .into_value()
        .context("deserialize linkText snapshot")?;
    let findings = detect_link_text_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::LinkText,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T105: capture runtime-landmarks snapshot. Pure DOM walk;
/// counts main/banner/contentinfo/nav/aside + flags same-role
/// nesting.
async fn capture_runtime_landmarks(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(RUNTIME_LANDMARKS_JS).await?;
    let snap: RuntimeLandmarksSnapshot = result
        .into_value()
        .context("deserialize runtimeLandmarks snapshot")?;
    let findings = detect_runtime_landmarks_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::RuntimeLandmarks,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T104: capture heading-order snapshot. Pure DOM walk (no
/// computed styles, no focus interactions) — cheap, runs on
/// every Wait step.
async fn capture_heading_order(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(HEADING_ORDER_JS).await?;
    let snap: HeadingOrderSnapshot = result
        .into_value()
        .context("deserialize headingOrder snapshot")?;
    let findings = detect_heading_order_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::HeadingOrder,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T103.3: capture a runtime-focus snapshot via raw page.evaluate.
async fn capture_runtime_focus(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(RUNTIME_FOCUS_JS).await?;
    let snap: RuntimeFocusSnapshot = result
        .into_value()
        .context("deserialize runtimeFocus snapshot")?;
    let findings = detect_runtime_focus_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::RuntimeFocus,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): formLabels detector — every form
/// control has an accessible label (WCAG 1.3.1 + 3.3.2).
async fn capture_form_labels(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(FORM_LABELS_JS).await?;
    let snap: FormLabelsSnapshot = result
        .into_value()
        .context("deserialize formLabels snapshot")?;
    let findings = detect_form_label_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::FormLabels,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): skipLink detector — first focusable
/// link is a same-page jump to #main / #content.
async fn capture_skip_link(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(SKIP_LINK_JS).await?;
    let snap: SkipLinkSnapshot = result
        .into_value()
        .context("deserialize skipLink snapshot")?;
    let findings = detect_skip_link_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::SkipLink,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): tapTargets detector — interactive
/// elements meet the 24×24 px AAA / 44×44 px iOS minimum size.
async fn capture_tap_targets(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(TAP_TARGETS_JS).await?;
    let snap: TapTargetsSnapshot = result
        .into_value()
        .context("deserialize tapTargets snapshot")?;
    let findings = detect_tap_target_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::TapTargets,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): docTitle detector — `<title>`
/// present, non-empty.
async fn capture_doc_title(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(DOC_TITLE_JS).await?;
    let snap: DocTitleSnapshot = result
        .into_value()
        .context("deserialize docTitle snapshot")?;
    let findings = detect_doc_title_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::DocTitle,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): placeholderText detector — sentinel
/// text (TODO / Lorem ipsum / "delete me") in rendered DOM.
async fn capture_placeholder_text(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(PLACEHOLDER_TEXT_DOM_CAPTURE_JS).await?;
    let snap: PlaceholderTextSnapshot = result
        .into_value()
        .context("deserialize placeholderText snapshot")?;
    let findings = detect_placeholder_text_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::PlaceholderText,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): viewportMeta detector.
async fn capture_viewport_meta(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(VIEWPORT_META_JS).await?;
    let snap: ViewportMetaSnapshot = result
        .into_value()
        .context("deserialize viewportMeta snapshot")?;
    let findings = detect_viewport_meta_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::ViewportMeta,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): htmlLang detector.
async fn capture_html_lang(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(HTML_LANG_JS).await?;
    let snap: HtmlLangSnapshot = result
        .into_value()
        .context("deserialize htmlLang snapshot")?;
    let findings = detect_html_lang_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::HtmlLang,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): favicon detector.
async fn capture_favicon(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(FAVICON_JS).await?;
    let snap: FaviconSnapshot = result
        .into_value()
        .context("deserialize favicon snapshot")?;
    let findings = detect_favicon_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Favicon,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): hsts response-header detector.
/// FIRST consumer of the new cdp_raw NetworkObservation.headers
/// map — pattern for the ~20 response-header detectors that
/// follow (csp / vary / sri / corp / etc.).
async fn capture_hsts(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_hsts_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_hsts_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Hsts,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 helper: fetch the lowercased-header BTreeMap for the top-level
/// page URL. Empty map if the URL hasn't been observed by the CDP
/// network listener yet (race-safe).
async fn page_headers_btreemap(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
) -> std::collections::BTreeMap<String, String> {
    let net = network.lock().await;
    net.get(page_url)
        .map(|obs| obs.headers.clone())
        .unwrap_or_default()
}

/// T75 batch wiring (2026-05-17): referrerPolicy.
async fn capture_referrer_policy(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_referrer_policy_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_referrer_policy_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::ReferrerPolicy,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): xFrameOptions.
async fn capture_x_frame_options(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_x_frame_options_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_x_frame_options_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::XFrameOptions,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): permissionsPolicy.
async fn capture_permissions_policy(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_permissions_policy_snapshot(page_url, &headers);
    let findings = detect_permissions_policy_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::PermissionsPolicy,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): varyHeader.
async fn capture_vary(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_vary_snapshot(page_url, &headers);
    let findings = detect_vary_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::VaryHeader,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): contentSecurityPolicy.
async fn capture_csp(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_csp_snapshot(page_url, &headers);
    let findings = detect_csp_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::ContentSecurityPolicy,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): cookieSecurity.
async fn capture_cookie_security(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_cookie_security_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_cookie_security_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::CookieSecurity,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): coep.
async fn capture_coep(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_coep_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_coep_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Coep,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): coop.
async fn capture_coop(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_coop_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_coop_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Coop,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): documentPolicy.
async fn capture_document_policy(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_document_policy_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_document_policy_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::DocumentPolicy,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): infoLeakHeaders.
async fn capture_info_leak(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_info_leak_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_info_leak_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::InfoLeakHeaders,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): originAgentCluster.
async fn capture_origin_agent_cluster(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_origin_agent_cluster_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_origin_agent_cluster_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::OriginAgentCluster,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): networkErrorLogging (NEL).
async fn capture_nel(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_nel_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_nel_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Nel,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): reportingEndpoints.
async fn capture_reporting_endpoints(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_reporting_endpoints_snapshot(page_url, &headers);
    let findings = detect_reporting_endpoints_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::ReportingEndpoints,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): speculationRules (DOM-walk via
/// page.evaluate). Mirrors the existing DOM-walk pattern.
async fn capture_speculation_rules(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(SPECULATION_RULES_DOM_CAPTURE_JS).await?;
    let snap: SpeculationRulesSnapshot = result
        .into_value()
        .context("deserialize speculationRules snapshot")?;
    let findings = detect_speculation_rules_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::SpeculationRules,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring: autocomplete / link_underline / mixed_content /
/// outbound_links / meta_description — all DOM-walk detectors using
/// the standard page.evaluate(JS_CONST) → deserialise → detect pattern.
async fn capture_autocomplete(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(AUTOCOMPLETE_JS).await?;
    let snap: AutocompleteSnapshot = result
        .into_value()
        .context("deserialize autocomplete snapshot")?;
    let findings = detect_autocomplete_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Autocomplete,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

async fn capture_link_underline(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(LINK_UNDERLINE_JS).await?;
    let snap: LinkUnderlineSnapshot = result
        .into_value()
        .context("deserialize linkUnderline snapshot")?;
    let findings = detect_link_underline_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::LinkUnderline,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

async fn capture_mixed_content(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(MIXED_CONTENT_JS).await?;
    let snap: MixedContentSnapshot = result
        .into_value()
        .context("deserialize mixedContent snapshot")?;
    let findings = detect_mixed_content_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::MixedContent,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

async fn capture_outbound_links(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(OUTBOUND_LINKS_JS).await?;
    let snap: OutboundLinksSnapshot = result
        .into_value()
        .context("deserialize outboundLinks snapshot")?;
    let findings = detect_outbound_link_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::OutboundLinks,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

async fn capture_meta_description(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(META_DESCRIPTION_JS).await?;
    let snap: MetaDescriptionSnapshot = result
        .into_value()
        .context("deserialize metaDescription snapshot")?;
    let findings = detect_meta_description_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::MetaDescription,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): per-sub-resource helper. Walks
/// every URL in the network observations and returns its
/// lowercased-header BTreeMap. Used by `corp` (and future per-sub-
/// resource detectors). Race-safe — clones under the lock.
async fn all_headers_by_url(
    network: &crate::cdp_raw::NetworkObservations,
) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
    let net = network.lock().await;
    net.iter()
        .map(|(url, obs)| (url.clone(), obs.headers.clone()))
        .collect()
}

/// T75 batch wiring (2026-05-17): corp — per-sub-resource
/// Cross-Origin-Resource-Policy. First detector that consumes the
/// full per-URL headers map (vs the page URL's headers).
async fn capture_corp(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let page_headers = page_headers_btreemap(page_url, network).await;
    let all_headers = all_headers_by_url(network).await;
    let snap = build_corp_snapshot(page_url, &page_headers, &all_headers);
    let findings = detect_corp_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Corp,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): sri — Subresource Integrity
/// per-element DOM audit.
async fn capture_sri(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(SRI_DOM_CAPTURE_JS).await?;
    let snap: SriSnapshot = result.into_value().context("deserialize sri snapshot")?;
    let findings = detect_sri_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::Sri,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 wiring (2026-05-17): fontLoading. Walks document.styleSheets
/// in the browser, filters to CSSFontFaceRule, extracts family +
/// font-display + sheet href. cross-origin sheets that throw on
/// cssRules access are counted as `inaccessibleSheetCount` so a
/// detector with low `faces` count can distinguish "genuinely clean"
/// from "we couldn't see".
///
/// The DOM-capture JS lives here (not in the detector module)
/// because font_loading.rs predates the per-detector JS-const
/// convention. Migrating it into the detector module is queued —
/// for now the JS is verified by hand and the test surface in the
/// detector unit-tests covers the classifier with stub snapshots.
const FONT_LOADING_DOM_CAPTURE_JS: &str = r#"
(() => {
  const faces = [];
  let inaccessibleSheetCount = 0;
  const sheets = Array.from(document.styleSheets);
  for (const sheet of sheets) {
    const sheetHref = sheet.href || '';
    let rules = null;
    try {
      rules = sheet.cssRules;
    } catch (_) {
      inaccessibleSheetCount += 1;
      continue;
    }
    if (!rules) continue;
    for (const rule of Array.from(rules)) {
      // CSSRule.FONT_FACE_RULE === 5
      if (rule.type !== 5) continue;
      const style = rule.style || {};
      const family = (style.fontFamily || '').replace(/^["']|["']$/g, '').trim();
      const fontDisplay = (style.fontDisplay || '').trim().toLowerCase();
      faces.push({ family, fontDisplay, sheetHref });
    }
  }
  return { pageUrl: window.location.href, inaccessibleSheetCount, faces };
})()
"#;

async fn capture_font_loading(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let result = page.evaluate(FONT_LOADING_DOM_CAPTURE_JS).await?;
    let snap: FontLoadingSnapshot = result
        .into_value()
        .context("deserialize fontLoading snapshot")?;
    let findings = detect_font_loading_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::FontLoading,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 (2026-05-17): Trusted Types runtime probe. Monkey-patches
/// innerHTML / outerHTML / document.write / setTimeout(string) etc.
/// to record every assignment into `window.__loomTTSinks`. Must be
/// installed BEFORE any page script via CDP
/// `Page.addScriptToEvaluateOnNewDocument` — otherwise the probe
/// misses early sink writes.
///
/// Ported char-for-char from `src/trustedTypesRuntime.ts`
/// `installTrustedTypesProbe`. Errors during install are swallowed
/// per the doctrine that the probe must never break the page.
const TRUSTED_TYPES_PROBE_JS: &str = r#"
(function() {
  if (window.__loomTTProbeInstalled) return;
  window.__loomTTProbeInstalled = true;
  var sinks = [];
  var startedAt = performance.now();
  window.__loomTTSinks = sinks;
  function record(kind, value, trusted) {
    try {
      var preview = '';
      if (typeof value === 'string') preview = value;
      else if (value && typeof value.toString === 'function') preview = String(value);
      if (preview.length > 200) preview = preview.slice(0, 200);
      sinks.push({
        kind: kind, preview: preview, trusted: !!trusted,
        t: Math.round(performance.now() - startedAt),
      });
    } catch (e) { }
  }
  function isTrusted(v) {
    try {
      return !!(window.TrustedHTML && v instanceof window.TrustedHTML) ||
             !!(window.TrustedScript && v instanceof window.TrustedScript) ||
             !!(window.TrustedScriptURL && v instanceof window.TrustedScriptURL);
    } catch (e) { return false; }
  }
  try {
    var elProto = Element.prototype;
    var ihDesc = Object.getOwnPropertyDescriptor(elProto, 'innerHTML');
    if (ihDesc && ihDesc.set) {
      var origIH = ihDesc.set;
      Object.defineProperty(elProto, 'innerHTML', {
        configurable: true, enumerable: ihDesc.enumerable, get: ihDesc.get,
        set: function(v) {
          record('innerHTML', v, isTrusted(v));
          try { return origIH.call(this, v); } catch (e) { throw e; }
        },
      });
    }
    var ohDesc = Object.getOwnPropertyDescriptor(elProto, 'outerHTML');
    if (ohDesc && ohDesc.set) {
      var origOH = ohDesc.set;
      Object.defineProperty(elProto, 'outerHTML', {
        configurable: true, enumerable: ohDesc.enumerable, get: ohDesc.get,
        set: function(v) {
          record('outerHTML', v, isTrusted(v));
          try { return origOH.call(this, v); } catch (e) { throw e; }
        },
      });
    }
    var origIAH = elProto.insertAdjacentHTML;
    if (typeof origIAH === 'function') {
      elProto.insertAdjacentHTML = function(pos, html) {
        record('insertAdjacentHTML', html, isTrusted(html));
        return origIAH.call(this, pos, html);
      };
    }
  } catch (e) { }
  try {
    var origWrite = document.write;
    document.write = function() {
      for (var i = 0; i < arguments.length; i++)
        record('document.write', arguments[i], isTrusted(arguments[i]));
      return origWrite.apply(this, arguments);
    };
    var origWriteln = document.writeln;
    document.writeln = function() {
      for (var i = 0; i < arguments.length; i++)
        record('document.writeln', arguments[i], isTrusted(arguments[i]));
      return origWriteln.apply(this, arguments);
    };
  } catch (e) { }
  try {
    var origSetTimeout = window.setTimeout;
    window.setTimeout = function(handler) {
      if (typeof handler === 'string') record('setTimeout(string)', handler, isTrusted(handler));
      return origSetTimeout.apply(this, arguments);
    };
    var origSetInterval = window.setInterval;
    window.setInterval = function(handler) {
      if (typeof handler === 'string') record('setInterval(string)', handler, isTrusted(handler));
      return origSetInterval.apply(this, arguments);
    };
  } catch (e) { }
  try {
    if (typeof Range !== 'undefined' && Range.prototype.createContextualFragment) {
      var origCCF = Range.prototype.createContextualFragment;
      Range.prototype.createContextualFragment = function(html) {
        record('createContextualFragment', html, isTrusted(html));
        return origCCF.call(this, html);
      };
    }
  } catch (e) { }
})();
"#;

/// Install the Trusted Types probe on the page BEFORE its first
/// script runs. Called once per page lifecycle.
async fn install_trusted_types_probe(page: &chromiumoxide::Page) -> Result<()> {
    let _ = page
        .execute(AddScriptToEvaluateOnNewDocumentParams {
            source: TRUSTED_TYPES_PROBE_JS.to_owned(),
            world_name: None,
            include_command_line_api: None,
            run_immediately: Some(true),
        })
        .await
        .context("install trusted-types probe")?;
    Ok(())
}

/// T75 batch wiring (2026-05-17): trustedTypesRuntime. Reads the
/// sink-monitor accumulator + CSP context from the page, classifies.
/// Must be called AFTER install_trusted_types_probe and AFTER the
/// page has had a chance to run user scripts.
async fn capture_trusted_types(
    page: &chromiumoxide::Page,
    network: &crate::cdp_raw::NetworkObservations,
    page_url: &str,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let csp_header = {
        let net = network.lock().await;
        net.get(page_url)
            .and_then(|obs| obs.headers.get("content-security-policy").cloned())
            .unwrap_or_default()
    };
    let v = page
        .evaluate(
            "(() => { \
              var sinks = (window.__loomTTSinks || []); \
              var hasScripts = document.querySelectorAll('script').length > 0; \
              var metaCsp = ''; \
              var metas = document.querySelectorAll('meta[http-equiv]'); \
              for (var i = 0; i < metas.length; i++) { \
                var equiv = (metas[i].getAttribute('http-equiv') || '').toLowerCase(); \
                if (equiv === 'content-security-policy') { metaCsp = metas[i].getAttribute('content') || ''; break; } \
              } \
              return { sinks: sinks, hasScripts: hasScripts, metaCsp: metaCsp }; \
            })()",
        )
        .await?;
    #[derive(serde::Deserialize)]
    struct Raw {
        sinks: Vec<crawler_detectors::trusted_types_runtime::CapturedTrustedTypesSink>,
        #[serde(rename = "hasScripts")]
        has_scripts: bool,
        #[serde(rename = "metaCsp")]
        meta_csp: String,
    }
    let raw: Raw = v
        .into_value()
        .context("deserialize trusted-types raw snapshot")?;
    let csp_text = if csp_header.is_empty() {
        raw.meta_csp
    } else {
        csp_header
    };
    let mut has_require_directive = false;
    let mut trusted_types_directive = String::new();
    for seg in csp_text.split(';') {
        let t = seg.trim();
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("require-trusted-types-for") {
            has_require_directive = true;
        } else if let Some(rest) = lower.strip_prefix("trusted-types") {
            if rest.is_empty() || rest.starts_with(' ') {
                trusted_types_directive = t
                    [lower.find("trusted-types").unwrap_or(0) + "trusted-types".len()..]
                    .trim()
                    .to_owned();
            }
        }
    }
    let snap = TrustedTypesSnapshot::new(
        page_url.to_owned(),
        raw.sinks,
        has_require_directive,
        trusted_types_directive,
        raw.has_scripts,
    );
    let findings = detect_trusted_types_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::TrustedTypes,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T75 batch wiring (2026-05-17): cross-page state accumulation.
/// One page.evaluate that captures `document.title` + the
/// `<meta name="description">` content, then records BOTH into
/// their respective journey-level accumulators. The
/// `detect_cross_page_*_duplicates` calls fire at journey end (in
/// `run()`), not per-step.
async fn record_cross_page_state(
    page: &chromiumoxide::Page,
    page_url: &str,
    title_acc: &Arc<Mutex<CrossPageTitleAccumulator>>,
    desc_acc: &Arc<Mutex<CrossPageMetaDescriptionAccumulator>>,
) -> Result<()> {
    let v = page
        .evaluate(
            "(() => ({ title: document.title || '', \
              description: (document.querySelector('meta[name=\"description\"]') || {}).content || '' }))()",
        )
        .await?;
    #[derive(serde::Deserialize)]
    struct Pair {
        title: String,
        description: String,
    }
    let pair: Pair = v
        .into_value()
        .context("deserialize cross-page title+description pair")?;
    {
        let mut acc = title_acc.lock().await;
        record_page_title(&mut acc, page_url, &pair.title);
    }
    {
        let mut acc = desc_acc.lock().await;
        record_page_meta_description(&mut acc, page_url, &pair.description);
    }
    Ok(())
}

/// T75 batch wiring (2026-05-17): cacheControl.
async fn capture_cache_control(
    page_url: &str,
    network: &crate::cdp_raw::NetworkObservations,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let headers = page_headers_btreemap(page_url, network).await;
    let snap = build_cache_control_snapshot(
        page_url,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    let findings = detect_cache_control_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::CacheControl,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

/// T103.5: locate and read web-vitals.iife.js from one of the
/// expected vendoring sites. Returns `None` if not found — the
/// runner then skips vitals capture rather than failing the run.
///
/// BUG ASSUMPTION: the resolution order matches what TS Crawler's
/// pipeline expects. If the runner is shipped from a release
/// directory without node_modules, the operator must vendor the
/// IIFE into a sibling `vendor/` directory or set
/// CRAWLER_WEB_VITALS_PATH explicitly.
async fn load_web_vitals_iife() -> Option<String> {
    if let Ok(path) = std::env::var("CRAWLER_WEB_VITALS_PATH") {
        if let Ok(s) = tokio::fs::read_to_string(&path).await {
            return Some(s);
        }
    }
    let candidates = [
        "node_modules/web-vitals/dist/web-vitals.iife.js",
        "../../node_modules/web-vitals/dist/web-vitals.iife.js",
        "../node_modules/web-vitals/dist/web-vitals.iife.js",
        "vendor/web-vitals.iife.js",
    ];
    for c in candidates {
        if let Ok(s) = tokio::fs::read_to_string(c).await {
            return Some(s);
        }
    }
    None
}

/// Map a vitals band to a Report severity. `Good` → None
/// (filtered out before push); other bands map to warn / strict.
fn band_to_severity(band: crawler_detectors::web_vitals::Band) -> Option<ReportSeverity> {
    use crawler_detectors::web_vitals::Band;
    match band {
        Band::Good => None,
        Band::NeedsImprovement => Some(ReportSeverity::Warn),
        Band::Poor => Some(ReportSeverity::Strict),
        _ => Some(ReportSeverity::Warn), // forward-compat for new bands
    }
}

/// T103.5: capture web-vitals via the page-side accumulator.
/// Pulls window.__lfiVitals, classifies bands, and pushes one
/// CapturedEvent per non-good metric.
async fn capture_web_vitals(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    started_at: Instant,
) -> Result<()> {
    let raw_val = page.evaluate(COLLECT_JS).await?;
    let raw: RawVitals = raw_val.into_value().context("deserialize web-vitals raw")?;
    let snap = classify(raw, epoch_millis());
    let t_ms = started_at.elapsed().as_millis() as u64;
    let mut findings = Vec::<CapturedEvent>::new();

    let mut push = |rule_id: &str, text: String, severity: ReportSeverity| {
        findings.push(CapturedEvent {
            t: t_ms,
            kind: EventKind::WebVitals,
            level: None,
            text,
            url: None,
            status: None,
            stack: None,
            impact: None,
            rule_id: Some(rule_id.to_owned()),
            severity: Some(severity),
        });
    };

    if let Some(b) = &snap.lcp {
        if let Some(sev) = band_to_severity(b.band) {
            push(
                "web_vitals.lcp",
                format!("[lcp] {:.0}ms ({:?})", b.value, b.band),
                sev,
            );
        }
    }
    if let Some(b) = &snap.cls {
        if let Some(sev) = band_to_severity(b.band) {
            push(
                "web_vitals.cls",
                format!("[cls] {:.3} ({:?})", b.value, b.band),
                sev,
            );
        }
    }
    if let Some(b) = &snap.inp {
        if let Some(sev) = band_to_severity(b.band) {
            push(
                "web_vitals.inp",
                format!("[inp] {:.0}ms ({:?})", b.value, b.band),
                sev,
            );
        }
    }

    if !findings.is_empty() {
        let mut g = events.lock().await;
        for f in findings {
            g.push(f);
        }
    }
    Ok(())
}

/// T103.4: capture a CSS-health snapshot via 6 sequential
/// `page.evaluate` calls, then run the pure detection logic.
///
/// BUG ASSUMPTION: the page-side `BRACE_COUNTS_JS` does
/// `fetch(url, {cache:'no-store'})` for each declared sheet.
/// The fetch counts the bytes the visitor would actually see
/// (post-decode), independent of what `Network.loadingFinished`
/// reports as `encodedDataLength`. We use both: brace counts
/// from same-origin fetch, body bytes from CDP `loadingFinished`.
///
/// Cross-origin sheets fail the fetch and return `null` brace
/// counts — that's expected and is not an error. The detector
/// silently ignores nulls in the brace-density heuristics.
async fn capture_css_health(
    page: &chromiumoxide::Page,
    events: &Arc<Mutex<Vec<CapturedEvent>>>,
    network: &cdp_raw::NetworkObservations,
    started_at: Instant,
) -> Result<()> {
    let page_url = page.url().await?.unwrap_or_default();

    // (1) declared <link rel="stylesheet"> hrefs.
    let hrefs_val = page.evaluate(DECLARED_HREFS_JS).await?;
    let declared_hrefs: Vec<String> = hrefs_val
        .into_value()
        .context("deserialize declared hrefs")?;

    // (2) same-origin fetch: brace counts per URL.
    let (open_braces, close_braces) = if declared_hrefs.is_empty() {
        (
            std::collections::HashMap::<String, Option<u32>>::new(),
            std::collections::HashMap::<String, u32>::new(),
        )
    } else {
        let raw_val = page.evaluate(brace_counts_js(&declared_hrefs)).await?;
        let raw: BraceCountsRaw = raw_val.into_value().context("deserialize brace counts")?;
        split_close_braces(raw)
    };

    // (3) <style> block count.
    let inline_blocks_val = page.evaluate(INLINE_STYLE_BLOCK_COUNT_JS).await?;
    let inline_style_block_count: u32 = inline_blocks_val
        .into_value()
        .context("deserialize inline style block count")?;

    // (4) computed body + html.
    let cs_val = page.evaluate(COMPUTED_STYLES_JS).await?;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ComputedStylesRaw {
        body: ComputedBody,
        html: ComputedHtml,
    }
    let cs: ComputedStylesRaw = cs_val.into_value().context("deserialize computed styles")?;

    // (5) body visible text length.
    let body_len_val = page.evaluate(BODY_VISIBLE_TEXT_LENGTH_JS).await?;
    let body_visible_text_length: u32 = body_len_val
        .into_value()
        .context("deserialize body visible text length")?;

    // (6) applied-rule count estimate.
    let arc_val = page.evaluate(APPLIED_RULE_COUNT_JS).await?;
    let applied_rule_count_estimate: u32 = arc_val
        .into_value()
        .context("deserialize applied rule count")?;

    // Merge network observations + brace counts into per-sheet rows.
    let net = network.lock().await;
    let declared_sheets: Vec<StylesheetObservation> = declared_hrefs
        .iter()
        .map(|url| {
            let obs = net.get(url);
            let (status, content_type, body_bytes, error_text, from_network) = match obs {
                Some(o) => (
                    o.status,
                    o.content_type.clone(),
                    o.body_bytes,
                    o.error_text.clone(),
                    true,
                ),
                None => (0, None, 0, None, false),
            };
            StylesheetObservation {
                url: url.clone(),
                status,
                content_type,
                body_bytes,
                declared_brace_count: open_braces.get(url).copied().flatten(),
                declared_close_brace_count: close_braces.get(url).copied(),
                from_network,
                error_text,
            }
        })
        .collect();
    drop(net);

    let snap = CssHealthSnapshot {
        page_url,
        declared_sheets,
        inline_style_block_count,
        computed_body: cs.body,
        computed_html: cs.html,
        body_visible_text_length,
        applied_rule_count_estimate,
    };
    let findings = detect_css_health_issues(&snap);
    push_axis_findings(
        events,
        findings,
        EventKind::CssHealth,
        started_at.elapsed().as_millis() as u64,
    )
    .await;
    Ok(())
}

async fn run_step(
    page: &chromiumoxide::Page,
    step: &Step,
    run_dir: &std::path::Path,
) -> Result<()> {
    match step {
        Step::Goto { url, timeout, .. } => {
            // Per-step timeout (T75 entrypoint polish 2026-05-17):
            // honor the journey's `timeout` field. None means
            // fall back to a 30s default so a hung navigation
            // doesn't wedge the whole run. The default mirrors
            // Playwright's navigationTimeout default for parity
            // with the TS port we're replacing.
            let nav_timeout = Duration::from_millis(timeout.map_or(30_000u64, u64::from));
            let goto_fut = async {
                page.goto(url.as_str()).await?.wait_for_navigation().await?;
                Ok::<(), anyhow::Error>(())
            };
            match tokio::time::timeout(nav_timeout, goto_fut).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "goto({}) exceeded {}ms timeout",
                        url,
                        nav_timeout.as_millis()
                    ));
                }
            }
        }
        Step::Wait { ms, .. } => {
            tokio::time::sleep(Duration::from_millis(*ms as u64)).await;
        }
        Step::Screenshot { label } => {
            let path = run_dir.join(format!("{label}.png"));
            let opts = chromiumoxide::page::ScreenshotParams::builder()
                .full_page(true)
                .build();
            let bytes = page.screenshot(opts).await?;
            tokio::fs::write(&path, bytes).await?;
        }
        Step::Click { selector, .. } => {
            page.find_element(selector).await?.click().await?;
        }
        Step::Press { key, selector, .. } => {
            // T75 (2026-05-17): replaced the synthetic-JS
            // KeyboardEvent dispatch with chromiumoxide's
            // `press_key`, which routes through CDP's real
            // `Input.dispatchKeyEvent`. The synthetic-event hack
            // didn't trigger browser default actions (End scrolled
            // nothing, Tab didn't change focus, Enter didn't submit
            // forms); the real dispatch does.
            //
            // Press the key against an element (Element::press_key
            // routes through DispatchKeyEvent). If a selector is
            // supplied, focus that element; otherwise fall back to
            // `body` so the key still dispatches into the document.
            let target = match selector {
                Some(sel) => page.find_element(sel.as_str()).await?,
                None => page.find_element("body").await?,
            };
            target.press_key(key.as_str()).await?;
        }
        Step::Scroll {
            selector, position, ..
        } => {
            // MVP: scroll page to position (top / bottom / pixel).
            let pos = position.as_deref().unwrap_or("bottom");
            let js = match (selector.as_deref(), pos) {
                (None, "top") => "window.scrollTo(0, 0)".to_owned(),
                (None, "bottom") => "window.scrollTo(0, document.body.scrollHeight)".to_owned(),
                (Some(sel), "bottom") => format!(
                    "document.querySelector('{}').scrollTop = document.querySelector('{}').scrollHeight",
                    escape_js_string(sel),
                    escape_js_string(sel)
                ),
                _ => "window.scrollTo(0, document.body.scrollHeight)".to_owned(),
            };
            let _ = page.evaluate(js.as_str()).await?;
        }
        Step::WaitForSelector {
            selector, timeout, ..
        } => {
            // T75 (2026-05-17): honor the per-step timeout. Previously
            // `find_element` was awaited indefinitely, which could
            // wedge a journey if the selector never resolved. Default
            // 30s mirrors Playwright's locator timeout for parity
            // with the TS port being replaced.
            let wait_timeout = Duration::from_millis(timeout.map_or(30_000u64, u64::from));
            let target_sel = selector.clone();
            let find_fut = async {
                page.find_element(target_sel).await?;
                Ok::<(), anyhow::Error>(())
            };
            match tokio::time::timeout(wait_timeout, find_fut).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "waitForSelector({}) exceeded {}ms timeout",
                        selector,
                        wait_timeout.as_millis()
                    ));
                }
            }
        }
        Step::Discover { .. } | Step::Probe { .. } => {
            // Runner-specific steps; MVP no-op.
        }
        // BUG ASSUMPTION: `Step` is `#[non_exhaustive]` for
        // forward-compat. New step kinds must explicitly land
        // here; a `_` arm would silently no-op them.
        _ => {
            warn!("step kind not yet handled in MVP runner: {}", step.kind());
        }
    }
    Ok(())
}

fn compute_counts(events: &[CapturedEvent], steps_ok: u32, steps_failed: u32) -> ReportCounts {
    let mut c = ReportCounts {
        steps_ok,
        steps_failed,
        ..Default::default()
    };
    for e in events {
        c.total += 1;
        match e.kind {
            EventKind::Console if matches!(e.level.as_deref(), Some("error")) => {
                c.console_errors += 1
            }
            EventKind::Pageerror => c.page_errors += 1,
            EventKind::RequestFailed | EventKind::ResponseError => c.failed_requests += 1,
            EventKind::A11yViolation => c.a11y_violations += 1,
            EventKind::CssHealth => c.css_health_findings += 1,
            EventKind::UiOverflow => c.ui_overflow_findings += 1,
            EventKind::RuntimeContrast => c.runtime_contrast_findings += 1,
            EventKind::RuntimeImages => c.runtime_images_findings += 1,
            EventKind::RuntimeFocus => c.runtime_focus_findings += 1,
            EventKind::WebVitals => c.web_vitals_findings += 1,
            EventKind::CspViolation => c.csp_violations += 1,
            _ => {}
        }
    }
    c
}

fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn iso_ts() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = json!({}); // keep serde_json imported (silences unused-import in MVP)
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    let day_secs = secs % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z",
        1970 + (secs / 31_557_600) as u32,
        ((secs / 2_629_800) % 12) + 1,
        ((secs / 86_400) % 31) + 1,
        hours,
        mins,
        day_secs
    )
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_ts_format_shape() {
        let s = iso_ts();
        assert_eq!(s.len(), 20, "expected ISO-ish 20-char shape: {s}");
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn escape_js_string_escapes_quote() {
        assert_eq!(escape_js_string("'foo'"), "\\'foo\\'");
        assert_eq!(escape_js_string("a\\b"), "a\\\\b");
    }

    #[test]
    fn compute_counts_basic() {
        let events = vec![
            CapturedEvent {
                t: 0,
                kind: EventKind::Console,
                level: Some("error".to_owned()),
                text: "x".to_owned(),
                url: None,
                status: None,
                stack: None,
                impact: None,
                rule_id: None,
                severity: None,
            },
            CapturedEvent {
                t: 1,
                kind: EventKind::Pageerror,
                level: None,
                text: "y".to_owned(),
                url: None,
                status: None,
                stack: None,
                impact: None,
                rule_id: None,
                severity: None,
            },
        ];
        let c = compute_counts(&events, 5, 0);
        assert_eq!(c.console_errors, 1);
        assert_eq!(c.page_errors, 1);
        assert_eq!(c.steps_ok, 5);
        assert_eq!(c.total, 2);
    }

    #[test]
    fn map_axis_severity_round_trips_known_variants() {
        assert!(matches!(
            map_axis_severity(AxisSeverity::Strict),
            ReportSeverity::Strict
        ));
        assert!(matches!(
            map_axis_severity(AxisSeverity::Warn),
            ReportSeverity::Warn
        ));
    }

    #[test]
    fn band_to_severity_classifies_web_vitals_bands_correctly() {
        use crawler_detectors::web_vitals::Band;
        // Good band ⇒ no event (None) — we don't surface healthy
        // metrics to keep report.json focused on regressions.
        assert!(band_to_severity(Band::Good).is_none());
        // NeedsImprovement ⇒ Warn (advisory).
        assert!(matches!(
            band_to_severity(Band::NeedsImprovement),
            Some(ReportSeverity::Warn)
        ));
        // Poor ⇒ Strict (operator must address).
        assert!(matches!(
            band_to_severity(Band::Poor),
            Some(ReportSeverity::Strict)
        ));
    }
}
