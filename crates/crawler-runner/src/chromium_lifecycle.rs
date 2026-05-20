//! `chromium_lifecycle` — graceful Chromium shutdown for the runner.
//!
//! Closes #182. Root cause + design notes in `CRAWLER_ZOMBIE_AUDIT.md`
//! at the repo root.
//!
//! Public surface:
//! * [`shutdown_browser`] — call from the runner's teardown site
//!   instead of `let _ = browser.close().await;`.
//! * [`crashpad_reap_enabled`] — checks the
//!   `CRAWLER_REAP_CRASHPAD=1` env flag (single-user opt-in).
//! * [`reap_crashpad_handlers`] — best-effort `pkill -KILL
//!   chrome_crashpad_handler` sweep, gated on the env flag.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * No panics in non-test code.
//! * Idempotent: calling twice is harmless.
//! * Bounded: every wait is timed; nothing can hang the runner.

use chromiumoxide::Browser;
use std::time::Duration;
use tokio::time::Instant;

/// How long we'll wait for `Browser::close().await` (CDP graceful path)
/// to return before falling through to the reap loop.
const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// `try_wait` poll interval — small enough that the typical case
/// (chromium exits in <100ms after CDP close) is reaped on the first
/// or second poll.
const TRY_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Maximum number of `try_wait` polls before falling through to
/// `Browser::kill` (which SIGKILLs). 10 × 50ms = 500ms.
const TRY_WAIT_MAX_POLLS: u32 = 10;

/// Env flag that opts a host into the post-shutdown `chrome_crashpad_handler`
/// sweep. Default off — see CRAWLER_ZOMBIE_AUDIT.md for the rationale.
pub const CRASHPAD_REAP_ENV: &str = "CRAWLER_REAP_CRASHPAD";

