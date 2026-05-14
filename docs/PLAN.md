# PlausiDen-Crawler — roadmap + FOSS survey

## v0.1 (shipped 2026-04-19)

Scaffold only: Playwright-based runner, smoke journey (click first 10 buttons), report.json + screenshots, budget gate.

## FOSS survey — per AVP-2 absorption protocol

User ask: "look for FOSS projects that do this." Four strong candidates in the "crawl a web UI and report runtime issues" space. All MIT/Apache/BSD-compatible.

| Tool            | License | What it does                                                                 | Good for us?                                                                 |
|-----------------|---------|------------------------------------------------------------------------------|------------------------------------------------------------------------------|
| **Playwright**  | Apache 2.0 | Headless browser automation (Chromium/Firefox/WebKit). Captures console, page errors, network. What v0.1 uses. | ✅ Already absorbed. Foundation layer. |
| **Lighthouse CI** | Apache 2.0 (Google) | Full audit: performance, accessibility, best-practices, SEO, PWA. CI-native. Outputs JSON + budget assertions. | ✅ Add as second runner — complementary to Playwright. Catches CSP, unsized images, render-blocking JS, a11y rules. |
| **Sitespeed.io** | MIT | Heavy crawler. HAR, filmstrip, WebPageTest integration, Docker-native. Captures everything. | ⚠️ Overkill for single-app CI. Good fit if we expand to multi-site monitoring. |
| **Pa11y CI**    | MIT (UK Gov) | WCAG/axe-core accessibility + console capture, configurable rules, CI-first. | ✅ Narrow but complementary. Add for accessibility regressions. |
| **Webhint**     | Apache 2.0 (Microsoft→OpenJS) | Best-practice auditor with a hint plugin system. Catches meta-tag issues, CSP misconfig, security headers. | ✅ Third complementary runner. One-shot CLI. |
| **Cypress**     | MIT | E2E test runner with real-browser interaction. More opinionated than Playwright. | ❌ Redundant with Playwright. Not absorbing. |
| **Puppeteer**   | Apache 2.0 (Google) | Chromium-only automation. Playwright's predecessor. | ❌ Playwright is the superset. |
| **Lighthouse (CLI)** | Apache 2.0 | Single-run audit. | 🟡 Use via Lighthouse CI instead. |

## v0.2 plan — absorb three, keep them thin

Following AVP-2 FOSS absorption protocol (discover → evaluate → absorb → integrate → loop):

1. **Keep Playwright** as the core browser layer. Mature, minimal deps, large contributor base.
2. **Absorb Lighthouse CI** (`@lhci/cli`) as a secondary runner triggered from the same CLI. Writes its own `lhci.json` into `runs/<ts>/`. Thresholds live in `lhci.yaml`.
3. **Absorb Pa11y CI** for accessibility-only runs. Output merged into the same `report.json`.
4. **Absorb Webhint** for header + CSP + SEO meta-tag issues.

Integration pattern: thin adapter in `src/adapters/{lhci,pa11y,webhint}.ts`. Each adapter reads the upstream output and normalises into our `CapturedEvent[]` shape. Consumers never see the upstream JSON — the adapter is the only thing they touch. Per AVP-2 dependency-inversion principle so we can swap adapters without touching the runner.

## v0.3 — journey config

Replace the hardcoded "click first 10 buttons" smoke with YAML-configured journeys:

```yaml
# journeys/plausiden-ai.yaml
- step: landing
  url: /
  asserts:
    - console-errors: 0
    - failed-fetches: 0
- step: open-admin
  click: '[aria-label="Open admin"]'
  wait: 'text=Dashboard'
- step: open-classroom
  click: '[data-tour="cmdk"]'
  type: 'Go to Classroom'
  press: 'Enter'
- step: screenshot-all-admin-tabs
  for: '[role=tab]'
  do:
    - click: '$.target'
    - screenshot: 'admin-tab-$.index.png'
```

Each step adds its events/screenshots to the run report.

## v0.4 — diff against prior run

