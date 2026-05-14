# PlausiDen-Crawler — vision document

> "If PlausiDen-Crawler was already built and did everything we
> wanted, what would this doc say?"

This is that doc. **[shipped]** works today. **[in-flight]** is
mid-build. **[queued]** has a task ID. **[concept]** has been
implied or requested and a developer should design it.

---

## 1. What PlausiDen-Crawler IS

**Headless-browser substrate** with two distinct first-class
surfaces:

1. **Site auditing** (the dominant surface today) — drive a known
   URL through a scripted journey, verify intended behaviour,
   capture every runtime signal a real visitor would experience.
2. **Reconnaissance / OSINT** (the under-leveraged second surface)
   — drive an unknown URL via discovery + probe modules
   (`src/discover.ts`, `src/probe.ts`), enumerate reachable
   surfaces (links, forms, endpoints, sub-resources), build a
   typed surface map suitable for security review or competitive
   intelligence.

Both surfaces share the same headless-browser core (Playwright
runtime + Rust crates for typed journeys / detectors / report).
Owner intent (clarified 2026-05-14): keep both, but consider
factoring the core out so a third repo (or fork) can specialise
on OSINT without polluting the audit-side detector set, and vice
versa. See section 5 for the proposed split.

Site-audit captures, today:

- Every `console.log`/`warn`/`error` message.
- Every `window.onerror` and `unhandledrejection`.
- Every failed network request (non-2xx, aborted, refused).
- Every render-blocking or console-logged CSP violation.
- Every web-vitals measurement (LCP, CLS, INP, TTFB, FCP).
- Screenshots at each step.
- (Queued) Full DOM snapshots, accessibility tree, memory
  usage, event-listener counts.

Output is a single JSON bundle per run (`runs/<host>-<ts>/
report.json`) plus per-step PNGs — diff-ready across deploys,
paste-ready into a bug tracker, ingestible by Forge as a
runtime-audit phase finding.

Operationally: TypeScript / Node + Playwright + Puppeteer +
Crawlee, with Rust modules for the typed report + journey
detector + report-renderer crates.

| Crate / package | Role |
|---|---|
| `crates/crawler-runner`     | Playwright-driven runtime that executes journey scripts and captures telemetry |
| `crates/crawler-detectors`  | Typed detectors per finding class (placeholder text, broken link, CSP violation, console error, slow LCP, etc.) |
| `crates/crawler-journey`    | Typed `Journey` + `Step` model |
| `crates/crawler-report`     | Typed `Report` + `Finding` schema (consumable by Forge) |
| (TS) `src/`                 | Playwright orchestration glue + CLI entrypoint |

PlausiDen-Crawler is **not**:

- A vulnerability scanner (Burp / Vulnerability-Scanner-* crates
  cover that — Crawler tests INTENDED behaviour OR enumerates
  reachable surface, never crafted exploit payloads)
- A search-engine indexer
- A SEO crawler (any incidental SEO findings are by-product)
- Bound to your own infrastructure (audit + recon both work
  against any externally-served URL)
- Internet-scale (yet — recon mode operates per-target;
  internet-scale federation is concept territory)

## The meta-mission: making AI-built UI reliable

Every PlausiDen tool — Loom, CMS, Forge, Crawler, Annotator —
exists for one common reason: **AI agents building GUI / frontend /
UX work need a reliability substrate that humans don't.** A
human dev opens DevTools, sees a console error, fixes it. An
AI agent doesn't open DevTools — so without an external runtime
audit, regressions ship silently.

Crawler is the runtime feedback loop that closes that gap. After
every agent-driven edit, Crawler drives the journey, captures
every console error / CSP violation / network failure / web-vital
deviation, and emits typed findings the agent can branch on. The
agent now has an oracle: "did my change break the rendered site?"
— answered in seconds, in JSON, severity-laddered.

OSINT mode serves the same mission for an adjacent need: when
an agent has to integrate with or reference an EXISTING site
(competitor research, partner integration, sales-prospect
dossier), Crawler enumerates the surface so the agent doesn't
have to scrape it manually.

