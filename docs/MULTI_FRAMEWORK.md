# Multi-framework integration — Playwright + Puppeteer + Crawlee + EasySpider

User directive 2026-04-19: "Use Playwright, Puppeteer, EasySpider, and Crawlee inside the crawler. Combine them all. Improve. Increase stress testing and speeds. Better at collecting data. Scrape console logs and correlate with what the crawler was doing when they fired."

## Evaluation per tool

| Tool            | License | Role                                                                       | Fit for us |
|-----------------|---------|----------------------------------------------------------------------------|------------|
| **Playwright**  | Apache 2.0 (Microsoft) | Modern multi-browser driver. CDP access, rich events, traces.     | ✅ **Primary driver.** Already in v0.2. |
| **Puppeteer**   | Apache 2.0 (Google) | Chromium-focused driver. Slightly tighter CDP control than Playwright. | ✅ **Secondary driver** for Chromium-only stress runs (supports `puppeteer-extra` stealth, bulk concurrency). |
| **Crawlee**     | Apache 2.0 (Apify)  | Scraper/crawler framework *built on top of* Playwright + Puppeteer + Cheerio. Queue mgmt, session pool, proxy rotation, auto-concurrency, storage adapters. | ✅ **Orchestration layer.** Perfect for stress testing + parallel multi-journey runs. |
| **EasySpider**  | MIT (visual builder) | Chrome-extension / desktop GUI for building scrape flows without code. | ⚠️ **Flow export only** — EasySpider exports to a JSON flow format. We can consume that format in our journey schema so users who prefer a GUI can author journeys visually, then run them through our crawler. No runtime integration (EasySpider is a browser extension, not a library). |

## Architecture (v0.3)

```
                        ┌────────────────┐
                        │    Runner      │  ← single CLI entry point
                        │  (src/main.ts) │
                        └───────┬────────┘
                                │
                ┌───────────────┼────────────────┐
                │               │                │
                ▼               ▼                ▼
         ┌──────────┐    ┌──────────┐    ┌──────────────┐
         │ Driver   │    │ Driver   │    │ Crawlee      │
         │ Playwr.  │    │ Puppet.  │    │ Orchestrator │
         └────┬─────┘    └────┬─────┘    └──────┬───────┘
              │               │                 │
              └───────┬───────┴─────────┬───────┘
                      ▼                 ▼
               ┌──────────┐       ┌──────────────┐
               │  Page    │◄──────┤ concurrent   │
               │ session  │       │ worker pool  │
               └─────┬────┘       └──────────────┘
                     │
                     ▼
              ┌─────────────┐
              │ Event bus   │ ← console + page-error + request-failed
              │ + CDP hooks │ ← WebSocket frames, network, performance
              └──────┬──────┘
                     │
                     ▼
              ┌─────────────┐
              │  Report     │ ← events keyed by stepIndex (NEW in v0.2.1)
              │   + diff    │   + cross-run diff, budget gate
              └─────────────┘
```

## What each tool adds

### Playwright (kept)
- Primary driver. Already integrated.
- Chromium + Firefox + WebKit.
- Built-in trace viewer for post-mortem.

### Puppeteer (new, v0.3)
- Alt driver for stress scenarios. Easier to spin up 20+ concurrent browser instances (Playwright contexts are heavier).
- `puppeteer-extra` stealth plugin for sites with bot detection.
- Use case: run the same journey in parallel 20× to shake out race conditions the single-instance Playwright smoke misses.

### Crawlee (new, v0.3)
- **Queue-driven orchestration** — register journeys once, Crawlee schedules them across N workers.
- **Auto-concurrency** — scales workers based on system load.
- **Request retry + backoff** — free retry logic on transient failures.
- **Storage adapters** — built-in JSON + SQLite stores that replace our hand-rolled `runs/<ts>/` layout if we want.
- **Proxy rotation** — for cross-geography testing later.
- **Session pool** — preserve cookies across a multi-step journey.
- Use case: "run plausiden-smoke + plausiden-deep + plausiden-admin-actions + plausiden-teach-loop in parallel against 3 deploy targets. Aggregate all findings into one diff-against-last-green report."

### EasySpider (import-only, v0.4)
- Visual flow builder. Non-engineer can build a journey by clicking through the UI.
- Exports a JSON flow (`.es.json`).
- Our converter `src/importers/easyspider.ts` reads the ES flow and emits our `Journey` schema.
- Use case: product managers / QA without JS fluency can author journeys.

## Stress-testing plan

Current: 1 browser, 1 journey, sequential steps. Finds obvious bugs but not races.

Stress layer (v0.3):
- **Concurrent journeys** — run 10 copies of `plausiden-smoke` in parallel. Catches "when I switch tabs quickly" class of bugs that surfaces under load.
- **Network chaos** — use Crawlee's network interception to fail 10% of requests randomly. Stress-tests error-handling paths.
- **Slow-3G emulation** — Playwright's network throttling. Reproduces "extremely long load" and "Suspense never resolves" bugs.
- **Randomized input** — fuzz the chat input with random UTF-8 + emoji + very long text. See what breaks the tokenizer / WS frame limit / localStorage quota.
- **Reload churn** — navigate, reload, navigate, reload — 50×. Catches stale-SW / chunk-hash-drift issues.

## Event correlation (v0.2.1, shipped just now)

Each CapturedEvent has a `t` (ms since run start). Each Step has `durationMs`. v0.2.1 adds `Report.eventsByStep`:

```json
{
  "stepIndex": 12,
  "stepLabel": "jump-classroom",
  "stepKind": "press",
  "windowMs": [3200, 4100],
  "events": [
    { "t": 3210, "kind": "console", "level": "debug", "text": "// SCC: Chat msg: chat_progress" },
    { "t": 3450, "kind": "request-failed", "url": "http://10.99.0.3:3000/api/admin/dashboard", "text": "net::ERR_CONNECTION_REFUSED" },
    { "t": 3600, "kind": "pageerror", "text": "[after step: jump-classroom] [stuck-loading] Fallback copy still visible: Loading classroom" }
  ]
}
```

So "during step 12 (jump-classroom) the Classroom fetch failed and the Suspense fallback never resolved" is now a single report entry, no cross-referencing required.

## Rollout

- **v0.2.1 (shipped)** — event correlation + deep-journey JSON + UI health heuristics (error copy / stuck loading / blank main / visible error boundary).
- **v0.3 (next cycle)** — Crawlee orchestrator + concurrent journeys + network chaos + Puppeteer alt driver.
- **v0.4** — EasySpider flow importer.
- **v0.5** — Lighthouse CI adapter (still in the adapter roadmap from PLAN.md).
- **v0.6** — Mobile (Maestro).