Store runs under `runs/<deploy-tag>/`. CLI `--compare-to <tag>` diffs two reports and flags NEW console errors or NEW failed fetches. Most useful CI signal: "this deploy introduced 3 errors that weren't there yesterday."

## v0.5 — mobile emulation

Playwright's `deviceDescriptors` → run every journey in 3 viewports (desktop 1280px, tablet 768px, mobile 375px). Report tags each event with the viewport so mobile-only regressions surface without a separate pipeline.

## v0.6 — CSP violation listener

`page.on('console', msg => ...)` catches most CSP violations (Chromium logs them). But the authoritative signal is the `report-uri` endpoint. Wire a tiny local HTTP server that receives CSP reports during the run, rolls them into the report.json. Matches the same pattern the main app already uses at `/api/csp-report`.

## v0.7+ — nice to have

- Per-route screenshot diff with `pixelmatch` or `odiff`.
- Network throttling presets (slow 3G, fast 3G, offline) to reproduce "extremely long load" bugs.
- `--record` flag to save an Playwright trace.zip for interactive post-mortem.
- Wrap as a GitHub Action (`plausiden/crawler-action@v1`) so other repos can run it with one line.

## v1.0 — Pluggable runner backends (T75)

**Owner directives 2026-05-14:**

> "how difficult would it be to rewrite plausiden crawler in rust?"
>
> "lets have a current 'working' version also. so one we can
> continue to test and use and then work on the new crawler stuff."
>
> "and when we get all the options available the user can choose
> the current way, method A, B, or C"

The current TS Playwright runner stays working + supported. New
Rust runners land alongside, each behind a backend flag. User
chooses per-run via `--backend`. Same `Journey` JSON in, same
`Report` JSON out — backends are interchangeable.

### The four backends

| Backend | Flag | Stack | Status | When to use |
|---|---|---|---|---|
| **Current (TS)** | `--backend=playwright` (default until v1.0) | TS / Node / Playwright | shipped | Today's working pipeline. Stays canonical until a Rust backend reaches parity on all detectors. |
| **Method A (CDP)** | `--backend=cdp` | Rust / `chromiumoxide` / Chromium | queued (T75A) | Recommended Rust path. Closes Oxidizer's `check_rust_only` SHIP-DECISION. 2-4 weeks of focused work. |
| **Method B (WebDriver)** | `--backend=webdriver` | Rust / `fantoccini` or `thirtyfour` / any WebDriver browser | queued (T75B) | Cross-browser native (Chrome + Firefox + Safari). 2-3 weeks. WebDriver less granular than CDP for network interception. |
| **Method C (Pure Rust)** | `--backend=servo` or `--backend=ladybird` | Rust all the way down | concept (T75C) | True supersociety endgame. 6-12 months — neither engine production-ready as headless yet. Sprint 5+ horizon. |

### Architecture: pluggable `RunnerBackend` trait

Extract the runtime orchestration interface into a typed Rust
trait + a TS-side equivalent. Every backend implements:

```rust
trait RunnerBackend {
    fn name(&self) -> &'static str;
    fn launch(&self, opts: &LaunchOpts) -> Result<Session>;
    fn navigate(&self, session: &mut Session, url: &str) -> Result<()>;
    fn click(&self, session: &mut Session, selector: &str) -> Result<()>;
    fn fill(&self, session: &mut Session, selector: &str, value: &str) -> Result<()>;
    fn screenshot(&self, session: &mut Session, path: &Path) -> Result<()>;
    fn capture_console(&self, session: &mut Session) -> Result<Vec<ConsoleEvent>>;
    fn capture_network(&self, session: &mut Session) -> Result<Vec<NetworkEvent>>;
    fn capture_csp_violations(&self, session: &mut Session) -> Result<Vec<CspEvent>>;
    fn capture_web_vitals(&self, session: &mut Session) -> Result<WebVitals>;
    fn evaluate_js(&self, session: &mut Session, expr: &str) -> Result<serde_json::Value>;
    fn close(&self, session: Session) -> Result<()>;
}
```

