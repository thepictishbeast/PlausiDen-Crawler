# Crawler Registry

Authoritative list of every web-crawling project the PlausiDen-Crawler
maintainers have evaluated as either a candidate for absorption, a
pattern reference, or competition to track. Mirrors the
[PlausiDen-Audits TOOL_REGISTRY](https://github.com/thepictishbeast/PlausiDen-Audits/blob/main/TOOL_REGISTRY.md)
discipline: every consideration is recorded with a verdict so the same
projects don't get re-evaluated each time someone scans GitHub for
"crawler."

| Status | Meaning |
|---|---|
| **adopted** | Vendored under `vendored/` or wired in via npm/cargo. |
| **adopted-as-dep** | Used directly via package manager; no fork. |
| **deferred** | Genuine value but waiting on a specific trigger. |
| **reference-only** | Pattern source; we read the code, did not absorb. |
| **rejected** | Considered and ruled out; **do not re-evaluate without new evidence**. |

## Current stack baseline

PlausiDen-Crawler is **TypeScript / Node + Playwright + Puppeteer +
Crawlee**. The crawler runs in headless Chromium, drives journey scripts,
and emits rich telemetry (CSP, console, network, web-vitals). Any
candidate is judged against this baseline:

1. Does it add a capability the current stack lacks?
2. Does it speed something up by ≥10× without losing fidelity?
3. Does it open a use-case Playwright can't address (no-browser, deep
   stealth, non-JS sites at scale)?

If none of (1)/(2)/(3), defer or reject — adding tools is not free.

---

## Rust ecosystem (high speed; non-browser)

| Tool | Status | Notes |
|---|---|---|
| **spider-rs/spider** | deferred | The leading Rust web crawler — async, parallel, polite, well-maintained. Performance is genuinely 10-100× a Node crawler for non-JS sites. **Adopt when:** PlausiDen needs a no-JS, high-volume crawl mode (corpus building, link-graph construction, monitoring at scale). Not a Playwright replacement; a complement. |
| **let4be/crusty** | reference-only | Designed for *very* large-scale crawls (millions of URLs). Politeness-policy and crawl-delay design is exemplary. Read the architecture; don't absorb code. |
| **hominee/dyer** | reference-only | Lighter Rust crawler than spider-rs. Builder API design is clean. Reference if we ever build a custom Rust crawler. |
| **Gonzih/crabler** | reference-only | Builder-pattern Rust crawler with declarative selectors. Reference for ergonomics. |
| **0x676e67/wreq** | deferred | **Browser-fingerprint-impersonating HTTP client** (Rust). Different scope from spider-rs — wreq is a `reqwest` alternative that mimics Chrome/Firefox JA3/JA4/HTTP2-fingerprints. Adopt when: a crawl target blocks our headless Chromium and we need a non-Playwright fallback. |
| **0x676e67/wreq-python** | reject-as-dep | Python bindings to wreq. We don't run Python in the crawler stack; if we add wreq, we use it from Rust. |
| **oscar-project/ungoliant** | reject | Specialized for OSCAR corpus mining. Out of PlausiDen scope. |
| **infinilabs/crawler** | reject | Elasticsearch-backed crawler. Heavy infra dependency we don't want. |

## Go ecosystem

| Tool | Status | Notes |
|---|---|---|
| **gocolly/colly** | reference-only | The most mature Go crawler. Selector + middleware pipeline pattern is worth reading. We don't run Go; can't absorb code, but the pattern shape is informative. |
| **Qianlitp/crawlergo** | reference-only | Go crawler built specifically for browser-based security testing (XSS payloads, parameter discovery). If PlausiDen ever offers a security-focused crawl mode, this is the design reference. |

## Python ecosystem

| Tool | Status | Notes |
|---|---|---|
| **s0md3v/Photon** | reference-only | OSINT-focused Python crawler. Fast, lightweight, but Python and not maintained recently. Pattern reference for OSINT-style discovery (emails, URLs, files in scope). |
| **binux/pyspider** | reject | Web-UI-driven Python crawler framework. Heavy stack (mq, webui), unmaintained since 2019. |
| **rugantio/fbcrawl** | reject | Facebook scraper. ToS-violating; PSA-incompatible. Out of scope. |
| **YoongiKim/AutoCrawler** | reject | Image-only crawler tied to Google/Naver. Niche. |
| **ferventdesert/Hawk** | reject | Visual crawler builder, Chinese ecosystem, unmaintained. |
| **Minoru/minoru-fediverse-crawler** | reject | Specialized fediverse crawler. Not a general pattern. |
| **CAIDA/commoncrawl-host-ip-mapper** | reject | One-shot CommonCrawl host-IP utility. Not a crawler in the PlausiDen sense. |

