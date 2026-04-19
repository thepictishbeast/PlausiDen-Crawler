# FOSS fork candidates — deep survey

Focused shortlist of projects suitable to fork/vendor per AVP-2 FOSS absorption protocol. Each entry evaluates: license, fit, fork-friendliness, risk.

## The "engine" layer — already settled

| Project | License | Why | Status |
|---|---|---|---|
| **Playwright** | Apache 2.0 (Microsoft) | Multi-browser automation, first-class event hooks (console/page-error/request-failed), CDP trace. Active, huge contributor base. | ✅ Direct dep. No fork needed. |

## Tier A — absorb as thin adapter (recommended)

These projects wrap a specific concern and are tight enough to fork cleanly.

### 1. pa11y-ci — accessibility
- **License:** MIT (pa11y.org, originated at Nature Publishing Group)
- **Repo:** https://github.com/pa11y/pa11y-ci
- **Lines:** ~2k (ci package) + pa11y core ~6k
- **What it does:** runs Pa11y (which itself runs axe-core and HTML CodeSniffer) against a list of URLs. CI-first. Output: JSON with issue objects — selector, message, runner, severity.
- **Fork path:** clone → strip sitemap crawler (we have our own journey) → keep the axe-core invocation → pipe results into our `CapturedEvent` shape.
- **Why fork over dep:** pa11y-ci 4.x is ESM-only with peer-dep churn. A frozen fork removes the upgrade churn risk.
- **Risk:** LOW. Core logic is a thin shell over axe-core which we'd pin separately.

### 2. axe-core — the accessibility rule engine
- **License:** MPL-2.0 (Deque Systems)
- **Repo:** https://github.com/dequelabs/axe-core
- **Lines:** ~70k (rules + DOM traversal + matchers)
- **What it does:** 90+ WCAG rules, run via `axe.run()` inside a page. Result: violations[] with html, target, failureSummary.
- **Fork path:** don't fork the whole engine; absorb as runtime dep. Write our own Playwright invocation that loads `axe.min.js` into the page and calls `axe.run()`.
- **Why:** the rule set is the IP; rewriting it would take years. MPL-2.0 allows static linking but file-level modifications must be open.
- **Risk:** MEDIUM — MPL viral at file level. Stay at arm's length (dep, not vendored source).

### 3. lighthouse-ci — perf + best-practices + CSP
- **License:** Apache 2.0 (Google Chrome team)
- **Repo:** https://github.com/GoogleChrome/lighthouse-ci
- **Lines:** ~15k (runner) + upstream Lighthouse ~200k
- **What it does:** drives headless Chrome, runs full Lighthouse audit (perf, a11y, BP, SEO, PWA), asserts against config budgets. CI-native: `lhci autorun` is a one-liner.
- **Fork path:** don't fork the audit itself — it's enormous. Fork only the CLI wrapper + config spec, shell out to upstream Lighthouse.
- **Why absorb:** catches render-blocking scripts, unsized images, failing CSP directives, slow LCP — none of which a Playwright smoke catches.
- **Risk:** LOW. Well-maintained, Apache 2.0, has a documented API.

### 4. webhint (`@hint/cli`) — best-practice auditor
- **License:** Apache 2.0 (Microsoft → OpenJS Foundation)
- **Repo:** https://github.com/webhintio/hint
- **Lines:** ~40k (monorepo, including 50+ hint plugins)
- **What it does:** pluggable hint system; hints are small modules that inspect a page and emit issues. Ships with hints for: meta-tags, security headers, TLS config, compatibility, etc.
- **Fork path:** fork the core `hint` runner + 3-4 essential hints (meta-tags, security-headers, no-broken-links). Drop the rest. Write 1-2 PlausiDen-specific hints (e.g. "every action POST returns training_actions_applied count").
- **Why:** the plugin architecture is exactly what our audit layer wants, without reinventing it.
- **Risk:** MEDIUM. Less-active upstream than Lighthouse; fork risk of divergence if we don't track releases.

## Tier B — useful but too heavy to fork

### sitespeed.io
- MIT, very comprehensive (HAR, filmstrip, WebPageTest). Docker-only workflow. Our CI is not Docker-native. Wrap-only, no fork.

### tracerbench
- Apache 2.0 (LinkedIn). Chrome trace analysis. Overkill for our smoke; revisit for v1.0 when we care about LCP deltas.

### puppeteer-extra
- MIT. Plugin system over puppeteer. We use Playwright → no fit unless we migrate.

## Tier C — reject

- **Cypress** — MIT but opinionated, not fork-friendly, redundant with Playwright.
- **TestCafe** — MIT but single-org (DevExpress) with commercial focus.
- **Selenium** — Apache 2.0 but legacy architecture (WebDriver proxy). Playwright is newer, less overhead.

## Concrete fork + vendor plan for v0.2

```
PlausiDen-Crawler/
├── src/
│   ├── main.ts                 # runner (us)
│   ├── journey.ts              # step executor (us)
│   ├── report.ts               # event normaliser + diff (us)
│   ├── adapters/
│   │   ├── axe.ts              # deps: axe-core@4 (npm dep, not forked)
│   │   ├── lighthouse.ts       # deps: @lhci/cli (npm dep, not forked)
│   │   └── webhint.ts          # FORKED from @hint/cli into vendor/webhint/
│   └── vendor/
│       ├── webhint/            # frozen fork, AVP-2 absorbed
│       └── pa11y-thin/         # our slim rewrite of pa11y-ci calling axe directly
├── journeys/                   # us
└── docs/
```

AVP-2 absorption cycle to execute for each fork:

1. **Discover** — links above.
2. **Evaluate** — this doc.
3. **Absorb** — clone at a pinned commit, strip unused features, run `cargo geiger` equivalent (`npm ls`, `npm audit`), replace every `process.exit` with thrown errors (library-mode), add input validation at public API surfaces.
4. **Integrate** behind `adapters/*.ts` so the rest of the project only touches the adapter.
5. **Loop AVP-2 Tiers 1–3 (≥12 passes)** on the forked code — fault injection, adversarial input, supply-chain check.
6. **Maintain** by diffing each upstream release quarterly and cherry-picking security fixes only.

## Immediate next step (this cycle)

Ship the **axe-core adapter** as a pure-dep integration since the risk profile is lowest and it catches the most bugs per line:

- add `axe-core` to package.json
- `src/adapters/axe.ts`: loads `axe.min.js` from node_modules, calls `axe.run()` after each journey step, normalises violations into `CapturedEvent` with `kind: 'a11y-violation'`.
- report.json grows a `a11y` section; budget asserts 0 serious/critical.
