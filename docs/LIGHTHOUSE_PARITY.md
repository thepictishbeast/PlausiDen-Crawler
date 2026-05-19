# Lighthouse Parity Audit

**Status:** parity audit. Maps every Lighthouse audit (Performance,
Accessibility, Best Practices, SEO, PWA) to a Crawler detector.
Identifies the gaps Crawler must close to fully replace Lighthouse.

**Method:** walked the Lighthouse audit list (v11.x category index),
matched each ID to a Crawler detector or existing Forge phase,
classified covered / partial / gap.

## Performance category

| Lighthouse audit             | Crawler axis / Forge phase                | Status |
|------------------------------|-------------------------------------------|--------|
| first-contentful-paint       | `web_vitals` (LCP-only today)             | partial |
| largest-contentful-paint     | `web_vitals`                              | covered |
| total-blocking-time          | `long_tasks`                              | covered |
| cumulative-layout-shift      | `web_vitals` (CLS captured)               | covered |
| speed-index                  | none                                      | gap |
| max-potential-fid            | none (replaced by INP in v11)             | obsolete |
| interaction-to-next-paint    | `web_vitals` (INP)                        | covered |
| server-response-time         | none — needs Network.responseReceived ts  | gap |
| first-meaningful-paint       | none (deprecated)                         | obsolete |
| render-blocking-resources    | none                                      | gap — highest-leverage gap |
| uses-rel-preconnect          | none                                      | gap |
| uses-rel-preload             | none                                      | gap |
| font-display                 | `font_loading`                            | covered |
| unused-css-rules             | none                                      | gap |
| unused-javascript            | none                                      | gap |
| modern-image-formats         | none — should check img.src for AVIF/WebP | gap |
| uses-optimized-images        | none                                      | gap |
| uses-text-compression        | `cache_control` (partial, via headers)    | partial |
| uses-responsive-images       | `image_dimensions` (size only)            | partial |
| efficient-animated-content   | none                                      | gap |
| duplicated-javascript        | none                                      | gap |
| legacy-javascript            | none                                      | gap |
| dom-size                     | `dom_size`                                | covered |
| critical-request-chains      | none                                      | gap |
| network-rtt                  | none                                      | gap |
| network-server-latency       | none                                      | gap |
| main-thread-tasks            | `long_tasks` (partial)                    | partial |
| diagnostics                  | n/a — meta-audit                          | n/a |

## Accessibility category

Covered by [PA11Y_WCAG_PARITY.md](./PA11Y_WCAG_PARITY.md) — same
WCAG 2.1 mapping; Lighthouse's a11y audits are a subset of Pa11y.

## Best Practices category

| Lighthouse audit                | Crawler axis / Forge phase           | Status |
|---------------------------------|--------------------------------------|--------|
| is-on-https                    | n/a — site-level config             | n/a |
| uses-http2                     | none                                 | gap |
| no-document-write              | `inline_script` (partial)            | partial |
| no-vulnerable-libraries        | `forge:sri` + cargo-audit at build  | partial |
| notification-on-start          | none                                 | gap (rare on static sites) |
| password-inputs-can-be-pasted  | none                                 | gap |
| image-aspect-ratio             | `image_dimensions`                   | covered |
| image-size-responsive          | none                                 | gap |
| preload-fonts                  | none                                 | gap |
| doctype                        | none                                 | gap |
| charset                        | none                                 | gap |
| no-unload-listeners            | none                                 | gap |
| geolocation-on-start           | `permissions_policy` (declarative)   | partial |
| inspector-issues               | `cspViolation` event (partial)       | partial |
| deprecations                   | none                                 | gap |
| third-party-cookies            | `cookie_security`                    | covered |
| valid-source-maps              | none                                 | gap |
| paste-preventing-inputs        | none                                 | gap |
| errors-in-console              | `Pageerror` event                    | covered |
| trust-types-xss                | `trusted_types_runtime`              | covered |

## SEO category

