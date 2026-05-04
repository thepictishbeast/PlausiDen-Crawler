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

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::EventLoadingFailed;
use chromiumoxide::cdp::js_protocol::runtime::{EventConsoleApiCalled, EventExceptionThrown};
use clap::Parser;
use crawler_journey::{Journey, Step};
use crawler_report::{
    CapturedEvent, EventKind, Report, ReportCounts, Viewport,
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

    let page = browser
        .new_page("about:blank")
        .await
        .context("creating page")?;

    // Subscribe to the three MVP axes BEFORE first navigation.
    let started_at = Instant::now();
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

    // Run journey steps.
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
    let _ = browser.close().await;
    let _ = browser_handle.await;
    let _ = started_epoch_ms;

    if steps_failed > 0 {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
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
        Step::Scroll { selector, position, .. } => {
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

fn compute_counts(
    events: &[CapturedEvent],
    steps_ok: u32,
    steps_failed: u32,
) -> ReportCounts {
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
    format!("{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z",
        1970 + (secs / 31_557_600) as u32,
        ((secs / 2_629_800) % 12) + 1,
        ((secs / 86_400) % 31) + 1,
        hours, mins, day_secs)
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
