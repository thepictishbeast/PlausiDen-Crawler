# Crawler UX/UI detector backlog (T76)

> Owner directive 2026-05-14: "right now we need something
> extremely capable and that works well finding UX and UI
> problems and issues."

This is the prioritised backlog of detectors to add to the
current TS Playwright runner. The Rust rewrite (T75) must
reach parity with everything here.

Current detectors (shipped):

- placeholder_text
- link_text_quality
- console_error_class
- csp_violation
- network_failure
- slow_lcp / slow_inp
- axe_a11y_violation (axe-core integration)
- broken_image
- mixed_content
- runtime_contrast
- runtime_focus
- runtime_landmarks
- runtime_images
- heading_order
- ui_overflow
- web_vitals
- css_health

## P0 — ship next (block deploys when failing)

| Detector | Spec / Why |
|---|---|
| **tap_target_size** | WCAG 2.5.5 — every interactive must be ≥ 44×44 CSS px (or 24×24 with adequate spacing per 2.5.8). Mobile-first sites fail this constantly. |
| **focus_indicator_visible** | WCAG 2.4.7 — on `:focus-visible`, the contrast ratio of the focus ring vs the surrounding ≥ 3:1 OR a minimum 2px outline offset. Common regression: outline removed by reset stylesheets. |
| **placeholder_as_label** | Anti-pattern — `<input placeholder="Email">` with no `<label>` fails screen readers + disappears on input. Detect: every input must have an associated label OR aria-labelledby. |
| **form_field_unlabelled** | WCAG 1.3.1 + 4.1.2 — every input/select/textarea has a name (label, aria-label, aria-labelledby, or title). |
| **form_required_marked** | Required fields must be programmatically marked (`required` attribute OR `aria-required="true"`). Asterisk-only is not enough. |
| **error_message_proximity** | When a form field is invalid, the error must be associated (`aria-describedby` or DOM-adjacent). |
| **color_only_information** | Detect text where the only visual indicator is colour. E.g., `<span style="color:red">Required</span>` with no other cue. Simulates colour-blindness via canvas + heuristic. |
| **autoplay_media** | `<video autoplay>` / `<audio autoplay>` without `muted` violates user agency. |
| **animation_respects_reduced_motion** | If `prefers-reduced-motion: reduce` is set, no animation > 5s + no parallax + no auto-rotating carousel. Set the media query in the runner, replay, diff. |
| **tab_order_logical** | Tabbing through interactive elements should follow visual order. Detect: tab through every focusable element; assert the tab sequence matches DOM order modulo `tabindex`. |
| **off_screen_focusable** | Any element with `tabindex >= 0` that's positioned off-screen with no `aria-hidden` is a keyboard trap candidate. |
| **hover_without_keyboard** | Detect `:hover`-revealed UI that has no `:focus`/`:focus-within` equivalent. Disclosed menus, tooltips, etc. |
| **viewport_meta_correct** | `<meta name="viewport" content="width=device-width, initial-scale=1">` present + not `user-scalable=no` (a11y violation). |
| **html_lang_attr** | `<html lang>` present + ISO 639-1 valid (per ISO directive 2026-05-13). |
| **page_title_unique_and_present** | Every page has a `<title>` of length 1-60 chars; titles unique across the journey. |
| **h1_present_and_unique** | Every page has exactly one `<h1>`. |
| **heading_skip_no_levels** | Heading sequence h1→h2→h3 (no h1→h3). |

## P1 — ship soon (warn-severity)

| Detector | Spec / Why |
|---|---|
| **layout_shift_attribution** | When CLS > threshold, report which element shifted (CDP `LayoutShift` events name the source). |
| **font_loading_strategy** | Detect FOIT (flash-of-invisible-text) via `font-display` audit. Recommend `swap` or `optional`. |
| **page_weight_budget** | Total bytes / image bytes / JS bytes / CSS bytes vs per-route budget in journey JSON. |
| **third_party_request_count** | Count distinct third-party origins; warn over threshold. Privacy-positive. |
| **cookie_set_count** | Count cookies set on first paint (zero-state-respecting sites set zero). |
| **localstorage_set_count** | Same shape as cookies. |
| **inline_style_attr_count** | Mirrors Forge `phase_html_semantic` runtime — every inline `style="..."` is layout drift. |
| **div_role_landmark** | Mirrors Forge T67 — `<div role="banner|main|...">` should be `<header>` / `<main>`. |
| **excessive_scroll_depth** | Detect pages requiring > N viewports of scroll to reach primary CTA. |
| **z_index_stacking_conflict** | Detect overlapping interactive elements with conflicting z-index. |
| **print_stylesheet_present_or_absent_loud** | If no `@media print` rules, the page can't be printed cleanly. Flag as info. |
| **favicon_present** | Most sites have one; absence is a polish miss. |
| **app_icon_pwa_completeness** | If `manifest.json` exists, check icons cover required sizes. |
| **og_card_completeness** | Open Graph fields for share-card rendering. |
| **twitter_card_completeness** | Same shape for Twitter cards. |
| **canonical_link_present** | SEO + dedupe across www/non-www. |
| **robots_txt_consistent** | Every URL the journey reaches is allowed by robots.txt OR explicitly disallowed (no surprises). |
| **security_headers_present** | CSP / HSTS / X-Content-Type-Options / Referrer-Policy / Permissions-Policy. Cross-cut with Forge `phase_csp`. |

