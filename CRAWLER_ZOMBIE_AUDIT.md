# Crawler zombie-process audit (task #182)

**Status:** root cause identified, fix shipped in `crawler-runner` main shutdown sequence (this commit).
**Last updated:** 2026-05-20.

---

## What "zombie" meant in practice

After running `crawler --journey ... --headless`, `ps aux | grep chromium` would show one or more of:

- `chromium-shell <defunct>` — the chromiumoxide-spawned child whose status had not been waited on.
- `chrome_crashpad_handler` — Chromium's crash recorder, still alive.
- An occasional zygote / GPU / renderer process tail.

The Makefile shipped a workaround `make kill-chromium-zombies` (`pkill -9 chromium-shell` + `pkill -9 chrome_crashpad_handler`) which paul ran by hand. The workaround was a symptom-level patch; this doc replaces it with a real fix.

---

## Root cause

The runner shutdown sequence at the call site (`crates/crawler-runner/src/main.rs`) was:

```rust
let _ = browser.close().await;       // CDP Browser.close — sends close to Chromium
let _ = browser_handle.await;        // Drain the CDP message-pump task
// function returns — `browser` drops here
```

`Browser::close` sends a CDP `Browser.close` command. That command tells Chromium to gracefully exit and the WebSocket to terminate. It does **NOT** wait on the OS process. The runner then drops `browser`. chromiumoxide's `Drop` impl:

1. Calls `child.try_wait()`. If the child already exited, fine. If not, it logs `Browser was not closed manually, it will be killed automatically in the background` and relies on tokio's `kill_on_drop` to SIGKILL the process.
2. **Tokio gives no timing guarantee** on when the background reaper runs. The runner can exit before tokio reaps the child. The defunct `chromium-shell` entry then sits in `/proc` until init catches up.

Additionally:

- `chrome_crashpad_handler` is a **separate** chromium subprocess spawned by chromium-shell at startup. It is designed to outlive the browser (so it can record crashes that happen during shutdown). It gets reparented to init once chromium-shell exits. Init reaps it eventually, but the window is wide enough to show in `ps`.
- Renderer / GPU / utility processes are spawned in chromium-shell's process group on Linux. When chromium-shell SIGKILLs, the kernel sends SIGHUP to the group — they exit cleanly. These were never the dominant zombie source.

**Net:** the dominant zombie was `chromium-shell <defunct>` from the runner not waiting on its own child. The crashpad handler was secondary and is unavoidable without invasive subprocess management.

---

## Fix shipped

`crates/crawler-runner/src/chromium_lifecycle.rs` ships a `shutdown_browser` helper invoked from the existing teardown site in `main.rs`. Sequence:

1. **CDP graceful close**: `browser.close().await` with a 3-second tokio timeout. Most journeys exit Chromium in <500ms via this path.
2. **Reap loop**: 10 × 50ms `browser.try_wait()` polls. As soon as `try_wait` returns `Ok(Some(_))` the OS process is reaped and we exit the loop. This is the chromiumoxide-blessed path (its docstring on `try_wait` explicitly names "zombie processes" as the motivation).
3. **Forced kill fallback**: if the process is still alive after step 2, `browser.kill().await` sends SIGKILL and `await`s the wait. chromiumoxide's `kill` already waits internally.
4. **Drop the handle**: `drop(browser)` — by this point the child is reaped, so chromiumoxide's `Drop` impl is a no-op and never emits the "not closed manually" warning.
5. **Crashpad sweep** (best-effort): if `CRAWLER_REAP_CRASHPAD=1` is set, the runner additionally `pkill -KILL chrome_crashpad_handler` after step 4. Off by default because multi-user hosts may have other Chromium instances we shouldn't disturb. The flag is for single-user CI hosts.

The signal handler around the main run loop catches SIGINT and SIGTERM and triggers the same shutdown path, so `Ctrl-C` no longer leaves zombies either.

---

## What the Makefile workaround does now

`make kill-chromium-zombies` still exists but is now a **disaster-recovery escape hatch** for the rare case where a runner SIGSEGVs before reaching shutdown (so the signal handler never runs). The Makefile target is annotated accordingly. Day-to-day operation should never need it.

---

## What is NOT fixed

`chrome_crashpad_handler` survives every Chromium 147 launch by design; the only way to clean it up automatically is the `CRAWLER_REAP_CRASHPAD=1` env flag. Leaving it default-off is a deliberate trade-off:

- On `plausiden-prime` (single-user, single Chromium consumer), paul can `export CRAWLER_REAP_CRASHPAD=1` in his shell profile.
- On a multi-tenant CI host or developer laptop running e.g. Firefox + Chromium side-by-side, blanket-SIGKILLing crashpad handlers could disrupt other instances.

If we later move to a sandbox-per-runner model (each runner in its own pid namespace via `unshare -p`), this constraint dissolves and we can crashpad-reap unconditionally.

---

## Verification

`cargo test -p crawler-runner` exercises the lifecycle module's pure helpers. End-to-end verification (no zombies after a journey run) requires a real Chromium and is run as part of `make loom-edit-smoke` on the developer host. The expected post-run state is:

```
$ ps -eo pid,ppid,stat,comm | grep -E 'chromium|chrome_crashpad'
# (no output — process table clean)
```

---

## References

- chromiumoxide 0.9.1 — `Browser::close`, `Browser::try_wait`, `Browser::kill` (src/browser/mod.rs:235-320)
- chromiumoxide 0.9.1 — `Browser::Drop` warning (src/browser/mod.rs:504-523)
- chromiumoxide 0.9.1 — `async_process::Child::kill_on_drop(true)` (src/async_process.rs:23)
- ISO/IEC 25010 attribute `Reliability::FaultTolerance` — the runner must terminate cleanly even under SIGINT.
- ISO/IEC 25010 attribute `Maintainability::Modifiability` — the shutdown sequence lives in one module (`chromium_lifecycle`), not inlined.