PlausiDen-Crawler's contract: feed it a `Journey` JSON describing
one user flow + a target URL + a finding-budget, get back a
typed `Report` listing every observed deviation from clean
behaviour. Exit non-zero if the finding-count exceeds the
budget.

## 2. The supersociety stack PlausiDen-Crawler uses

- **Typed JSON contracts** — `Journey` and `Report` are
  serde-typed Rust structs. The TS runner emits / consumes them
  via a thin schema; Rust detectors operate on them
  type-safely.
- **Strict severity ladder** — each `Finding` carries a severity
  (`info` / `warn` / `strict`). Strict findings block deploys
  via Forge integration.
- **Reproducible runs** — same journey + same target → same
  set of findings (modulo timing-dependent flake, which the
  crawler explicitly tags as such).
- **Budget gates** — every journey declares a per-class finding
  budget; exceeding the budget exits non-zero. Used for CI / CD
  go/no-go gating.
- **Headless-only** — Playwright in headless Chromium today;
  Firefox + Safari Tech Preview cross-checks queued.
- **Privacy-respecting** — no analytics ingestion, no
  fingerprinting, no third-party requests. The crawler is the
  privacy floor; if Crawler can fingerprint a site, the site
  can be fingerprinted.
- **Property-based testing** — proptest on every detector to
  guarantee no panic on arbitrary HTML / DOM input.
- **Crawler Registry** — every crawler / scraper / Playwright
  alternative we've evaluated is recorded with a verdict
  (`adopted` / `adopted-as-dep` / `deferred` / `reference-only`
  / `rejected`). Same discipline as PlausiDen-Audits TOOL_REGISTRY.
- **No `unwrap`/`expect` in lib code** (lint enforced on Rust
  side; lint guidance documented for TS side).

## 3. Personas

### 3.1 Mom — non-technical client

Mom never opens Crawler directly. What she gets:

- After every deploy, **Forge runs Crawler against her site**
  using a journey she didn't have to write. Forge declares
  "deploy verified" or "deploy has 3 issues — review before
  shipping" in plain English. Mom decides.
- If a Crawler finding is severity `strict`, the deploy is held
  and the editor surfaces "Your most recent change broke X.
  Click here to see what happened" with the relevant screenshot
  + console excerpt.
- (Queued) **Auto-recorded journeys** — when Mom uses the editor
  for the first time, Crawler watches her clicks + form-fills
  and writes a `Journey` JSON describing her own behaviour. That
  becomes her own deploy verification suite, no scripting required.

### 3.2 The technical client

What they get today:

- **Journey scripts in TS** that drive any flow they care about
  (login, checkout, newsletter signup, contact form).
- **Per-class finding budgets** in the journey JSON — they can
  say "ignore exactly 2 console-warn lines for the next 30 days
  while we work down the legacy code, but block any new ones."
- **Diff-able JSON reports** they can store in their own Git for
  trend analysis.
- **CSP / console / network / web-vitals** captured by default,
  no extra config.

What they get next:

- **Visual diff** across deploys — pixel-hashed screenshots per
  step, surfaces any unintended visual change [queued].
- **Cross-browser matrix** — same journey runs in Chromium +
  Firefox + Safari Tech Preview, findings merged [concept].
- **Cross-device matrix** — mobile / tablet / desktop viewports,
  findings tagged per breakpoint [concept].
- **Localization audit** — verify every locale loads cleanly
  with no missing-string warnings [concept].
- **Performance regression budgets** per page (LCP, CLS, INP,
  TTFB) with baseline auto-computed from rolling window [concept].

### 3.3 The developer — contributor or forker

What they get today:

- **Typed `Detector` trait** in `crates/crawler-detectors` —
  implement `name()` + `inspect(&Telemetry) -> Vec<Finding>` and
  you have a new detector. Register it.
- **Typed `Journey` model** — TS emits, Rust consumes. Schema
  is stable + versioned.
- **17+ detectors shipped** as worked examples (placeholder text
  detection, link-text quality, console-error class, CSP
  violation, network-failure, slow-LCP, axe-rule violation, …).