## P2 — ship eventually (info-severity)

| Detector | Spec / Why |
|---|---|
| **memory_leak_across_journey** | Heap growth across N steps > threshold. |
| **event_listener_leak** | Event-listener count grows monotonically. |
| **detached_dom_node_growth** | DOM nodes referenced from JS but no longer in the tree. |
| **fragile_selector_in_journey** | Journey uses CSS selector that's likely to break (e.g., `nth-child(7)` inside a list, `.x > .y > .z` deep nesting). |
| **service_worker_misbehaviour** | SW caches stale JS, refuses to update, etc. |
| **client_side_routing_smoke** | After a SPA route change, axe-core re-runs cleanly. |
| **input_type_mismatch** | `<input type="text">` for an email field; recommend `type="email"`. |
| **autocomplete_attr_present** | `autocomplete="email"` etc. for known field types. |
| **form_action_present** | Forms have a server-side action even if JS-handled (graceful degradation). |
| **iframe_title_present** | Every `<iframe>` has a `title` attribute (a11y). |
| **iframe_allow_attr_minimal** | `<iframe allow>` doesn't grant unneeded capabilities. |
| **long_task_attribution** | Long tasks > 50ms attributed to source script. |
| **render_blocking_resource_count** | Render-blocking CSS/JS count + size. |
| **unused_css_estimate** | Per Lighthouse — unused CSS bytes > threshold. |
| **unused_js_estimate** | Same for JS. |

## P3 — adversarial / supersociety

| Detector | Spec / Why |
|---|---|
| **fingerprintability_score** | Combine canvas fingerprint, audio context, font enumeration, screen size — produce a 0-1 fingerprintability score. |
| **third_party_tracker_blocklist** | Every third-party request matched against EasyList / EasyPrivacy / Disconnect. |
| **tor_onion_compatibility** | The site loads cleanly in Tor Browser default-mode (NoScript-friendly fallback path). |
| **no_javascript_fallback_works** | With JS disabled, the site still serves meaningful content (zero-JS doctrine cross-cut with Loom). |
| **mixed_third_party_origins** | Distinct script-src / img-src / media-src / connect-src origins enumerated. |
| **subresource_integrity_coverage** | Every external script/stylesheet has SRI. Cross-cut with Forge `phase_sri`. |
| **referrer_policy_strict** | Referrer-Policy is `no-referrer` or `same-origin` (privacy-respecting). |

## Cross-cutting

- **Detector parity across backends** — every detector here
  must run identically on TS Playwright + Rust chromiumoxide
  (T75A) backends. Detector parity tests: run identical
  journey, diff findings.
- **Severity ladder** — every detector has a severity
  (`info`/`warn`/`strict`/`block`). P0 = strict, P1 = warn,
  P2-P3 = info by default.
- **Journey-config override** — every detector accepts a
  per-journey budget + enable/disable toggle.
- **Documentation** — every detector ships with a per-detector
  doc describing when it fires + how to fix.
- **Property-based fuzz** — every detector that consumes DOM /
  network / console gets a proptest target proving it never
  panics on arbitrary input.
- **Crawler `Finding` schema** — already typed; new detectors
  just add new `class` enum variants.

## Sequence

1. Write the P0 detectors first (block-severity by default).
   Estimated effort: ~2 weeks of focused work, ~1 detector per
   day with tests.
2. P1 next (warn-severity). Estimated ~3 weeks.
3. P2-P3 over the following 2-3 months.
4. T75A (Rust port) develops in parallel; every detector
   added in TS gets a port-task in the Rust backend backlog.
