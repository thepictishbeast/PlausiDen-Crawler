# AGENTS.md — PlausiDen-Crawler

Orientation for any AI agent (Claude or otherwise) working in this repository. Read **before** writing any code or running any script.

> Per [[tool-starvation-anti-pattern]] doctrine: the failure mode that wastes most time is reaching for generic tools (bash, grep, find, curl, hand-rolled scripts) when a platform tool already exists. Stop and check first.

> Cross-repo orientation: see [PlausiDen-Forge/PLAUSIDEN_ECOSYSTEM.md](../PlausiDen-Forge/PLAUSIDEN_ECOSYSTEM.md) for how Crawler relates to Forge / Loom / Annotator / CMS / Canon / Meta / AVP-Doctrine / LFI / Forge-LFI.

> Tool surface for AI clients: see [PlausiDen-Forge/mcp/manifest.json](../PlausiDen-Forge/mcp/manifest.json) — declares the `crawler_journey` MCP tool.

---

## RULE 0 — Consumer-agnostic substrate (LOAD-BEARING)

Per `[[crawler-stays-general-purpose]]` memory: Crawler must work for every consumer — Sacred.Vote, Loom, Forge, third-party sites, anything. **The journey schema is consumer-agnostic; detectors are universal; nothing introduces Forge-specific (or Loom-specific) coupling on the Crawler side.**

**Forbidden:** Forge-specific Detector impls, Loom-shaped journey conventions, hand-rolled URL-fetching scripts to skirt the journey runner. Per `[[substrate-only-path]]` doctrine + crawler-stays-general-purpose.

**Canonical defaults:**
- Journey schema (per `crawler-journey` crate): typed JSON steps — `goto` / `screenshot` / `dom_assert` / `detector_axis` / `wait` / `viewport`.
- chromiumoxide as the browser-automation engine (Send / Sync; no thread-pinning).
- All detector logic in `crawler-detectors`; consumer-agnostic.
- Output: structured runs/ directory with `<journey>-<timestamp>/` subdirs containing screenshots + per-step JSON + the rendered diff report.

Per `[[deterministic-first-lfi-optional]]`: detectors are pure deterministic Rust code. No AI judgment. LFI augmentation is opt-in via the Critic trait abstraction in forge-critic (when a consumer wants AI-graded analysis).

---

## RULE 1 — Look before you build (tool selection)

Before reaching for bash/curl/playwright-cli/puppeteer-cli/hand-rolled scripts:

1. **`crawler --help`** — see if the subcommand exists.
2. **Scan the journey templates in `journeys/`** — many composition shapes are already prepared.
3. **Check `crates/crawler-detectors/`** for existing Detector impls — broken-text / hidden-elements / contrast / overflow / FOUC / etc.
4. **Check `crates/crawler-runner/`** for the runtime entrypoint.
5. **If none of the above** — propose an extension via capability-request, never route around.

---

## Tool inventory (use these — don't reinvent)

**Top-level CLI:**

```
crawler --journey <file>              Run a journey through chromiumoxide.
crawler --journey <file> --headless   Headless mode (default in CI).
crawler --viewport WxH                Override the journey's declared viewport.
crawler --output <dir>                Override the runs/ output directory.
```

**Crate map:**

| Crate | Owns |
|-------|------|
| `crawler-runner` | Top-level binary; argv parsing + journey dispatch. |
| `crawler-journey` | Typed `Journey` + step variants (`Goto` / `Screenshot` / `DomAssert` / `DetectorAxis` / etc). The consumer-agnostic schema. |
| `crawler-detectors` | Detector trait + the canonical detector impls (contrast / viewport-overflow / hidden-elements / FOUC / etc). |
| `crawler-browser-matrix` | Cross-browser / cross-viewport matrix configuration. |
| `crawler-debug-capture` | Diagnostic capture for journey failures. |
| `crawler-layout-spec` | Layout-shape assertions decoupled from the journey. |
| `crawler-report` | runs/ output rendering + diff against historical baselines. |

**Journeys** (in `journeys/`):

