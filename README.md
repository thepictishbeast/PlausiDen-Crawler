# PlausiDen-Crawler

Headless-browser audit tool. Drives any URL through a scripted journey (click buttons, fill forms, navigate tabs) while capturing:

- Every `console.log/warn/error` message.
- Every `window.onerror` and `unhandledrejection`.
- Every failed network request (non-2xx / aborted / refused).
- Every render-blocking or console-logged Content Security Policy violation.
- WCAG violations via injected axe-core (writes `findings.txt` + per-step annotated screenshots).
- **PlausiDen D₀ design-system violations** (44×44 touch targets, 36px control min-height) via the runtime audit in `src/d0Audit.ts` (writes `d0-findings.txt`).
- Screenshots at each step.

Output is a single JSON bundle per run (`runs/<host>-<ts>/report.json`) plus per-step PNGs, suitable for diffing across deploys or pasting into a bug tracker.

## Why this exists

The PlausiDen-AI UI keeps shipping regressions that only surface in the browser at runtime (TDZ crashes, CSP-blocked fetches, stale-chunk load failures). Static lints + TypeScript catch some — the rest only reveal themselves when a real browser executes the code. This crawler runs the journey unattended after every deploy and reports everything the user would otherwise eat in their own console.

## Status

Scaffold only (2026-04-19). Playwright-based runner stubbed in `src/main.ts`. See `docs/PLAN.md` for the build-out roadmap.

## Run

```bash
npm install
npx playwright install chromium
npm run audit -- --url http://10.99.0.3:3000/
```

Exits non-zero if console-errors, failed-requests, axe a11y violations, or D₀ design-system violations exceed the per-journey budget.

## D₀ runtime audit

The crawler enforces PlausiDen's [design-system standards](https://github.com/thepictishbeast/plausiden-standards) on the **rendered** page, not just the source. This complements:

- `axe-core` (broad WCAG checks, looser 24×24 touch-target floor).
- `plausiden-standards/audits/*` (build-time, source-level: raw `<input>`, raw color literals, missing overflow safety classes).

Runtime checks (v0):

1. **`touch-target`** — every interactive element renders ≥ 44×44 CSS px (PlausiDen UI-STANDARDS §1.1, iOS HIG, WCAG 2.5.5 AAA).
2. **`control-min-height`** — every form control's computed `min-height` ≥ 36px (matches the `min-h-9` baseline from UI-STANDARDS §2.1).

Findings land in `runs/<ts>/d0-findings.txt` with selector + section heading + nearby text + bounding box, so a triager can locate the offending element on the live page in seconds. Per-journey budget knob `newD0Violations` tolerates a baseline while migration is in flight.