- **Crawler Registry** — every alternative tool considered is
  recorded with a verdict so the same projects don't get
  re-evaluated.
- **Property-based fuzz** on every detector so panics never
  reach production.

What developers want next:

- **Pluggable browser backend** — Playwright today, Selenium /
  WebDriver-BiDi / pure-Rust browser (Servo / Ladybird wrapper)
  swappable [concept].
- **Headless OS-level browser** for non-Chromium fidelity
  [concept].
- **`crawler-watch`** — file-watcher mode that re-runs the
  current journey on every save (paired with Loom's
  edit-serve) [concept].
- **`crawler-replay`** — replay a captured network log into a
  recorded journey for offline debugging [concept].
- **Memory-leak / event-listener-leak detection** across long
  journeys [concept].

### 3.4 Claude Code (and other autonomous agents)

What an agent gets today:

- **JSON in, JSON out.** `Journey` schema is the input contract;
  `Report` schema is the output contract. No screen-scraping
  required.
- **Exit code conveys gate status** — non-zero on
  budget-exceeded, suitable for `set -e` orchestration.
- **Stable severity ladder** — agents can branch on
  `Severity::Strict` / `Warn` / `Info` without parsing prose.

What agents want next:

- **MCP server** exposing Crawler capabilities (`run_journey`,
  `get_report`, `replay_run`, `auto_record_journey`) as
  discoverable tools [concept].
- **`crawler auto-discover`** — given a URL, an agent asks
  Crawler to explore the site (BFS through links + form
  submission) and propose a default journey covering the
  meaningful flows [concept].
- **`crawler shrink-finding`** — given a strict finding, an
  agent asks Crawler to bisect the journey to isolate the
  smallest reproducer [concept].
- **Streaming finding-firehose** so an agent can act on
  findings as they emit, not after the run completes [concept].
- **Session recording with time-travel debugging** — an agent
  can step backward through a captured run, inspect DOM /
  network at any point [concept].

## 4. Capability map

### 4.1 Telemetry capture

| Capability | Status |
|---|---|
| `console.log/warn/error` capture | shipped |
| `window.onerror` + `unhandledrejection` capture | shipped |
| Network request capture (URL, status, MIME, size) | shipped |
| CSP violation capture | shipped |
| Per-step screenshots (PNG) | shipped |
| Web-vitals (LCP / CLS / INP / TTFB / FCP) | shipped |
| Full DOM snapshot per step | queued |
| Accessibility-tree snapshot per step | queued |
| Memory usage per step | queued |
| Event-listener count per step | queued |
| Service-worker / cache-storage state | queued |
| Cookie / localStorage / sessionStorage diff per step | queued |

### 4.2 Detectors (per-class finding emitters)

| Detector | Status |
|---|---|
| `placeholder_text` (T16 — "Lorem ipsum" left over) | shipped |
| `link_text_quality` (T106 — "click here" / "read more") | shipped |
| `console_error_class` | shipped |
| `console_warn_class` | shipped |
| `csp_violation` | shipped |
| `network_failure` (non-2xx / aborted / refused) | shipped |
| `slow_lcp` (web-vitals threshold) | shipped |
| `slow_inp` (web-vitals threshold) | shipped |
| `axe_a11y_violation` (axe-core integrated) | shipped |
| `broken_image` | shipped |
| `mixed_content` | shipped |
| `keyboard_trap` | queued |
| `screen_reader_landmark_missing` | queued |
| `form_submission_succeeded` (positive case) | queued |
| `flaky_step` (timing-dependent flake tag) | queued |
| `memory_leak` (heap-growth across steps) | concept |
| `event_listener_leak` | concept |
| `service_worker_misbehaviour` | concept |

### 4.3 Journey + budget

| Capability | Status |
|---|---|
| Typed `Journey` JSON schema | shipped |
| Per-step click / fill / navigate / wait actions | shipped |
| Per-class finding budgets per journey | shipped |
| Per-detector enable/disable per journey | shipped |
| Auto-record journey from human clicks | concept |
| Auto-discover journeys via BFS exploration | concept |
| Multi-page journeys with state-machine tracking | partial |
| Concurrent journey runner (N parallel browsers) | concept |
| Crawl-budget per-site | concept |
| Sitemap-driven exhaustive crawl | concept |