## Java / JS / Other

| Tool | Status | Notes |
|---|---|---|
| **code4craft/webmagic** | reference-only | Mature Java crawler framework. Java-only, not absorbing; pattern reference for selector + pipeline composition. |
| **bda-research/node-crawler** | reject | Node.js crawler that predates Playwright/Crawlee. Crawlee is the modern Node answer; we already use it. |
| **yujiosaka/headless-chrome-crawler** | reject | Built on the deprecated `puppeteer-core` headless-chrome variant. Superseded by Playwright. |
| **janreges/siteone-crawler** | reference-only | PHP-based site auditor with strong reporting (PDF outputs, sitemap, accessibility). Read the report-shape patterns; don't absorb (PHP). |
| **a11ywatch/crawler** | deferred | **Accessibility-focused** crawler. Closest in scope to one of PlausiDen's stated goals. Adopt when: we add formal a11y journey support beyond the current ad-hoc CSP/console capture. |
| **mat-1/x227f** | reject | Tiny experimental crawler. No coverage we lack. |
| **neur0map/docrawl** | reject | Small project; coverage subsumed by Crawlee. |
| **aichat-bot/crawly** | reject | Likely abandoned; no evidence of unique capability. |
| **0xMassi/webclaw** | reject | Small project; no unique capability. |
| **buckyroberts/Spider** | reject | Tutorial code from a YouTube channel. Not production-grade. |

## Audit & quality tools

Not crawlers, so they do not belong in any table above — this file is
organised by language ecosystem for *crawlers*. These are tools that
audit a page the crawler has already loaded. Judged against the same
three tests.

Source: a 12-agent evaluation of ~36 UI/UX tools (2026-09-08). All three
independent judges reached the same verdict on the one adoption, and all
three independently specified the **Node library API** over the CLI.

| Tool | Status | Notes |
|---|---|---|
| **GoogleChrome/lighthouse** | **adopted-as-dep** | Apache-2.0, pinned `lighthouse@13.4.1` exact (not a caret — a minor bump renames audit ids and would silently shrink coverage). The only one of ~36 candidates to pass test 1. Used as a **library via the Node API**; drives Chrome through `chrome-launcher` pointed at the Chromium Playwright already downloaded, so it adds **no** second browser stack. Adapter at `src/lighthouseAdapter.ts`, entry `src/lighthouseAudit.ts`, run by `plausiden-uxaudit.timer`, results normalised to JSONL for `analytics.plausiden.com`. **Only 14 audit ids are ingested** (`src/lighthouseAllowlist.ts`): the byte-weight and network-timing audits no detector covers. Its Accessibility category IS axe-core — already a dependency, already injected at `src/audit.ts` — and its LCP/CLS/INP duplicate the `web-vitals` dep and `web_vitals.rs`; emitting the whole LHR would double-count both into analytics. Offline-clean: no account, no upload, and the bundled `@sentry/node` is opt-in and disabled for the programmatic API. |
| **@lhci/cli** (Lighthouse CI) | **reject** | Same library underneath, wrapped in target lists, retries, output directories and exit codes — every one of which `crawler-runner`, `journeys/` and `runs/` already provide. It also assumes a build/PR workflow that does not exist here. Adopting it would mean two runners that disagree about what a run is. Fails test 1: the library is the part that adds capability, the wrapper is the part that duplicates. |
| **lighthouse** (global CLI) | **reject** | Same reason as `@lhci/cli`, one layer thinner. The library is adopted; the command is orchestration we have. |
| **@axe-core/cli** | **reject** | Strongest reject on the list: zero new capability. It runs the same `axe-core` already in `package.json` and already injected at `src/audit.ts`, and would add a Selenium/chromedriver chain to do out-of-process, one URL at a time, what the crawler does in-process across a journey matrix. (Note the name trap: the literal `axe-cli` on npm is the abandoned 2022 predecessor.) |
| **pa11y** | **reject** | Almost entirely duplicated — Puppeteer, headless Chromium and axe-core are already here. The only thing it adds is the HTML_CodeSniffer ruleset, a noisier WCAG-techniques second opinion. If that opinion is ever wanted, vendor HTML_CodeSniffer's single JS file through the existing `src/audit.ts` injection path rather than adopting a whole duplicate browser stack. See `docs/PA11Y_WCAG_PARITY.md`. |
| **pa11y-ci** | **reject** | The most duplicative candidate evaluated. Sitemap crawling, concurrency, per-target thresholds and CI exit codes are exactly `crawlee` + `journeys/` + `crawler-runner` + `crawler-report`. Orchestration for a runner we would not adopt. |
| **uxlint** | **reject** | Name does not resolve cleanly: two unrelated projects share it, and the probable referent is a ~6-star personal LLM UX reviewer whose 4.5.0 version number badly oversells its maturity. The other is a 2022-dead eslint wrapper that is not a UX tool at all. An LLM critique generator is also not deterministic, which is the property every other row here is judged on. |
| **Visual Regression Tracker** | **deferred** | Apache-2.0, self-hosted. Baseline **history** across branches/OS/viewport plus an approve/review workflow is the one layer that genuinely does not exist here — `pixel_diff.rs` compares against a checked-in baseline and has no lifecycle. Deliberately the only deferred entry for this concern, so one need does not accumulate three registry rows. |
| **reg-cli** | **reject** | Supplies only glue — pairing, manifest, promotion — over a differ, a capture and a finding taxonomy `pixel_diff.rs` already owns. The contrast with the row above is deliberate: VRT is deferred for baseline *history*, which we would otherwise build from scratch; reg-cli is rejected because its layer already exists. |
| **Maestro** | **deferred** | Apache-2.0 JVM CLI. The cleanest test-3 pass on the list: Playwright **cannot** drive an APK or IPA, and the estate does ship one. Adopt the **native** driver only — its newer web-browser mode duplicates Playwright and is the weaker option. Note `docs/MULTIPLATFORM.md:35` already names it "Primary": that is an unexecuted plan, not a prior adoption. `--format junit` is mandatory; the default reporter is NOOP. |
| **dembrandt** | **deferred** | Design-token drift. No such question is being asked yet. |

