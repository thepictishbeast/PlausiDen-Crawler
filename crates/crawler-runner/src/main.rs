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
use chromiumoxide::cdp::browser_protocol::page::EnableParams as PageEnable;
use chromiumoxide::cdp::js_protocol::runtime::{
    EnableParams as RuntimeEnable, EventConsoleApiCalled, EventExceptionThrown,
};
use clap::Parser;
use crawler_detectors::css_health::{
    brace_counts_js, detect_css_health_issues, split_close_braces, BraceCountsRaw, ComputedBody,
    ComputedHtml, CssHealthSnapshot, StylesheetObservation, APPLIED_RULE_COUNT_JS,
    BODY_VISIBLE_TEXT_LENGTH_JS, COMPUTED_STYLES_JS, DECLARED_HREFS_JS,
    INLINE_STYLE_BLOCK_COUNT_JS,
};
use crawler_detectors::heading_order::{
    detect_heading_order_issues, HeadingOrderSnapshot, HEADING_ORDER_JS,
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
use crawler_detectors::ui_overflow::{
    detect_ui_overflow_issues, Severity as UiSeverity, UiOverflowSnapshot, UI_OVERFLOW_JS,
};
use crawler_detectors::web_vitals::{classify, RawVitals, COLLECT_JS, WIRE_CALLBACKS_JS};
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
    let config = BrowserConfig::builder()
        .no_sandbox()
        .build()
        .map_err(|e| anyhow::anyhow!("BrowserConfig: {e}"))?;
    let _ = args.headless; // 0.9 is headless by default; --no-headless TBD next tick

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
    for (i, step) in journey.steps.iter().enumerate() {
        let label = step.label().unwrap_or("");
        info!(
            "step {}/{}: {} {}",
            i + 1,
            journey.steps.len(),
            step.kind(),
            label
        );
        match run_step(&page, step, &run_dir).await {
            Ok(()) => steps_ok += 1,
            Err(e) => {
                warn!("step {} failed: {e}", i + 1);
                steps_failed += 1;
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
        steps: vec![], // Per-step result objects deferred to next tick.
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
            let _ = timeout; // TODO honor per-step timeout
            page.goto(url.as_str()).await?.wait_for_navigation().await?;
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
            // chromiumoxide: simulate via keyboard input on the
            // selected element (fall back to the page if no
            // selector is given). This is a partial impl; full
            // key dispatch matrix lands in next tick.
            let _ = selector;
            // The chromiumoxide Page exposes keyboard helpers
            // through CDP `Input.dispatchKeyEvent`. Wrap in a
            // simple call for now.
            let key_text = key.clone();
            let _ = page.evaluate(format!(
                "document.activeElement && document.activeElement.dispatchEvent(new KeyboardEvent('keydown', {{ key: '{}', bubbles: true }}))",
                escape_js_string(&key_text)
            ).as_str()).await?;
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
            selector,
            timeout: _,
            ..
        } => {
            page.find_element(selector).await?;
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
}
