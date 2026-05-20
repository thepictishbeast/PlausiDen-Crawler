# TOOLS.md — PlausiDen-Crawler

Canonical command index for the `crawler` runtime + the crawler-* crate surface.

> Cross-repo TOOLS reference: see [../PlausiDen-Forge/TOOLS.md](../PlausiDen-Forge/TOOLS.md). Forge consumes Crawler via journey JSON.

---

## Journey execution

```
crawler --journey <file>              Run a typed journey through chromiumoxide.
crawler --journey <file> --headless   Headless mode (default in CI).
crawler --viewport WxH                Override the journey's declared viewport.
crawler --output <dir>                Override the runs/ output directory.
crawler --journey <file> --json       Emit machine-readable journey result.
```

---

## Journey templates (in `journeys/`)

```
journeys/loom-state-matrix.json              Loom UI state coverage
journeys/loom-edit-server.json               loom edit serve smoke / regression
journeys/lfi-landing-smoke.json              LFI smoke test
journeys/lfi-meta-chart.json                 LFI evaluation visualization
journeys/css-health-fixtures.json            CSS-health detector fixtures
journeys/fixture-perf-csp.json               Perf budget + CSP coverage
journeys/forge-skillshots-build.json         Forge build smoke
journeys/loom-state-matrix.whitelist.json    Baseline whitelist for the state matrix
```

---

## Crate map

```
crates/crawler-runner            Top-level binary; argv parsing + journey dispatch
crates/crawler-journey           Typed Journey + step variants (consumer-agnostic schema)
crates/crawler-detectors         Detector trait + canonical impls (contrast / viewport-overflow / hidden-elements / FOUC / etc)
crates/crawler-browser-matrix    Cross-browser / cross-viewport matrix
crates/crawler-debug-capture     Diagnostic capture for journey failures
crates/crawler-layout-spec       Layout-shape assertions decoupled from journeys
crates/crawler-report            runs/ output rendering + diff against baselines
```

---

## Crate-level direct invocation

```
cargo build --release -p crawler-runner   # Build the crawler binary
cargo test --workspace                    # Crawler-wide test suite
cargo run -p crawler-runner -- --journey journeys/<file>.json   # Direct invocation
```

---

## Anti-patterns — DO NOT do these

- ❌ Hand-rolling Puppeteer / Playwright scripts → use a typed journey JSON.
- ❌ Using `curl https://target.com/...` to fetch a page → journeys handle goto + render + post-JS state correctly.
- ❌ Forge-specific Detector impl → Crawler stays consumer-agnostic (per `[[crawler-stays-general-purpose]]`).
- ❌ Embedding hard-coded site URLs in Detector impls → URLs come from journey JSON.
- ❌ Direct chromiumoxide invocation outside `crawler-runner` → the runner owns the browser lifecycle.

---

## Known investigation

- **#182 chromium-shell zombies** — chromiumoxide can spawn defunct chromium child processes. Workaround: `pkill chromium-shell` after a hung headless run. Permanent fix in progress.

---

## See also

- `AGENTS.md` — orientation including Rule 0 + Rule 1
- `CRAWLER_REGISTRY.md` — registered detectors + journey templates
- `CRAWLER_STACK_AUDIT.md` — periodic stack-audit
- `../PlausiDen-Forge/TOOLS.md` — Forge-side TOOLS index
- `../PlausiDen-Forge/mcp/manifest.json` — `crawler_journey` MCP tool