## Adjacent (proxy / infra)

| Tool | Status | Notes |
|---|---|---|
| **zu1k/proxypool** | deferred | Proxy aggregator. Adopt when: a crawl target requires rotating egress IPs that the current single-source can't cycle. Standalone tool, no integration cost — invoke via subprocess. |
| **h3r2tic/shader-prepper** | reject | Mis-listed in original sweep — this is a shader compiler, not a crawler. |

## Awesome lists

| Tool | Status | Notes |
|---|---|---|
| **BruceDone/awesome-crawler** | reference-only | Meta-list. Useful one-time reading; do not link from CI. |

## Spider-py

| Tool | Status | Notes |
|---|---|---|
| **spider-rs/spider-py** | reject-as-dep | Python bindings to spider-rs. We don't run Python; if we adopt spider-rs, we use it from Rust. |

---

## Decision rules (apply when adding to this registry)

1. **Stack alignment**: candidates in Rust, Node/TS, or Go are evaluable.
   PHP / Java / Python tools are reference-only at most.
2. **Capability test**: does it open a use case Playwright + Crawlee
   can't already serve? If no → reject or reference-only.
3. **Maintenance posture**: any candidate without a commit in the last
   18 months → reject unless it's a pattern reference.
4. **PSA filter**: requires SaaS / phones home / ToS-violating → reject
   per [`PlausiDen-Meta/SUPERSOCIETY_BASELINE.md`](https://github.com/thepictishbeast/PlausiDen-Meta/blob/main/SUPERSOCIETY_BASELINE.md).
5. **Vendor binary, not source** when adopting: invoke through a thin
   adapter; don't fork the upstream tool unless absorbing under FOSS
   Absorption Protocol (12+ AVP-2 passes minimum per CLAUDE.md).

## Triggers for the deferred set

The following are deferred with explicit triggers:

| Tool | Trigger to adopt |
|---|---|
| spider-rs/spider | First use case demanding ≥1M-URL no-JS crawl. |
| 0x676e67/wreq | First crawl target that blocks headless Chromium TLS-fingerprint. |
| a11ywatch/crawler | Formal a11y journey support added to PlausiDen-Crawler scope. |
| zu1k/proxypool | First crawl target requiring egress-IP rotation. |
| Visual Regression Tracker | First need for baseline **history** across branches/viewports plus an approve/review workflow — as opposed to pass/fail against a checked-in baseline, which `pixel_diff.rs` already does. |
| Maestro | Native-mobile auditing formally enters scope: the Tempered Studio APK needs journey coverage **and** an Android emulator is available to run it (the emulator, not Maestro, is the real cost). |
| dembrandt | First design-token drift question. |

When a trigger fires, file an issue against this repo with the
[CONTRIBUTOR_CHECKLIST](https://github.com/thepictishbeast/PlausiDen-Meta/blob/main/CONTRIBUTOR_CHECKLIST.md)
workflow before integration.

## Refresh cadence

This registry is reviewed quarterly alongside the PlausiDen-Audits
TOOL_REGISTRY. Rejected items stay rejected unless new evidence arrives
(new maintainer, capability addition, PSA-respecting fork, etc.).
