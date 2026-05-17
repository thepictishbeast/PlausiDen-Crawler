> # ⚠️ DO NOT USE — UNVERIFIED — UNSAFE ⚠️
>
> This software is **unverified and unsafe for any production use**.
> It is published publicly only for transparency, third-party audit,
> and reproducibility. Treat every commit as guilty until proven
> innocent.
>
> By using this code you accept:
> - **No warranty** of any kind, express or implied.
> - **No fitness** for any particular purpose.
> - **No guarantee** of correctness, safety, or freedom from defects.
> - **Zero liability** on the maintainer for any damages — data loss,
>   security compromise, financial loss, or any consequential damages.
>
> The code is under active engineering development per the
> [Adversarial Validation Protocol v2](https://github.com/thepictishbeast/PlausiDen-AVP-Doctrine/blob/main/AVP2_PROTOCOL.md).
> Every commit's default verdict is **STILL BROKEN**. AVP-2 requires
> a minimum of 36 verification passes before a `SHIP-DECISION:`
> annotation may be considered. **No commit in this repository has
> reached `SHIP-DECISION:` status.**

# PlausiDen-Crawler

Headless-browser audit tool. Drives any URL through a scripted
journey (click buttons, fill forms, navigate tabs) while capturing
console errors, page errors, failed network requests, CSP
violations, axe-core a11y findings, and 40+ runtime detector
axes (CSP / COEP / COOP / CORP / HSTS / referrer-policy /
mixed-content / form-labels / tap-targets / heading-order /
contrast / placeholder-text / etc.).

Output is one JSON bundle per run (`runs/<host>-<ts>/report.json`)
plus per-step PNGs.

> ## ⚠ Status: pre-1.0, AVP-2 in flight — NOT production-ready
>
> Tests pass locally; CI may or may not be green at any given
> moment. APIs, CLI flags, and on-disk formats can and will change.
> Licensed under [FSL-1.1-MIT](./LICENSE) — source-available with a
> 2-year competitor-restriction window, then converts automatically
> to MIT.
>
> The repo currently houses **two parallel runtimes**:
>
> * The original TypeScript Playwright pipeline (`src/main.ts`) —
>   exercised against production sites today.
> * The Rust chromiumoxide port (`crates/`) — 45 detector axes ported
>   and wired through; integration tests cover the journey-step +
>   detector flow.
>
> Per T75 (#640), both runtimes ship. Operators choose at invocation
> time via `npm run audit` (TS) or `npm run audit:rust` (Rust).
> Owner decides when the TS path is removed.

## Run

The TS runtime (production-exercised):

```sh
npm install
npx playwright install chromium
npm run audit -- --url http://127.0.0.1:8123/
```

The Rust runtime (chromiumoxide port — same detector axes, native
performance):

```sh
# build once
cargo build --release -p crawler-runner

# run with the same args as the TS path
npm run audit:rust -- --journey journeys/lfi-landing-smoke.json
# or directly:
./target/release/crawler --journey journeys/lfi-landing-smoke.json
```

Shipping npm shortcuts:

| Script                  | What it does                                          |
|-------------------------|-------------------------------------------------------|
| `npm run audit`         | TS runner, default desktop viewport                   |
| `npm run audit:mobile`  | TS runner, mobile viewport (plausiden-smoke-mobile)   |
| `npm run audit:tablet`  | TS runner, tablet viewport                            |
| `npm run audit:all`     | TS runner, all three viewports                        |
| `npm run audit:rust`    | Rust runner — pass `--journey <path>` after `--`      |
| `npm run audit:rust:smoke`  | Rust runner, lfi-landing-smoke journey            |
| `npm run audit:rust:mobile` | Rust runner, plausiden-smoke-mobile journey       |

Exit code is non-zero if console-errors, failed-requests, axe a11y
violations, or detector findings exceed the per-journey budget.

## Journeys

A journey is a JSON file under `journeys/` that scripts the browser:

```json
{
  "name": "lfi-landing-smoke",
  "baseUrl": "http://127.0.0.1:8765/",
  "steps": [
    { "kind": "goto", "url": "http://127.0.0.1:8765/", "timeout": 10000 },
    { "kind": "wait", "ms": 1500 },
    { "kind": "screenshot", "label": "01-landing" },
    { "kind": "press",  "key": "End",  "label": "scroll-to-bottom" }
  ]
}
```

Supported step kinds (both runtimes):
`goto`, `wait`, `screenshot`, `click`, `press`, `scroll`,
`waitForSelector`. Each carries an optional `label` and (for
network/wait operations) an optional `timeout`.

## Layout

```
src/                  TypeScript Playwright runner + detector port-of-record
crates/
  crawler-journey/    Typed journey schema (serde) — shared with the Rust runner
  crawler-detectors/  45 detector axes (pure functions: snapshot → findings)
  crawler-runner/     chromiumoxide-based runner binary (`crawler` CLI)
  crawler-report/     Report schema (serde, wire-compatible with the TS Report)
journeys/             Journey JSON files
fixtures/             Static test fixtures (each subdir has its own serve.py)
runs/                 Per-run report bundles (gitignored)
docs/                 Stack audit + architecture notes
```

## Pre-push hook

Opt in with `git config core.hooksPath .githooks` — blocks push if
the SuperSociety test suite (property + mutation + drift) doesn't
pass. See [`.githooks/pre-push`](./.githooks/pre-push).

## License

[FSL-1.1-MIT](./LICENSE).
