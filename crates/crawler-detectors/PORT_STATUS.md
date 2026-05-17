# Crawler T75 — detector port status

**Date: 2026-05-17**

## TL;DR

**All TS detectors that have a Rust mirror target are ported.** The
`crawler-detectors` crate exports 45 modules, one per TS detector
source. The 2 remaining TS files (`runtimeImagesEndToEnd`,
`uiOverflowEndToEnd`) are end-to-end test harnesses, not detector
sources — explicitly out of scope for the port.

The remaining T75 work is the **chromiumoxide runtime adapter**
that lets the Rust crawler drive a browser end-to-end (currently
the production crawl path is still TS+Playwright).

## Port inventory (45 / 45 detector sources mirrored)

| TS source                  | Rust module                   |
|----------------------------|-------------------------------|
| autocomplete               | autocomplete                  |
| cacheControl               | cache_control                 |
| coep                       | coep                          |
| contentSecurityPolicy      | content_security_policy       |
| cookieSecurity             | cookie_security               |
| coop                       | coop                          |
| corp                       | corp                          |
| crossPageMetaDescription   | cross_page_meta_description   |
| crossPageTitle             | cross_page_title              |
| cssHealth                  | css_health                    |
| docTitle                   | doc_title                     |
| documentPolicy             | document_policy               |
| favicon                    | favicon                       |
| fontLoading                | font_loading                  |
| formLabels                 | form_labels                   |
| headingOrder               | heading_order                 |
| hstsHeader                 | hsts                          |
| htmlLang                   | html_lang                     |
| infoLeakHeaders            | info_leak_headers             |
| inlineScript               | inline_script                 |
| linkText                   | link_text                     |
| linkUnderline              | link_underline                |
| metaDescription            | meta_description              |
| mixedContent               | mixed_content                 |
| networkErrorLogging        | network_error_logging         |
| originAgentCluster         | origin_agent_cluster          |
| outboundLinks              | outbound_links                |
| permissionsPolicy          | permissions_policy            |
| placeholderText            | placeholder_text              |
| referrerPolicy             | referrer_policy               |
| reportingEndpoints         | reporting_endpoints           |
| runtimeContrast            | runtime_contrast              |
| runtimeFocus               | runtime_focus                 |
| runtimeImages              | runtime_images                |
| runtimeLandmarks           | runtime_landmarks             |
| skipLink                   | skip_link                     |
| speculationRules           | speculation_rules             |
| sri                        | sri                           |
| tapTargets                 | tap_targets                   |
| trustedTypesRuntime        | trusted_types_runtime         |
| uiOverflow                 | ui_overflow                   |
| varyHeader                 | vary_header                   |
| viewportMeta               | viewport_meta                 |
| webVitals                  | web_vitals                    |
| xFrameOptions              | x_frame_options               |

## Out of scope (explicit non-targets)

* `runtimeImagesEndToEnd.ts` — e2e test harness, runs the
  `runtime_images` detector against a fixture page. Not a detector
  source. Will be replaced by Rust integration tests once the
  chromiumoxide adapter lands.
* `uiOverflowEndToEnd.ts` — same pattern, paired with `ui_overflow`.
* All `*.test.ts` files — vitest unit tests, mirrored by `#[cfg(test)]
  mod tests` blocks inside each Rust detector module.
* Utility / aggregator files (`htmlReport`, `imageHash`,
  `responseHeaderDetector`, `scoreHistory`, `scoreWhitelist`,
  `selectorHealer`, `supersocietyBadge*`, `supersocietyScore`,
  `telemetry`, `visualDiff`, `forgeReplay`, `frequencyShift`,
  `noJsRender`, `fingerprint`, `drift`, `report*`, `aggregates`,
  `aria`, `audit`, `checkSkillshots`, `d0Audit`, `discover`,
  `main`, `journey`, `probe`, `stress`, `cssHealthEndToEnd`,
  `cssVarResolution`, `annotate*`) — pipeline plumbing or
  aggregators, not pure-detector ports. Each has its own porting
  decision separate from T75's detector scope.

## What's left for T75 to be ✅ resolved

1. **Chromiumoxide adapter scaffold** — typed `CrawlerRuntime` trait
   that the existing TS pipeline shape can target. Lives in a new
   crate (`crawler-runtime`) or as a sibling module here.
2. **Driver wiring** — replace `playwright.chromium.launch()` with
   `chromiumoxide::Browser::launch()`, route page lifecycle through
   the trait.
3. **Snapshot bridge** — each detector currently has its own
   per-detector DOM-capture JS const (preserved char-for-char from
   the TS source). The adapter needs to:
   - call `page.evaluate(capture_js)` per detector
   - deserialise the result into the detector's `*Snapshot` type
   - feed the snapshot into the detector's `detect_*_issues`
4. **Per-request capture** — sub-resource header detectors (`corp`,
   `cookie_security`, etc.) need the `allResponseHeaders` map fed
   from CDP events (Network.responseReceived). The TS path collects
   this via Playwright's `response` event listener.
5. **Replace main.ts** with a Rust binary that calls the trait.
6. **Integration tests** — the e2e harnesses (`runtimeImagesEndToEnd`,
   `uiOverflowEndToEnd`) get Rust equivalents.

## Estimated remaining work

* Adapter scaffold + trait: 1-2 cycles
* Driver wiring (chromiumoxide launch / page lifecycle): 2-3 cycles
* Per-detector capture bridge: 1 cycle per detector × ~10 batched =
  ~5 cycles (most detectors share the same evaluate-then-deserialise
  shape; can batch via a generic helper)
* Sub-resource Network event wiring: 2 cycles
* Replace main.ts: 3-4 cycles
* Integration test harnesses: 2 cycles

**Total: ~15-20 focused cycles**, achievable across several days
of cron-driven progress at 1 cycle per tick. The detector-port
foundation is done; the remaining work is pipeline-plumbing.

## Doctrine — why the per-detector DOM-capture JS is preserved verbatim

Each Rust detector module that wraps a runtime-DOM-walk classifier
exports a `pub const FOO_DOM_CAPTURE_JS: &str` containing the EXACT
template literal from the TS source. This is deliberate:

1. The future chromiumoxide path feeds `FOO_DOM_CAPTURE_JS` to
   `chromiumoxide::Page::evaluate(...)` and gets back the same
   snapshot shape Playwright's path produces today. Snapshot
   compatibility is a hard guarantee.
2. The Rust modules include a `js_brackets_balanced` test that
   catches accidental edits to the embedded JS that would silently
   break runtime evaluation.
3. When a detector's logic changes, the JS must change in BOTH
   places — the test pins the contract; PRs that update only one
   side get caught.
