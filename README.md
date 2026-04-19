# PlausiDen-Crawler

Headless-browser audit tool. Drives any URL through a scripted journey (click buttons, fill forms, navigate tabs) while capturing:

- Every `console.log/warn/error` message.
- Every `window.onerror` and `unhandledrejection`.
- Every failed network request (non-2xx / aborted / refused).
- Every render-blocking or console-logged Content Security Policy violation.
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

Exits non-zero if console-errors or failed-requests exceed the per-journey budget.
