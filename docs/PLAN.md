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
