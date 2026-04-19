# First real crawler run — 2026-04-19 against 10.99.0.3:3000

Target: `http://10.99.0.3:3000/` (user's WireGuard-fronted self-hosted instance).
Journey: `plausiden-smoke` (33 steps: land → dismiss overlays → toggle sidebar → jump through every view → open Admin → close).

## Headline

**31 of 33 steps passed. 0 console errors. 0 failed fetches. 0 a11y violations.** (Note: crawler was run against a running static server; backend WS + /api may still have had issues not counted here.)

Two failures, both the same root cause: `page.click('[data-tour="chats-toggle"]')` timed out at 3s because **`<div>…</div> intercepts pointer events`** over the button.

## What that means

Something (likely the Welcome modal or a stale tour overlay) is rendering on top of the header chats-toggle button during initial load. The Escape-press at the top of the journey dismisses the tour overlay, but the Welcome modal doesn't respond to Escape on this build — so its backdrop keeps catching clicks.

This would bite a real user too: **first-visit users can't click the header controls until they dismiss Welcome**. The 6-view nav strip below works (it's above the Welcome modal in z-stack, or below the backdrop depending on config), but header elements are blocked.

## Action items

### UI fixes (claude-2, upcoming)

1. Add Escape key handler to `Welcome` modal so it can be dismissed with one keypress (matches every other modal).
2. Lower Welcome backdrop z-index OR raise header chats-toggle z-index so header controls stay interactive. Welcome is informational; blocking the whole header is overkill.
3. Alternative: make Welcome's click-on-backdrop dismiss it (common UX pattern for non-destructive modals).

### Journey tweak

- Add a step after the Escape presses that explicitly dismisses the Welcome modal if present (click its "Start chatting" / "OK" button). That's more robust than relying on Escape.

## Positive signals

- Keyboard shortcuts (`Cmd+1..6`, `Cmd+3` to open Admin, `Escape` to close) ALL worked.
- All 6 views rendered without console errors.
- All 8 screenshots captured successfully (see `runs/<ts>/*.png`).
- No CSP violations, no `ERR_CONNECTION_REFUSED` — backend was reachable this run.
- Report + diff wired correctly: `diff vs prior run` showed `0 new regressions` against the previous run's same-shape failures. That's the budget gate working.

## Screenshots inspected

- `01-landing.png` — landing view post-Escape (still likely showing Welcome modal; confirms the blocker)
- `02-post-sidebar.png` — sidebar toggled via `Cmd+B` (succeeded where the button click failed)
- `03-classroom.png` through `08-final.png` — every view rendered cleanly via keyboard navigation.

## What to do next

- Ship a fix for the Welcome backdrop-blocking issue (and add a crawler journey step that regression-tests it).
- Add a second journey targeting Admin sub-tabs (Dashboard, Proof, Diag, Docs, Backup card interaction).
- Wire axe-core so a11y violations get counted too.
