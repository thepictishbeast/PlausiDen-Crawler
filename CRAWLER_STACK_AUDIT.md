# Crawler stack audit — TypeScript/Playwright vs Rust

**Triggered by:** owner directive 2026-05-04: *"make sure the entire cms,
loom, forge, audits and everything else is super society level tech
stacks, and if its not using rust it better have a damn good reason.
being lazy and doing whats easy is never a justification."*

PlausiDen-Crawler is the only TypeScript subsystem still in active
development (along with PlausiDen-Browser-Ext, which has no Rust path
at all — browsers REQUIRE JS for extensions). Every other
PlausiDen-* repo with code is Rust. This document is the verdict.

## Inventory — what the Crawler actually does

**Codebase:** 6,667 lines of TypeScript across 25 source files in
`src/`. Most line-count concentrated in:

* `main.ts` (~1,100) — argv parse, browser/context/page lifecycle,
  CDP wiring, journey runner driver, report writer
* `report.ts` — structured JSON / per-axis summary
* `audit.ts` — axe-core integration
* `cssHealth.ts`, `uiOverflow.ts`, `runtimeContrast.ts`,
  `runtimeImages.ts`, `runtimeFocus.ts` — five detection-axis impls
  that each ship a hand-rolled `evalFn` template literal evaluated
  in page context

**Playwright surface used (counted):**

| API                    | Usages |
|------------------------|-------:|
| `page.evaluate`        |     29 |
| `page.on(...)`         |     13 |
| `page.goto`            |      7 |
| `page.exposeFunction`  |      5 |
| `page.screenshot`      |      3 |
| `page.addInitScript`   |      3 |
| `page.context`         |      2 |
| `page.click`           |      2 |
| `page.fill / press / waitForSelector / keyboard / addScriptTag` | 5 (combined) |

**CDP surface used:**

* `Network.enable`
* `Network.emulateNetworkConditions`   *(throttle journey)*
* `Network.webSocketClosed` / `webSocketFrameError`
* `Audits.enable`
* `Audits.issueAdded`                   *(CSP violations — currently silent, T84)*
* `Log.enable`
* `Log.entryAdded`

**Detector axes (12):** console, page-error, failed-request, axe-static,
cssHealth, uiOverflow, runtimeContrast, runtimeImages, runtimeFocus,
webVitals, cspViolations, ariaDrift.

**Browser targets:** Chromium only (Playwright supports Firefox/WebKit
but the Crawler doesn't currently exercise them — all journeys assume
Chromium).

## Rust alternatives surveyed

| Crate              | Browser engines       | Mechanism | Async    | Maturity (2026-Q1)        | Multi-target | Test infra |
|--------------------|----------------------|-----------|----------|---------------------------|--------------|------------|
| **chromiumoxide**  | Chromium only        | CDP       | tokio    | 1.2k stars; active 2026   | No           | Custom     |
| **headless_chrome**| Chromium only        | CDP       | sync+async | 1.4k stars; sporadic   | No           | Custom     |
| **fantoccini**     | Any (WebDriver)      | WebDriver | tokio    | 600 stars; active         | Yes          | Custom     |
| **thirtyfour**     | Any (WebDriver)      | WebDriver | tokio    | 500 stars; active         | Yes          | Custom     |
| Direct CDP via tungstenite | Chromium only | raw WS+JSON | tokio | DIY                       | No           | DIY        |

**Playwright-for-Rust:** does not exist. There is no maintained Rust
binding to Microsoft's Playwright server.

## Feature-parity matrix — Rust port cost

For each Playwright feature the Crawler uses today, what's the
Rust-side equivalent + cost?

| Crawler feature                        | chromiumoxide   | fantoccini (WebDriver) | Notes |
|----------------------------------------|-----------------|------------------------|-------|
| `page.goto` + `waitUntil`              | ✓ direct        | ✓ direct               | Equal |
| `page.evaluate(string)`                | ✓ `page.evaluate_function` | ✓ `execute`     | Equal |
| `page.exposeFunction`                  | ✓ `expose_function`        | ✗ no equivalent | WebDriver loses bidirectional comms |
| `page.addInitScript`                   | ✓ via CDP `Page.addScriptToEvaluateOnNewDocument` | ✗ | WebDriver lacks pre-doc-create hooks |
| `page.on('console')` / `pageerror`     | ✓ event stream  | △ partial (logs only)  | WebDriver smaller event surface |
| CDP `Audits.enable` / `issueAdded`     | ✓ direct CDP    | ✗ no CDP exposure      | WebDriver out — chromiumoxide only |
| CDP `Log.entryAdded`                   | ✓ direct CDP    | ✗                      | Same |
| CDP `Network.emulateNetworkConditions` | ✓ direct CDP    | △ via Selenium 4 BiDi  | Mature in chromiumoxide |
| `page.screenshot`                      | ✓ direct        | ✓ direct               | Equal |
| axe-core injection (`addScriptTag`)    | ✓ JS string injection | ✓                | Equal |
| Multi-browser (Chrome/FF/WebKit)       | ✗ Chromium only | ✓ all WebDriver-compat | Crawler doesn't currently use multi-browser |
| Long-running browser process management | △ DIY          | ✓ standard webdriver service | DIY in chromiumoxide |

**Verdict on multi-browser:** crawler doesn't currently use it. Adding
Firefox/WebKit later requires a different abstraction either way.