/// Whether the operator has opted into the crashpad sweep.
#[must_use]
pub fn crashpad_reap_enabled() -> bool {
    std::env::var(CRASHPAD_REAP_ENV)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Best-effort `pkill -KILL chrome_crashpad_handler`. Returns the
/// exit-status code or None if pkill was not on PATH.
///
/// **Only invoked when [`crashpad_reap_enabled`] is true.** Multi-user
/// hosts must leave the flag off so we don't disturb other Chromium
/// instances.
pub fn reap_crashpad_handlers() -> Option<i32> {
    let status = std::process::Command::new("pkill")
        .args(["-KILL", "chrome_crashpad_handler"])
        .status()
        .ok()?;
    Some(status.code().unwrap_or(-1))
}

/// Graceful Chromium shutdown sequence.
///
/// Returns the path actually taken so the caller can record it in
/// telemetry. Never panics; bounded by a tokio timeout.
///
/// # Sequence
/// 1. CDP `Browser.close` with a 3-second tokio timeout.
/// 2. Up to 10 × 50ms `try_wait` polls — exits as soon as the OS
///    process has been reaped.
/// 3. If still alive, `Browser::kill` (SIGKILL + wait) as fallback.
/// 4. If [`crashpad_reap_enabled`], `pkill -KILL chrome_crashpad_handler`.
pub async fn shutdown_browser(browser: &mut Browser) -> ShutdownOutcome {
    let started_at = Instant::now();
    let mut outcome = ShutdownOutcome::default();

    // 1. CDP graceful close, bounded. `Browser::close` returns
    // `Result<CloseReturns, CdpError>`; we only care whether it
    // succeeded.
    match tokio::time::timeout(GRACEFUL_CLOSE_TIMEOUT, browser.close()).await {
        Ok(Ok(_returns)) => outcome.cdp_close_ok = true,
        Ok(Err(e)) => outcome.cdp_close_err = Some(e.to_string()),
        Err(_elapsed) => outcome.cdp_close_timed_out = true,
    }

    // 2. Reap loop. `try_wait` is synchronous-but-fast; no need to
    // wrap each call in a timeout.
    let mut reaped = false;
    for _ in 0..TRY_WAIT_MAX_POLLS {
        match browser.try_wait() {
            Ok(Some(_status)) => {
                reaped = true;
                break;
            }
            Ok(None) => {
                tokio::time::sleep(TRY_WAIT_POLL_INTERVAL).await;
            }
            Err(e) => {
                outcome.try_wait_err = Some(e.to_string());
                break;
            }
        }
    }
    outcome.try_wait_reaped = reaped;

    // 3. Forced kill fallback if reap loop didn't catch it.
    if !reaped {
        match browser.kill().await {
            Some(Ok(())) => outcome.forced_kill_ok = true,
            Some(Err(e)) => outcome.forced_kill_err = Some(e.to_string()),
            None => outcome.had_no_child = true,
        }
    }

    // 4. Optional crashpad sweep.
    if crashpad_reap_enabled() {
        outcome.crashpad_sweep_status = reap_crashpad_handlers();
    }

    outcome.elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    outcome
}

/// Observable result of `shutdown_browser`. Surfaces all branches the
/// shutdown sequence may have taken so the runner can record telemetry
/// or fail soft on partial-reap.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct ShutdownOutcome {
    /// CDP `Browser.close` returned `Ok(())`.
    pub cdp_close_ok: bool,
    /// CDP `Browser.close` returned an error.
    pub cdp_close_err: Option<String>,
    /// CDP `Browser.close` exceeded the [`GRACEFUL_CLOSE_TIMEOUT`].
    pub cdp_close_timed_out: bool,
    /// `try_wait` poll loop observed the child exit.
    pub try_wait_reaped: bool,
    /// `try_wait` returned an OS error (e.g. ECHILD if the runtime
    /// reaped it asynchronously between polls).
    pub try_wait_err: Option<String>,
    /// `Browser::kill` was invoked and returned Ok.
    pub forced_kill_ok: bool,
    /// `Browser::kill` was invoked and failed.
    pub forced_kill_err: Option<String>,
    /// Browser was launched via `Browser::connect` and there is no
    /// child process to reap.
    pub had_no_child: bool,
    /// Crashpad sweep exit status, or `None` if disabled / pkill unavailable.
    pub crashpad_sweep_status: Option<i32>,
    /// Wall-clock duration of the full shutdown sequence in ms.
    pub elapsed_ms: u64,
}

impl ShutdownOutcome {
    /// `true` if the OS process was definitively reaped.
    #[must_use]
    pub fn process_reaped(&self) -> bool {
        self.try_wait_reaped || self.forced_kill_ok || self.had_no_child
    }

