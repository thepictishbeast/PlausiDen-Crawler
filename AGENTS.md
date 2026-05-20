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

## Known investigation: chromium-shell zombies (task #182)

Per loop preamble: chromiumoxide can spawn defunct chromium child processes that survive past journey completion. Symptom: `crawler --headless` hangs after a successful journey because the parent is waiting on a zombie child.

Workaround: manually `kill -9` the chromium zombies via `pkill chromium-shell`.

Permanent fix is filed as task #182. Suspected root cause: the chromiumoxide handle's Drop impl doesn't always SIGTERM the browser process. Investigation paths:
- Pin chromiumoxide to a known-good version.
- Wrap the browser-handle Drop with explicit kill via process group.
- Or switch to playwright-rust if chromiumoxide can't be made reliable.

Until #182 closes, expect occasional manual intervention on CI runners (the GitHub Actions runner cleans up between jobs, so this is primarily a local-dev concern).

---

## Trait + orientation declarations

Per `[[trait-dag]]`: Crawler Detectors declare these default-required traits via `trait-manifest.toml` (when migrated per task `#170` [trait-v5]):

- `manifested` / `property-tested` / `regression-fixtured`

The runtime trait-verification (#170) is the consumer of trait declarations — Crawler asserts at journey time that the consumed entity actually behaves consistent with its declared traits.

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