### 4.4 Cross-platform / cross-config

| Capability | Status |
|---|---|
| Headless Chromium | shipped |
| Headless Firefox | queued |
| Headless Safari Tech Preview | concept |
| Headless OS-level browser (Servo / Ladybird wrapper) | concept |
| Mobile viewport emulation | partial |
| Tablet viewport emulation | partial |
| Desktop viewport emulation | shipped |
| Network throttling (3G / 4G / offline) | shipped |
| CPU throttling | partial |
| Localization (per-locale URL audit) | concept |

### 4.5 Reporting + diffs

| Capability | Status |
|---|---|
| JSON `Report` per run | shipped |
| Per-step PNG screenshots | shipped |
| Per-step DOM snapshot diff vs baseline | concept |
| Pixel-hash visual diff vs baseline | queued |
| Web-vitals trend across rolling window | concept |
| Finding-budget burn-down per release | concept |
| Severity ladder (`info` / `warn` / `strict`) | shipped |
| Exit code conveys gate status | shipped |
| Markdown report (human-readable) | partial |
| HTML report (human-readable + interactive) | concept |
| Streaming finding firehose (websocket) | concept |
| Replay captured network log into a recorded journey | concept |

### 4.6 Forge + Annotator + CMS integration

| Capability | Status |
|---|---|
| Forge invokes crawler as a phase via `npm run audit -- --journey` | partial |
| Crawler `Finding` schema convertible to Forge `Finding` | partial |
| Annotator session JSON consumable as a journey input | concept |
| CMS publish event triggers Crawler verification automatically | concept |
| Crawler results posted back to CMS audit log | concept |
| Crawler runs inside an isolated container per tenant | concept |

### 4.7 OSINT / reconnaissance (second first-class surface)

| Capability | Status |
|---|---|
| `src/discover.ts` — link/sitemap/robots BFS exploration | shipped (basic) |
| `src/probe.ts` — endpoint reachability + response-shape capture | shipped (basic) |
| Sitemap-driven exhaustive enumeration | partial |
| Form-field enumeration (every input on every reachable page) | queued |
| Asset graph (every script/style/image with dependency edges) | queued |
| Tech-stack fingerprinting (framework / CMS / server detection) | concept |
| Reachable-surface map as typed `SurfaceMap` JSON | concept |
| Subdomain enumeration via DNS / cert-transparency cross-ref | concept |
| Robots.txt + security.txt + humans.txt + ads.txt scrape | concept |
| Open-graph / JSON-LD / microdata structured-data extraction | concept |
| Email / phone / social-handle harvesting (with opt-in only — never against random targets) | concept |
| Sales-dossier ingestion (typed handoff to PlausiDen-Salesman) | concept |
| Competitive-content snapshot (typed page model from any URL) | concept |
| Tech-debt scoring of a target site (a11y + perf + sec snapshot) | concept |
| Partner-integration surface map (every public endpoint + auth shape) | concept |
| Privacy-respecting OSINT (no identifying probes against unconsented targets) | doctrine — concept |

### 4.8 Privacy + opsec

| Capability | Status |
|---|---|
| No analytics ingestion / fingerprinting | shipped (by design) |
| Privacy-mode crawl for measuring site fingerprintability | concept |
| Tor / VPN crawl for measuring geo-blocking + TLS-MITM behaviour | concept |
| Cookie / localStorage / sessionStorage diff per step (privacy audit) | concept |
| Identifying when a site reaches third-party domains | concept |

### 4.8 Documentation