| Lighthouse audit               | Crawler axis / Forge phase            | Status |
|--------------------------------|---------------------------------------|--------|
| document-title                 | `doc_title`                           | covered |
| meta-description               | `meta_description`                    | covered |
| http-status-code               | `ResponseError` event                 | covered |
| link-text                      | `link_text`                           | covered |
| crawlable-anchors              | `link_text` (partial)                 | partial |
| is-crawlable                   | none — robots.txt audit               | gap |
| robots-txt                     | none                                  | gap |
| image-alt                      | gap (see PA11Y 1.1.1)                 | gap |
| hreflang                       | `hreflang`                            | covered |
| canonical                      | `canonical_url`                       | covered |
| structured-data                | `forge:structured_data` phase         | covered |
| viewport                       | `viewport_meta`                       | covered |
| font-size                      | none — minimum-body-font audit        | gap |
| tap-targets                    | `tap_targets`                         | covered |
| plugins                        | none (Flash etc.)                     | gap (legacy) |

## PWA category

| Lighthouse audit               | Crawler axis / Forge phase            | Status |
|--------------------------------|---------------------------------------|--------|
| installable-manifest           | `web_manifest`                        | covered |
| service-worker                 | none                                  | gap (rare for PlausiDen — strict CSP blocks most SW) |
| offline-start-url              | none                                  | gap |
| splash-screen                  | `web_manifest` (icons only)           | partial |
| themed-omnibox                 | `web_manifest` (theme_color)          | partial |
| content-width                  | `viewport_meta` (initial-scale)       | covered |
| apple-touch-icon               | `favicon` (partial)                   | partial |
| maskable-icon                  | `web_manifest`                        | partial |

## Summary

* **Performance:** 6 of 28 covered, 5 partial, 17 gaps.
  Render-blocking-resources, unused CSS/JS, modern-image-formats are
  the highest-leverage gaps (marketing-site biggest perf wins).
* **Best Practices:** 4 of 20 covered, 4 partial, 12 gaps. Most are
  edge-case (notification-on-start, password-paste) but doctype,
  charset, image-size-responsive are common real issues.
* **SEO:** 8 of 15 covered, 1 partial, 6 gaps. robots-txt audit and
  font-size audit are common-issue gaps.
* **PWA:** 1 of 8 covered, 5 partial, 2 hard gaps. Service-worker
  and offline-start-url are skip-by-design for the supersociety
  / strict-CSP doctrine.

## Highest-priority gaps to close

Marketing pages most often fail these — each is a one-detector add:

1. **render-blocking-resources** — scan `<link rel="stylesheet">` in
   `<head>` without `media="print"` swap; scan `<script>` in `<head>`
   without `defer` / `async` / `type="module"`. Easy heuristic.
2. **modern-image-formats** — walk `<img src>` and flag any `.jpg` /
   `.png` over 50 KB that doesn't have a `<picture><source
   type="image/avif">` sibling. Lighthouse's "uses-optimized-images"
   bundles into this.
3. **doctype + charset** — single-shot per-page: `<!doctype html>`
   first line + `<meta charset="utf-8">` in first 1024 bytes.
4. **font-size SEO** — sample 100 text nodes, compute computed font-
   size, flag if median < 12px (mobile readability gate).
5. **unused-css-rules** — heavier; needs CDP CSS coverage API. Drop
   in for later iterations.
6. **robots-txt audit** — fetch /robots.txt, parse, flag missing or
   wildcard-blocked-canonical errors.

## Out of scope for Crawler

* **service-worker / offline-start-url** — PlausiDen sites
  deliberately don't ship service workers (CSP-strict + Tor mode +
  reader_safety phase). Not a gap; out of scope.
* **uses-http2** — site-level config, not a per-page concern.
* **legacy-javascript** — PlausiDen sites ship zero JS by default
  (except the THEME_TOGGLE_JS + ERUDA_LOADER_JS). Not applicable.
* **paste-preventing-inputs** — Loom forms don't prevent paste; if
  one ever does, the typed-CmsForm enum can refuse the prop.
  Substrate-level guarantee, not a runtime check.

## Maintenance

Point-in-time audit (Crawler at commit f20361e, Lighthouse v11.x).
Re-audit on every Crawler detector addition or Lighthouse version
bump. Cross-reference with PA11Y_WCAG_PARITY.md — the accessibility
overlap is intentional duplication so each doc reads standalone.