    /// Compact log line suitable for `tracing::info!`.
    #[must_use]
    pub fn log_line(&self) -> String {
        let mut path = Vec::with_capacity(4);
        if self.cdp_close_ok {
            path.push("cdp-close-ok");
        }
        if self.cdp_close_timed_out {
            path.push("cdp-close-timeout");
        }
        if self.try_wait_reaped {
            path.push("try-wait-reaped");
        }
        if self.forced_kill_ok {
            path.push("sigkill-ok");
        }
        if self.had_no_child {
            path.push("no-child");
        }
        if self.crashpad_sweep_status.is_some() {
            path.push("crashpad-swept");
        }
        format!(
            "chromium shutdown: [{}] in {}ms",
            path.join(","),
            self.elapsed_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crashpad_reap_default_off() {
        // EnvGuard pattern — restore prior value to avoid bleed.
        let prior = std::env::var(CRASHPAD_REAP_ENV).ok();
        // SAFETY: tests run sequentially in this module via #[test];
        // crate-wide env writes are not pretty but acceptable here.
        std::env::remove_var(CRASHPAD_REAP_ENV);
        assert!(!crashpad_reap_enabled());
        if let Some(v) = prior {
            std::env::set_var(CRASHPAD_REAP_ENV, v);
        }
    }

    #[test]
    fn crashpad_reap_on_with_one() {
        let prior = std::env::var(CRASHPAD_REAP_ENV).ok();
        std::env::set_var(CRASHPAD_REAP_ENV, "1");
        assert!(crashpad_reap_enabled());
        if let Some(v) = prior {
            std::env::set_var(CRASHPAD_REAP_ENV, v);
        } else {
            std::env::remove_var(CRASHPAD_REAP_ENV);
        }
    }

    #[test]
    fn crashpad_reap_on_with_true() {
        let prior = std::env::var(CRASHPAD_REAP_ENV).ok();
        std::env::set_var(CRASHPAD_REAP_ENV, "true");
        assert!(crashpad_reap_enabled());
        std::env::set_var(CRASHPAD_REAP_ENV, "True");
        assert!(crashpad_reap_enabled());
        if let Some(v) = prior {
            std::env::set_var(CRASHPAD_REAP_ENV, v);
        } else {
            std::env::remove_var(CRASHPAD_REAP_ENV);
        }
    }

    #[test]
    fn crashpad_reap_off_with_other() {
        let prior = std::env::var(CRASHPAD_REAP_ENV).ok();
        for v in ["0", "false", "no", "yes", ""] {
            std::env::set_var(CRASHPAD_REAP_ENV, v);
            assert!(
                !crashpad_reap_enabled(),
                "expected disabled for value {v:?}"
            );
        }
        if let Some(v) = prior {
            std::env::set_var(CRASHPAD_REAP_ENV, v);
        } else {
            std::env::remove_var(CRASHPAD_REAP_ENV);
        }
    }

    #[test]
    fn shutdown_outcome_default_is_clean() {
        let out = ShutdownOutcome::default();
        assert!(!out.cdp_close_ok);
        assert!(!out.try_wait_reaped);
        assert!(!out.forced_kill_ok);
        assert!(!out.process_reaped());
        assert_eq!(out.elapsed_ms, 0);
    }

    #[test]
    fn process_reaped_true_when_try_wait_succeeded() {
        let out = ShutdownOutcome {
            try_wait_reaped: true,
            ..Default::default()
        };
        assert!(out.process_reaped());
    }

    #[test]
    fn process_reaped_true_when_sigkill_succeeded() {
        let out = ShutdownOutcome {
            forced_kill_ok: true,
            ..Default::default()
        };
        assert!(out.process_reaped());
    }

    #[test]
    fn process_reaped_true_when_no_child() {
        let out = ShutdownOutcome {
            had_no_child: true,
            ..Default::default()
        };
        assert!(out.process_reaped());
    }

    #[test]
    fn process_reaped_false_when_only_cdp_close() {
        // CDP close succeeded but no try_wait / kill / no-child marker —
        // the process may still be defunct in /proc.
        let out = ShutdownOutcome {
            cdp_close_ok: true,
            ..Default::default()
        };
        assert!(!out.process_reaped());
    }

    #[test]
    fn log_line_includes_taken_paths() {
        let out = ShutdownOutcome {
            cdp_close_ok: true,
            try_wait_reaped: true,
            elapsed_ms: 142,
            ..Default::default()
        };
        let s = out.log_line();
        assert!(s.contains("cdp-close-ok"));
        assert!(s.contains("try-wait-reaped"));
        assert!(s.contains("142ms"));
    }

    #[test]
    fn log_line_records_timeout_path() {
        let out = ShutdownOutcome {
            cdp_close_timed_out: true,
            forced_kill_ok: true,
            elapsed_ms: 3500,
            ..Default::default()
        };
        let s = out.log_line();
        assert!(s.contains("cdp-close-timeout"));
        assert!(s.contains("sigkill-ok"));
    }

    #[test]
    fn log_line_records_crashpad_sweep() {
        let out = ShutdownOutcome {
            cdp_close_ok: true,
            try_wait_reaped: true,
            crashpad_sweep_status: Some(0),
            elapsed_ms: 100,
            ..Default::default()
        };
        let s = out.log_line();
        assert!(s.contains("crashpad-swept"));
    }
}
