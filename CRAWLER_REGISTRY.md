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

When a trigger fires, file an issue against this repo with the
[CONTRIBUTOR_CHECKLIST](https://github.com/thepictishbeast/PlausiDen-Meta/blob/main/CONTRIBUTOR_CHECKLIST.md)
workflow before integration.

## Refresh cadence

This registry is reviewed quarterly alongside the PlausiDen-Audits
TOOL_REGISTRY. Rejected items stay rejected unless new evidence arrives
(new maintainer, capability addition, PSA-respecting fork, etc.).