| Capability | Status |
|---|---|
| README with status + run instructions | shipped |
| `docs/PLAN.md` — build-out roadmap | shipped |
| `docs/CRAWLER_VISION.md` (this doc) | shipped (T72) |
| `docs/MULTIPLATFORM.md` — cross-OS deployment notes | shipped |
| `docs/MULTI_FRAMEWORK.md` — Loom / Forge / consumer-app scope | shipped |
| `docs/FOSS_FORK_CANDIDATES.md` | shipped |
| `docs/FIRST_RUN_FINDINGS.md` | shipped |
| `docs/VISIONLESS_AI.md` | shipped |
| `CRAWLER_REGISTRY.md` — alternatives evaluated | shipped |
| `CRAWLER_STACK_AUDIT.md` — current stack rationale | shipped |
| Per-detector documentation (when each fires + how to fix) | partial |
| In-tool tutorial (run a sample journey, see findings) | concept |

## 5. Architecture (when fully built)

### 5.1 Proposed three-repo split (owner direction 2026-05-14)

Owner intent: keep both surfaces, but factor the shared core
out so the audit-side and OSINT-side detector sets evolve
independently without polluting each other.

```
                    ┌────────────────────────┐
                    │ PlausiDen-CrawlerCore  │  ← new, T73
                    │ (shared substrate)     │
                    │                        │
                    │ - crawler-runner       │
                    │   (Playwright + Rust)  │
                    │ - crawler-journey      │
                    │   (typed Journey/Step) │
                    │ - crawler-report       │
                    │   (typed Report/Finding│
                    │ - crawler-detectors-   │
                    │     core (severity     │
                    │     ladder, base trait)│
                    └────────────────────────┘
                              ▲
                ┌─────────────┴──────────────┐
                │                            │
   ┌────────────────────────┐    ┌──────────────────────────┐
   │ PlausiDen-Crawler      │    │ PlausiDen-Recon          │
   │ (this repo, focused on │    │ (new fork, focused on    │
   │  site-audit detectors  │    │  OSINT-style enumeration │
   │  per known journey)    │    │  + surface-mapping)      │
   │                        │    │                          │
   │ - audit detectors      │    │ - discover / probe       │
   │   (placeholder, link,  │    │   (link BFS, sitemap,    │
   │    css health, web     │    │    DNS, cert-transp)     │
   │    vitals, a11y …)     │    │ - tech-stack finger-     │
   │ - per-journey gates    │    │   printing               │
   │ - Forge phase_crawl    │    │ - typed SurfaceMap       │
   │   integration          │    │ - Salesman dossier feed  │
   │                        │    │ - competitor snapshot    │
   └────────────────────────┘    └──────────────────────────┘
                │                            │
                ▼                            ▼
       Forge / CMS / Loom            Salesman / Harvest /
       (deploy verification)         competitor research
```

Today's monolith stays functional during the split; T73 refactor:

1. Lift `crawler-runner` / `crawler-journey` / `crawler-report`
   plus the severity-ladder + base-Detector trait into a new
   `PlausiDen-CrawlerCore` repo (or a workspace under
   PlausiDen-Crawler that the other two depend on).
2. Move the audit-side `src/*.ts` + audit detectors into the
   PlausiDen-Crawler repo proper (no rename).
3. Create PlausiDen-Recon as a sibling consumer of CrawlerCore;
   port the existing `src/discover.ts` + `src/probe.ts` over.
4. Each consumer ships its own `Journey` / `Report` / `Finding`
   schemas extending the core types.
5. CrawlerCore stays minimal — runtime + types + base trait. No
   detectors past the base.

This split mirrors how Loom / CMS / Forge already share types
across repo boundaries via Cargo path/git deps.

### 5.2 Today's monolith