- `loom-state-matrix.json` — Loom UI state coverage (state-matrix style)
- `loom-edit-server.json` — `loom edit serve` smoke / regression journey
- `lfi-landing-smoke.json` — LFI smoke test
- `lfi-meta-chart.json` — LFI evaluation visualization
- `css-health-fixtures.json` — CSS-health detector fixtures
- `fixture-perf-csp.json` / `fixture-perf-csp-only.json` — perf budget + CSP coverage
- `forge-skillshots-build.json` — Forge build smoke
- `loom-state-matrix.whitelist.json` — known-passing baseline whitelist

---

## Anti-patterns — do NOT do these

- ❌ Hand-rolling Puppeteer / Playwright scripts → use a typed journey JSON.
- ❌ Using `curl https://target.com/...` to fetch a page → journeys handle goto + render + post-JS state correctly.
- ❌ Forge-specific Detector impl → detectors are universal; if a check is Forge-specific, it belongs in a Forge phase + a Crawler journey assertion that's consumer-agnostic.
- ❌ Embedding hard-coded site URLs in Detector impls → URLs come from the journey JSON, not from detector code.
- ❌ Direct chromiumoxide invocation outside `crawler-runner` → the runner owns the browser lifecycle.

---

## Chromium lifecycle (formerly task #182)

`crawler --journey ... --headless` used to leave defunct `chromium-shell` entries in `/proc` after journey completion. Root cause: the runner's `let _ = browser.close().await;` shutdown step did not actively wait on the OS process, leaving chromiumoxide's tokio `kill_on_drop` to reap in the background with no timing guarantee.

Fixed in `crates/crawler-runner/src/chromium_lifecycle.rs`:
1. CDP `Browser.close` with a 3s timeout.
2. Up to 10 × 50ms `try_wait` polls.
3. `Browser::kill` (SIGKILL + wait) as fallback.
4. Optional `chrome_crashpad_handler` sweep gated on `CRAWLER_REAP_CRASHPAD=1` for single-user hosts.

Full root cause + design notes: see `CRAWLER_ZOMBIE_AUDIT.md`.

`make kill-chromium-zombies` is preserved as a disaster-recovery escape hatch for the rare case where the runner SIGSEGVs before reaching shutdown (so the lifecycle module never runs).

---

## Trait + orientation declarations

Per `[[trait-dag]]`: Crawler Detectors declare these default-required traits via `trait-manifest.toml` (when migrated per task `#170` [trait-v5]):

- `manifested` / `property-tested` / `regression-fixtured`

Runtime trait verification ships in `crawler-detectors::trait_verification` (#170). The detector is **consumer-agnostic**: callers supply a list of `(entity_id, selector, declared_traits)` rows plus a `trait_id → predicate` registry; the detector emits a strict finding for every runtime-verifiable trait the rendered DOM does not uphold. The ecosystem-default registry maps 11 runtime-verifiable traits (`has-accessible-name`, `keyboard-operable`, `focusable`, `lang-aware`, `theme-aware`, `mobile-friendly`, `rtl-aware`, `lazy-loadable`, `reduced-motion-aware`, `screen-reader-accessible`, `touch-target-sized`) plus marks source-level traits (`manifested`, `versioned`, `doctrine-cited`, `substrate-native`, `no-site-specific`, `bundle-size-bounded`, `audit-passing`, `non-flaky`, `deterministic-baseline`) as `NotRuntimeVerifiable` so they pass through cleanly to the consumer's source-level audit.

The matching journey step is `{ "kind": "verifyTraits", "probes": [...], "registryOverrides": {...} }` — see `journeys/loom-trait-verification.json` for the canonical shape.

---

## Doctrine references

- `CRAWLER_REGISTRY.md` — registered detectors + journey templates.
- `CRAWLER_STACK_AUDIT.md` — periodic stack-audit doc.
- `PlausiDen-AVP-Doctrine` — AVP-2 protocol + rule database.
- `PlausiDen-Forge/AGENTS.md` — Forge-side companion.

---

## First steps when starting work in this repo

1. **Read this file** (you are doing that now).
2. **Run `cargo build --workspace`** to confirm green baseline.
3. **Pick a journey in `journeys/`** that's closest to the change you're making, run it (`crawler --journey journeys/<...>.json --headless`), see what passes.
4. **Add detector impls** in `crawler-detectors`, NOT inline in journey JSON.
5. **State the goal** in one sentence — does it match an existing journey shape? a new Detector? a runner extension? Reach for the typed thing first.

If you are about to invoke bash/curl/playwright-cli on Crawler-managed state, stop and re-read RULE 0.
