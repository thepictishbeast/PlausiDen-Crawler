# Visionless AI Testing — Accessibility Trees + Self-Healing Selectors + LLM-Ready Crawlers

User directive 2026-04-19: absorb the paradigms from "Advanced Paradigms in Visionless AI Web UI Testing" — accessibility trees as AI vision, 10-tier self-healing selectors, and LLM-optimized FOSS crawlers (Crawl4AI, Firecrawl, LibreCrawl).

This doc explains what each concept is, why it matters for us, and which ones are already wired in vs. scheduled.

## The core premise: don't look at pixels

Traditional UI testing fights brittle CSS selectors. Modern AI-driven testing fights token budgets + model hallucinations on screenshots. **Accessibility trees sidestep both.**

The browser already builds an accessibility tree — a role/name/state graph originally designed for screen readers. An entire page's tree is typically <1k tokens vs. ~15k for the raw DOM. And because it's the same API screen-reader users consume, any gap in the tree IS a UX bug — not something we have to infer.

## What we shipped this cycle

### 1. `src/aria.ts` — accessibility tree capture

Each screenshot step now also emits an `.aria.txt` file:

```
banner
  button "Chats" pressed
  button "New chat"
  heading "PlausiDen AI" level=1
main
  textbox "Chat message input"
  button "Send" disabled
navigation "Top level sections"
  tab "Agora" selected
  tab "Classroom"
  tab "Admin"
  tab "Fleet"
  tab "Library"
  tab "Auditorium"
```

Plus a flat interactable-nodes list ("what can the user click"), plus an a11y score (count of unnamed buttons, images without alt, etc.) → logged as `a11y-violation` events so cross-run diff catches regressions.

### 2. `src/selectorHealer.ts` — 10-tier priority resolver

When you know an element by MULTIPLE signals (role + name + testid + CSS + text), the healer tries each in order — most stable first:

1. `getByRole(role, { name })` — W3C, language-independent, survives DOM shifts.
2. `[data-testid="..."]` — engineer-authored, stable contract.
3. `#id` — scoped to page.
4. `[aria-label="..."]` — screen-reader name.
5. `aria-describedby` target text.
6. `[name="..."]` — form fields.
7. `getByPlaceholder`.
8. `getByText`.
9. `[class*="..."]` — class fragment.
10. CSS / XPath — last resort.

The healer returns `{strategy, locator}` so the report shows WHICH tier matched. If tier 1 starts failing but tier 4 succeeds, we know an accessibility-name change broke the contract — actionable signal.

## What's scheduled (v0.4+)

### LLM-ready FOSS crawlers — absorb or integrate?

| Project     | License | Focus                                                           | Integration plan                                                              |
|-------------|---------|-----------------------------------------------------------------|------------------------------------------------------------------------------|
| **Crawl4AI** | Apache 2.0 (Python) | RAG-oriented. Outputs token-efficient Markdown. On-state-change checkpointing. | **Absorb as sidecar** — call via `child_process.spawn('crawl4ai', ...)` when a journey wants the AI-ready Markdown digest of a page. |
| **Firecrawl** | AGPL-3.0 / MIT-dual (TS + Go) | Sitemap + JS-rendered content cleaning. Browser Sandbox mode. | **Absorb via HTTP API** — run Firecrawl as a local service, our crawler posts URLs, receives cleaned Markdown. |
| **LibreCrawl** | MIT | Technical-SEO crawler. 404s, redirects, duplicate content, missing meta. | **Absorb as periodic audit** — run `librecrawl` nightly against the deploy, feed its JSON to our diff. Catches structural issues (dead links, redirect loops) our journey-based crawler misses. |

Integration pattern (thin adapter, per AVP-2):

```
src/vendor/
├── crawl4ai/       (sidecar CLI wrapper)
│   └── adapter.ts  — spawns crawl4ai process, consumes its Markdown
├── firecrawl/      (HTTP client)
│   └── adapter.ts  — POST /v0/crawl, consume JSON
└── librecrawl/     (periodic audit)
    └── adapter.ts  — spawns librecrawl CLI, consumes report.json
```

Outputs normalize into our `CapturedEvent[]` shape. Existing diff + budget gate apply.

### Token-efficient LLM handoff

Beyond raw aria snapshots, v0.4 will emit a `per-step LLM digest`:

```markdown
## Step 12 — jump-classroom (0.9s, ok)

- URL: http://10.99.0.3:3000/#classroom
- Interactables visible: 28 (3 new since prior step)
- A11y warnings: 0
- Console: 0 errors, 2 debug
- Network: 4 requests, 1 failed (/api/admin/dashboard → ERR_CONNECTION_REFUSED)
- Accessibility tree diff vs prior step:
  + tablist "Classroom sections" with 12 tabs
  - tablist "Admin sections"
- Rendered error copy: none
```

A reasoning LLM can ingest that entire 90-step report in <30k tokens and tell you "the classroom-jump step succeeded in the UI but the backend endpoint failed; the user is seeing a tablist with no data."

### Self-healing runtime

When a step fails with "locator not found", the healer re-resolves from the current DOM snapshot and logs a `selector-healed` event. Over time this creates a dataset of "selectors that drift" — an early-warning signal that a UI change will break downstream automation.

## Why this matters for PlausiDen specifically

- **Token economy for the user's training loop.** The user teaches LFI facts. The crawler can now give LFI structured reports of its OWN UI behavior — "this was the state when the user taught X, this was the state after." Facts about the app, for the app.
- **Accessibility is a correctness signal, not a nice-to-have.** An unnamed button in the aria tree is a bug for assistive-tech users AND for any AI agent driving the UI.
- **Self-healing beats brittle fixtures.** PlausiDen's UI moves fast; every commit today can break a test written yesterday. The healer's priority tiers survive most of those changes.