```
┌──────────────────── PlausiDen-Crawler ────────────────────┐
│                                                            │
│  ┌────────────────────────────────────────────────────┐  │
│  │  Playwright runtime (TS)                          │  │
│  │  - Drives Chromium / Firefox / Safari TP          │  │
│  │  - Executes Journey JSON                          │  │
│  │  - Captures telemetry per step                    │  │
│  └────────────────────────────────────────────────────┘  │
│                          │                                 │
│                          ▼                                 │
│  ┌────────────────────────────────────────────────────┐  │
│  │  crates/ (Rust workspace)                          │  │
│  │  ┌─────────────────┐  ┌─────────────────┐         │  │
│  │  │ crawler-journey │  │ crawler-report  │         │  │
│  │  │ typed Journey   │  │ typed Report    │         │  │
│  │  └─────────────────┘  └─────────────────┘         │  │
│  │  ┌─────────────────┐  ┌─────────────────┐         │  │
│  │  │ crawler-detectors│ │ crawler-runner  │         │  │
│  │  │ N typed detector │ │ orchestration   │         │  │
│  │  │ implementations  │ │ glue             │         │  │
│  │  └─────────────────┘  └─────────────────┘         │  │
│  └────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
       │                            │                  │
       ▼                            ▼                  ▼
   Forge phase                   CMS audit log     Annotator session
   ┌──────────────┐         ┌─────────────────┐    ┌─────────────────┐
   │ phase_crawl  │         │ post-publish    │    │ replay user-    │
   │ - runs       │         │ verification    │    │ flagged session │
   │   Crawler    │         │ landing in      │    │ as a journey    │
   │   pre-deploy │         │ audit.log       │    │                 │
   └──────────────┘         └─────────────────┘    └─────────────────┘
```

Multi-tenant + multi-browser future:

```
┌── tenant A ──┐  ┌── tenant B ──┐
│ chromium     │  │ chromium     │
│ firefox      │  │ firefox      │
│ safari TP    │  │ safari TP    │
│  × 3 viewport│  │  × 3 viewport│
│  = 9 runs    │  │  = 9 runs    │
└──────────────┘  └──────────────┘
       │                 │
       └────────┬────────┘
                ▼
       ┌────────────────────┐
       │ Crawler scheduler  │
       │ - parallel runner  │
       │ - per-tenant       │
       │   isolation        │
       │ - per-browser      │
       │   findings merged  │
       └────────────────────┘
```

## 6. Roadmap from now to "done"

### Sprint 1 — close the existing-detector backlog

- Memory-leak / event-listener-leak detection
- Form submission positive-path verification
- Keyboard-trap detection
- Screen-reader landmark presence verification
- Flaky-step timing-flake tagging

### Sprint 2 — cross-config matrix

- Headless Firefox runtime
- Mobile / tablet / desktop viewport per-journey rotation
- Per-locale URL audit
- Network throttling (3G / 4G / offline) per-journey
- Service-worker / cache-storage state capture

### Sprint 3 — pixel-diff + reporting polish

- Pixel-hash visual diff vs baseline
- Per-step DOM-snapshot diff vs baseline
- Web-vitals trend across rolling window
- HTML report (human-readable + interactive)
- Markdown report polished + Forge-ready

### Sprint 4 — agent + CMS + Forge tight integration

- MCP server exposing Crawler tools
- `crawler auto-record` from human clicks
- `crawler auto-discover` via BFS exploration
- Streaming finding firehose for agents
- CMS publish event auto-triggers Crawler verification
- Crawler `Finding` lands in CMS audit log
- Forge `phase_crawl` becomes first-class (today: shell-out)
- Annotator session JSON ingestible as a journey

### Sprint 5+ — the supersociety horizon

**For Mom (non-technical client):**
- Auto-recorded journey from her own editor clicks
- Plain-English deploy verification ("looks good" / "X broke")
- One-button "show me what broke" with screenshot + console excerpt
- Auto-fix suggestions when the finding maps to a known fix

**For the technical client:**
- Cross-browser matrix (Chromium + Firefox + Safari TP)
- Cross-device matrix (mobile / tablet / desktop)
- Localization audit (every locale loads cleanly)
- Performance regression budgets per-page with auto-baseline
- Real-User-Monitoring (RUM) ingestion alongside synthetic
- Captcha / paywall / bot-defense detection
- A/B test arm verification

**For the developer:**
- Pluggable browser backend (Playwright / Selenium / Servo /
  Ladybird)
- Pure-Rust headless browser wrapper (Servo / Ladybird) for
  non-V8 audit