The 4 typed Rust crates (`crawler-runner` / `crawler-journey` /
`crawler-detectors` / `crawler-report`) consume this trait
agnostic of backend. Detector code never knows which backend
captured the events.

### Why pluggable instead of "pick one and migrate"

- **No big-bang.** TS keeps working through every Rust backend
  rollout; rollback is `--backend=playwright`.
- **Detector parity becomes verifiable.** Run the same journey
  through two backends, diff the findings — any divergence is a
  backend bug, surfaces immediately.
- **Audience choice.** Mom-class operator runs `--backend=cdp`
  for fastest local audit; a cross-browser CI matrix runs
  `--backend=webdriver`; the supersociety enthusiast runs
  `--backend=servo` once it's ready.
- **T73 (CrawlerCore split) becomes natural.** Each backend
  lives in its own crate; CrawlerCore exposes the trait;
  PlausiDen-Crawler + PlausiDen-Recon both consume the same
  backend set.

### Sprint plan (parallel-track development)

#### Track 0 (continuous): keep TS working

- All current detectors keep running on Playwright.
- Bug fixes + new detectors land in TS while Rust catches up.
- TS stays the canonical reference for parity verification.

#### Track A (T75A — chromiumoxide, 2-4 weeks)

- **Week 1.** `RunnerBackend` trait extracted + a `CdpBackend`
  that implements it via `chromiumoxide`. Click / fill / wait /
  screenshot / nav. Verify on the SkillShots smoke journey.
- **Week 2.** Network + console + CSP + web-vitals capture via
  CDP events. Port runtime-* detectors to consume the new event
  stream.
- **Week 3.** Port `aria.ts` + `audit.ts` + `journey.ts`
  orchestration logic. Embed axe-core via injected JS for a11y
  rules.
- **Week 4.** Port `discover.ts` + `probe.ts` (OSINT mode for
  the PlausiDen-Recon fork landing in T73). Mutation tests +
  parity verification: same journey through `--backend=playwright`
  and `--backend=cdp` produces same findings.

#### Track B (T75B — fantoccini, 2-3 weeks, after Track A)

- **Week 1.** `WebDriverBackend` implementing the trait via
  `fantoccini` (or `thirtyfour`).
- **Week 2.** Cross-browser matrix (Chromium + Firefox + Safari
  Tech Preview) with per-browser parity verification.
- **Week 3.** Network interception via WebDriver-BiDi where
  available; fall back to in-page JS shim for browsers without
  it.

#### Track C (T75C — pure-Rust browser, Sprint 5+)

Wait for Servo or Ladybird to mature. Until then this stays
concept. When ready: `ServoBackend` / `LadybirdBackend`
implementing the trait. Eliminates Chromium dep entirely.

### Acceptance for v1.0

1. `--backend=playwright` is still the default; existing CI
   keeps passing without change.
2. `--backend=cdp` reaches parity on every shipped detector;
   diff against `--backend=playwright` is zero on every
   journey in `journeys/`.
3. Mutation-test survival rate < 5% across the new Rust
   backend.
4. Property-based fuzz on `CdpBackend` (no panic on arbitrary
   CDP message).
5. T73 (CrawlerCore split) lands cleanly: `RunnerBackend`
   trait lives in CrawlerCore; both PlausiDen-Crawler and
   PlausiDen-Recon consume the same backend set.
6. The `--backend` flag is documented in `--help` and tested
   in CI across all four (or all that are ready).

### What we keep from TS forever (acceptable cost)

- Playwright's `npx playwright codegen` ergonomic test-recording
  UI stays useful even after Rust backends ship — the
  recorded TS journey gets translated into the Journey JSON
  schema and runs on any backend.
- Rapid prototyping of new detectors stays easier in TS
  initially, then ports to Rust once stable.

### Why this matters

The chooser-flag design lets us be honest about tradeoffs.
Today TS is mature; CDP is fast; WebDriver is portable; pure
Rust is incomplete. As each matures the default backend may
shift. The pluggability means we never have to commit to one
forever.