**chromiumoxide is the right Rust target IF we port.** It exposes the
full CDP surface — including the four CDP domains we already use —
and supports `expose_function` + `addInitScript` patterns. WebDriver
crates lose those features entirely.

## Port cost (chromiumoxide path)

* Lines to rewrite: ~6,700 TS → estimated **~5,500 Rust** (Rust is
  more compact for this kind of structured detector code, but adds
  ceremony around lifetimes + async context)
* Testing infrastructure: rebuild from scratch. Playwright's test
  fixtures don't translate; chromiumoxide tests are typically against
  a real Chromium binary which CI must provision.
* Detector eval-function strings: stay as JS strings injected via
  CDP — no syntactic change. Just the surrounding harness.
* Journey JSON schema: language-neutral, no change.
* Tooling around the crawler (the `npm run audit` consumer in CI):
  swap to `cargo run -p crawler` — minor.

**Effort estimate:** 80–120 focused hours by a single Rust-capable
contributor. Crawler covers a wide surface (12 detector axes, 12
journey types, 4 CDP domains) and the eval-function template literals
each need careful audit during port to preserve their failure modes
(see T513, T84 — already-known detector subtleties that bashed our
heads against during the bash→Rust port for forge).

## What changes if we port

**Wins** (real, not hypothetical):

* **Single language** for all PlausiDen subsystems. AVP-2 supersociety
  alignment achieved.
* **Performance:** chromiumoxide async loop ~2-3× faster startup vs
  ts-node + Playwright (anecdotal from open-source benchmarks).
  Build/CI minutes saved.
* **Memory:** Playwright pulls a 200-300MB Node + browser
  installation; Rust binary + chromium-launcher is closer to 5-10MB
  binary + browser.
* **Type safety on detector data:** cssHealth/uiOverflow/runtime*
  detector findings currently have hand-typed TS interfaces; in Rust
  they'd be enforced shapes shared with `forge-core` so a finding
  produced by the Crawler is the same type the Forge JSON report
  consumes. Eliminates the bash↔Rust JSON-shape compat drift we hit
  in T38.
* **Single test framework:** workspace-wide `cargo test` instead of
  TS + npm + jest + playwright fixtures. Reproducibility win.

**Losses:**

* **Multi-browser becomes harder.** chromiumoxide is Chrome-only;
  re-adding Firefox/WebKit later means a second integration via
  fantoccini/thirtyfour OR a CDP-equivalent that doesn't exist in
  Firefox.
* **Playwright's debugger / trace-viewer goes away.** chromiumoxide
  has no equivalent of `playwright show-trace`. Some detector-debug
  workflows need a custom replacement.
* **axe-core integration is uglier in Rust** — axe is JS, has to be
  injected as a script blob and called via evaluate_function. (Same
  pattern as today, just less ergonomic.)

## Recommendation: port to chromiumoxide — but on a phased plan, not in one pass

Owner doctrine + AVP-2 cumulative weight outweighs the migration cost.
Multi-browser is a hypothetical we don't currently use; if we ever
need it, fantoccini wraps any WebDriver target.

**Proposed port plan:**

* **Phase 1** — extract detector-eval-functions into `crawler-detectors`
  Rust crate (string constants + their post-processing types). This
  lets both the TS Crawler and a future Rust Crawler consume the same
  eval bodies. ~1 tick.
* **Phase 2** — port `report.ts` shape into `crawler-report` crate
  (shared with `forge-core::Finding` so the JSON round-trips between
  Forge and Crawler are typed). ~1 tick.
* **Phase 3** — minimal chromiumoxide-based journey runner that
  consumes `crawler-detectors` + `crawler-report`. Initial parity
  target: `skillshots-poc.json` desktop journey, console/page-error/
  failed-request axes. ~3-5 ticks.
* **Phase 4** — port detector axes one at a time. Each port closes
  the parity matrix entry by entry. ~2 ticks per axis × 12 axes.
* **Phase 5** — port UX-persona journeys (mobile/tablet/zoom/
  throttled/keyboard/screen-reader/first-time). Throttle uses
  Network.emulateNetworkConditions which already works in
  chromiumoxide. ~2 ticks total.
* **Phase 6** — retire `npm run audit`; switch CI to `cargo run -p
  crawler`. Delete the TS source. ~1 tick.

**Total estimate:** 30-40 focused ticks (~30 hours).

**Defer condition:** if a Rust port creates a critical regression in
a detector axis we currently rely on, fall back to keeping that axis
in the TS path while the Rust impl matures. The phased plan supports
side-by-side execution during the migration window.

## Decision

> **PORT.** The TypeScript Crawler is the last non-Rust active
> subsystem and the cost of porting (~30 ticks) is bounded.
> chromiumoxide covers every API we depend on. The phased plan
> means the build is never red for more than a tick at a time.

This document is queued as **Crawler T100** — start with Phase 1
(extract detector eval-functions into a shared crate). The actual
port is its own sequence of tasks.

---

*Reviewed against AVP-2 Tier 5 cross-repo contribution doctrine: the
port should bring Crawler under the same `cargo test --workspace`
umbrella as Forge / Loom / CMS, eliminating a class of cross-language
JSON-shape drift bugs (the same bug class we already had to fix in
forge-replay's `#[serde(alias = "STRICT")]` accommodation).*