- `crawler-watch` mode (re-run on file save)
- `crawler-replay` (replay captured network log)
- TLA+ specification of the journey + state-machine
- Mutation-testing CI gate

**For Claude Code (and other autonomous agents):**
- MCP server exposing every Crawler capability
- `crawler shrink-finding` (bisect to smallest reproducer)
- `crawler explore` (autonomous journey discovery)
- Time-travel debug across captured run
- Annotator integration (replay flagged sessions)
- Cost / time budgets per agent session
- Stable JSON-RPC API for non-MCP orchestrators

**Cross-cutting supersociety capabilities:**
- Tor / VPN / privacy-mode crawl (measure fingerprintability)
- Cookie / localStorage / sessionStorage diff (privacy audit)
- Identifying every third-party domain a site reaches
- Per-detector property-based fuzz with corpus replay
- Memory-safe Rust runner replacing the TS orchestration
  layer (long-term — TS stays as one valid runner among many)
- Reproducible-run attestation (signed report + run-state hash
  in a transparency log)
- Cross-Crawler federation — a tenant can opt into "any peer
  crawler in the federation can also verify my deploys" for
  decentralized verification

## 7. Future shape — three years out

PlausiDen-Crawler becomes the privacy-respecting runtime audit
substrate for every PlausiDen-served site AND for any external
site whose owner runs it. The TS / Playwright runtime stays as
ONE valid backend; pure-Rust browser wrappers (Servo / Ladybird)
land for non-V8 audit. The journey schema is stable across all
backends. Findings are typed, deduplicated, severity-laddered.

Crawler doesn't try to be a vulnerability scanner (that's the
Vulnerability-Scanner crates' job), an OSINT tool (Salesman /
Harvest cover that), or a search-engine indexer. What it owns:
journey-driven runtime verification of intended behaviour. If
your site says "click checkout, see thank-you page," Crawler
proves that flow still works after every deploy. If it doesn't,
the deploy is held.

The cross-cutting wins from sister repos compound:

- **CMS** triggers a Crawler verification at every publish.
- **Forge** runs Crawler as a build phase before signing the
  bundle.
- **Annotator** captures a human reviewer's flagged elements;
  Crawler converts the annotation into a journey assertion so
  that finding never recurs unnoticed.
- **Loom** serves the rendered output Crawler audits.

Each tenant's per-deploy verification runs in <60 seconds
across Chromium + Firefox + Safari Tech Preview, three viewports,
all locales, with full visual diff. Mom sees "verified" in her
editor; the Sacred.Vote-class technical client sees the full
matrix in their dashboard. Both get the same supersociety
guarantees — Crawler does not have a Pro tier.

## 8. Acceptance criteria for "done"

PlausiDen-Crawler is **done** when:

1. Mom never has to know Crawler exists — Forge invokes it,
   surfaces only the actionable findings, in plain English.
2. Every deploy of every PlausiDen-served site is verified by
   Crawler before going live. Strict findings hold the deploy.
3. Every finding class has a typed detector, a property-based
   test that proves the detector never panics, and documentation
   of when it fires + how to fix it.
4. Cross-browser matrix (Chromium + Firefox + Safari TP) runs
   in CI by default for every site.
5. Cross-device matrix (mobile + tablet + desktop) runs by
   default.
6. Every Crawler run is reproducible — same journey + same
   target + same browser version → same findings (modulo
   tagged flake).
7. Crawler can audit ANY URL on the internet (not just
   PlausiDen-served ones); third-party site owners can run it.
8. Memory-safe pure-Rust browser backend reaches parity with
   Playwright for the journey actions PlausiDen sites use.
9. The privacy-mode crawl (Tor + cookie diff + third-party
   domain reach) lets owners measure their own sites'
   fingerprintability against the privacy floor.
10. The threat model from `~/.claude/CLAUDE.md` (state-actor
    adversary, full breach, unlimited time) holds against the
    deployed crawler — no captured DOM / screenshot leaks
    beyond the per-tenant scope.

The verdict is always **STILL BROKEN** — shipping is risk
acceptance, not a declaration of correctness. The loop resumes
on the next commit.
