# PlausiDen-Crawler — Detector Reference

Authoritative catalog of every detector axis the crawler emits, with
the WCAG / standards reference each maps to and the canonical
finding kinds + severities. Maintained alongside the code: a new
detector landing without an entry here is incomplete.

Doctrine: every detector has a **TS implementation** under `src/`
that runs in the live Playwright crawl, AND a **Rust mirror** under
`crates/crawler-detectors/src/` that the future chromiumoxide port
(T75) will consume from a single source of truth. The JS string
embedded in each Rust module is byte-equivalent to the TS file's
`page.evaluate` source — see the `js_brackets_balanced` test in each
Rust mirror, which catches truncation regressions.

Severity model:

- **strict** — gate-blocking. Any new strict finding fails the
  crawler's pass/fail gate and exits non-zero. Reserve for
  unconditionally broken behaviour.
- **warn** — within budget. Surfaces in the report but does not
  block. Reserve for guidance / recommendations / per-context
  trade-offs.

---

## Detector axes

### `cssHealth` — CSS load + parse health
Source: `src/cssHealth.ts` · Rust: `css_health.rs`

| Finding | Sev | Catches |
|---|---|---|
| `css.no-stylesheets-declared` | strict | Page `<link rel=stylesheet>` count is zero — almost certainly a deploy mistake. |
| `css.all-sheets-failed-network` | strict | Every linked stylesheet 4xx/5xx'd or timed out. Page renders unstyled. |
| `css.some-sheets-failed-network` | warn | At least one stylesheet failed to load. Partial styling. |
| `css.empty-or-tiny-body` | strict | `<body>` has < 50 chars of computed text content — a blank or near-blank page. |
| `css.wrong-mime` | warn | Stylesheet returned with `text/html` or another non-CSS mime. |
| `css.served-but-not-applied` | warn | Sheet downloaded but `document.styleSheets` shows zero rules from it. |
| `css.applied-rule-count-anomaly` | warn | Applied rule count is suspiciously low for the byte size — usually a parse failure. |
| `css.brace-vs-applied-mismatch` | warn | The downloaded text has many `{ }` blocks but few rules applied — parser stopped early. |
| `css.brace-imbalance` | warn | `{ }` brace count mismatch in the source — corrupt or truncated. |

### `uiOverflow` — viewport overflow / clipping
Source: `src/uiOverflow.ts` · Rust: `ui_overflow.rs`

| Finding | Sev | Catches |
|---|---|---|
| `overflow.page-horizontal-scroll` | strict | `documentElement.scrollWidth > clientWidth + 2`. Whole-page horizontal scrollbar at the configured viewport. |
| `overflow.element-bleeds-viewport` | strict | At least one positioned element has `rect.right > viewport + 4` AND visible content. |
| `overflow.text-clipped` | strict | Element has `scrollWidth > clientWidth + 2` AND `overflow-x` is not `auto`/`scroll` — content is genuinely unreachable. |

> NOTE: `overflow.tap-target-too-small` was retired 2026-05-14 (T76).
> The new `tapTargets` axis below is the canonical replacement.

### `runtimeContrast` — WCAG contrast at runtime
Source: `src/runtimeContrast.ts` · Rust: `runtime_contrast.rs`

| Finding | Sev | Catches |
|---|---|---|
| `contrast.text-below-aa` | strict | Visible text node has effective fg/bg contrast ratio below 4.5:1 (3:1 for large text). |
| `contrast.large-text-below-aa` | warn | Large text below 3:1. |

Walks the parent chain to compute effective background; bails on
gradient backgrounds (false-positive avoidance).

### `runtimeImages` — image load + a11y health
Source: `src/runtimeImages.ts` · Rust: `runtime_images.rs`

| Finding | Sev | Catches |
|---|---|---|
| `image.broken` | strict | `<img>` failed to load (`naturalWidth === 0` after settle). |
| `image.empty-src` | strict | `<img src="">` — empty/whitespace src attribute. |
| `image.missing-alt` | strict | `<img>` with no `alt` attribute (not even empty). WCAG 1.1.1. |
| `image.cls-risk` | warn | Image with no `width`/`height` attributes and no aspect-ratio CSS — guaranteed CLS. |

### `runtimeFocus` — focus indicator visibility
Source: `src/runtimeFocus.ts` · Rust: `runtime_focus.rs`

| Finding | Sev | Catches |
|---|---|---|
| `focus.invisible-indicator` | strict | Focusing the element produces no visible change in outline/box-shadow/border. WCAG 2.4.7. |

### `runtimeLandmarks` — semantic landmarks
Source: `src/runtimeLandmarks.ts`

Captures presence of `<header>`, `<nav>`, `<main>`, `<aside>`,
`<footer>` and their ARIA equivalents. Strict on a missing
`<main>` — screen-reader users rely on it for skip-to-content.

### `headingOrder` — heading hierarchy
Source: `src/headingOrder.ts` · Rust: `heading_order.rs`

| Finding | Sev | Catches |
|---|---|---|
| `heading.no-h1` | strict | Page has zero `<h1>`. |
| `heading.multiple-h1` | warn | Page has > 1 `<h1>`. |
| `heading.skip` | warn | A heading skips a level (h2 → h4). |

### `linkText` — link purpose
Source: `src/linkText.ts` · Rust: `link_text.rs`

| Finding | Sev | Catches |
|---|---|---|
| `link.empty-text` | strict | Visible `<a>` with no accessible name (no text, aria-label, aria-labelledby, title). |
| `link.generic-text` | warn | Link text is "click here" / "more" / "read more" / etc. WCAG 2.4.4. |

### `placeholderText` — Lorem ipsum + dev markers
Source: `src/placeholderText.ts`

| Finding | Sev | Catches |
|---|---|---|
| `placeholder.lorem-ipsum` | strict | Latin filler in rendered DOM. |
| `placeholder.template-marker` | strict | `TODO` / `FIXME` / `XXX` / `HACK` / `{{var}}` / `<%= %>` etc. in DOM. |
| `placeholder.coming-soon` | warn | "Coming soon" / "TBD" / "Under construction". |

### `webVitals` — Core Web Vitals
Source: `src/webVitals.ts` · Rust: `web_vitals.rs`

| Finding | Sev | Catches |
|---|---|---|
| `webvitals.lcp-poor` | strict | Largest Contentful Paint > 4s. |
| `webvitals.lcp-needs-improvement` | warn | LCP 2.5–4s. |
| `webvitals.cls-poor` | strict | Cumulative Layout Shift > 0.25. |
| `webvitals.cls-needs-improvement` | warn | CLS 0.1–0.25. |
| `webvitals.inp-poor` | strict | Interaction-to-Next-Paint > 500ms. |
| `webvitals.inp-needs-improvement` | warn | INP 200–500ms. |

### `tapTargets` — touch target size *(T76 — added 2026-05-14)*
Source: `src/tapTargets.ts` · Rust: `tap_targets.rs`

| Finding | Sev | Catches |
|---|---|---|
| `tap.too-small` | strict | Interactive target with `min(width, height) < 24` CSS px. WCAG 2.2 SC 2.5.8 AA. |
| `tap.below-recommended` | warn | Target with `min(width, height)` 24–43 px. WCAG 2.1 SC 2.5.5 AAA + Apple HIG / Material 44px. |

Honors the WCAG 2.5.8 inline-in-sentence exception (a small inline
link mid-paragraph isn't flagged) and the project's
`data-tap="compact"` opt-out (matches `uiOverflow`'s prior doctrine).

Selector set: `a[href]`, `button`, `input[type=button|submit|reset|checkbox|radio|image|file]`, `select`, `summary`, `[role=button|link|checkbox|radio|menuitem|tab|switch]`, `[onclick]`.

### `formLabels` — form-control labelling *(T76 — added 2026-05-14)*
Source: `src/formLabels.ts` · Rust: `form_labels.rs`

| Finding | Sev | Catches |
|---|---|---|
| `form.no-label` | strict | No accessible name at all (no `<label>`, `aria-label`, `aria-labelledby`, `title`, OR `placeholder`). WCAG 1.3.1 + 4.1.2. |
| `form.placeholder-only-label` | warn | Placeholder is the ONLY label. WCAG 3.3.2. |
| `form.required-no-indicator` | warn | `required` / `aria-required="true"` set but no `*` or "required" in the visible label. |

### `crossPageTitle` — duplicate page titles across a journey *(T76 aggregates-layer — added 2026-05-14)*
Source: `src/crossPageTitle.ts`

| Finding | Sev | Catches |
|---|---|---|
| `title.cross-page-dup` | warn | Two or more pages in the journey share the same trimmed `<title>`. SEO suffers (Google filters duplicate-title results) and users can't tell open tabs / bookmarks / history entries apart. |

**First aggregates-layer detector.** Unlike per-page detectors (which run on each goto with a single page's snapshot), this one accumulates per-page titles during the journey and emits findings ONCE at the end. The pattern:

1. `newCrossPageTitleAccumulator()` — empty accumulator, created in main.ts before the goto loop.
2. `recordPageTitle(acc, url, title)` — called per-goto, piggybacked off the docTitle capture so no extra `page.evaluate` cost.
3. `detectCrossPageTitleDuplicates(acc)` — called once after the loop completes. Returns findings.

No Rust mirror yet — the aggregates layer is conceptually different from the per-page-snapshot shape the `crawler-detectors` Rust crate is built around. A future `crawler-aggregates` crate would be the right home if Rust parity becomes load-bearing for the chromiumoxide port (T75).

### `crossPageMetaDescription` — duplicate page descriptions across a journey *(T76 aggregates-layer — added 2026-05-14)*
Source: `src/crossPageMetaDescription.ts`

| Finding | Sev | Catches |
|---|---|---|
| `meta-description.cross-page-dup` | warn | Two or more pages share the same trimmed `<meta name="description">`. Google explicitly filters duplicate descriptions in search results; social-share preview cards collapse into a single tile. |

Sister to `crossPageTitle` — same accumulator+record+detect pattern, piggybacked off the existing per-page `metaDescription` capture. Validates that the aggregates-layer pattern generalises cleanly. If a third aggregates detector lands, the current ~80% structural overlap is a candidate for a generic `dupGroupDetector(accumulator, kind, label)` helper.

### `fontLoading` — `@font-face` font-display strategy *(T76 — added 2026-05-14)*
Source: `src/fontLoading.ts`

| Finding | Sev | Catches |
|---|---|---|
| `font-loading.no-display` | warn | `@font-face` rule with no `font-display` declaration. Browser default is `block` → text renders blank until font loads (Flash of Invisible Text / FOIT). |
| `font-loading.display-block` | warn | `@font-face` rule explicitly sets `font-display: block` or `auto`. Same FOIT effect. |

Walks `document.styleSheets` for every `CSSFontFaceRule` (type 5). Cross-origin sheets the browser refuses to expose (`SecurityError` on `cssRules` access) are counted in `evidence.inaccessibleSheetCount` so the audit reader knows where the detector's coverage ends.

Acceptable values: `swap` (recommended), `fallback`, `optional`. Any other value (or absence) is a finding. Unknown values (typos, future tokens) are treated as `no-display`.

### `linkUnderline` — link distinguishability *(T76 — added 2026-05-14)*
Source: `src/linkUnderline.ts` · Rust: `link_underline.rs`

| Finding | Sev | Catches |
|---|---|---|
| `link.color-only-distinction` | warn | Inline link inside running text (`<p>` / `<li>` / `<dd>` / `<blockquote>` / `<td>` / `<th>`) where the only visual cue distinguishing it from surrounding text is colour. WCAG 1.4.1 Level A. |

Out of scope (NOT flagged):
* Block-level links (nav items, button-like CTAs, card links).
* Links inside `<header>` / `<nav>` / `<footer>` / `<aside>` chrome — they're conventionally button-styled.
* Links with explicit visual distinction: underline (canonical), bold weight (≥200 unit difference vs parent), border, outline (with style != none), different background, box-shadow, italic, icon child (svg / img / `i.icon` / `[class*="icon"]`).

**Detector ordering note**: linkUnderline runs FIRST in the per-goto detector chain. Other detectors (focus simulation, contrast walks) can transiently mutate computed styles; capturing pristine state avoids false negatives.

### `hsts` — Strict-Transport-Security response header *(T76 — added 2026-05-14)*
Source: `src/hstsHeader.ts`

| Finding | Sev | Catches |
|---|---|---|
| `hsts.missing` | strict | https page response carries no `Strict-Transport-Security` header (or unparseable). First-hit users on a clean browser remain MITM-vulnerable before the https redirect. |
| `hsts.max-age-too-short` | warn | Header present but `max-age` < 6 months (15552000s). Protection lapses if the user doesn't return within the window. |
| `hsts.no-subdomains` | warn | Adequate `max-age` but missing `includeSubDomains`. Subdomain takeovers can serve `http://attacker.example.com`. |

Out of scope: http pages (HSTS doesn't apply), localhost / 127.0.0.1 / `*.localhost` (browsers don't honour HSTS on loopback).

**First response-header detector.** Reads from main.ts's `topLevelResponseHeaders: Map<url, headers>` accumulator, populated by the existing `page.on('response')` listener for any response where `request().isNavigationRequest()`. Future header-flavoured detectors (xFrameOptions, referrerPolicy, contentSecurityPolicy strict mode) read from the same Map — no new listener needed per detector.

### `referrerPolicy` — Referrer-Policy response header *(T76 — added 2026-05-14)*
Source: `src/referrerPolicy.ts`

| Finding | Sev | Catches |
|---|---|---|
| `referrer-policy.missing` | warn | No `Referrer-Policy` header. Modern browsers fall back to a safe default but older clients may leak the full URL + query string to every third-party fetch. |
| `referrer-policy.permissive` | strict | Policy explicitly set to `unsafe-url`, `no-referrer-when-downgrade`, or `origin-when-cross-origin` — all leak more than the modern default. |
| `referrer-policy.invalid` | warn | Policy is set to a token not in the W3C set. Browsers fall back to default; intent is lost. |

Multi-token policies are honored per the W3C spec — the LAST recognised token wins (allowing safe defaults with permissive overrides). The detector classifies based on which recognised token is most prominent.

**Third response-header detector.** Reads from the same `topLevelResponseHeaders` Map as hsts + xFrameOptions. With three concrete examples now in hand, the ~70% structural overlap is a candidate for a generic `headerDetector(headerName, parser, classifier)` helper — extract on the next addition.

### `cookieSecurity` — Set-Cookie attribute audit *(T76 — added 2026-05-14)*
Source: `src/cookieSecurity.ts`

| Finding | Sev | Catches |
|---|---|---|
| `cookie.no-secure` | strict | https-page response sets a cookie without `Secure`. The cookie can leak over an http downgrade (MITM, mixed content, network rewrite). Add `; Secure`. |
| `cookie.samesite-none-no-secure` | strict | Cookie carries `SameSite=None` without `Secure`. Browsers REJECT this combination — the cookie is silently discarded. Add `Secure` or change to `Lax`. |
| `cookie.no-samesite` | warn | Cookie omits `SameSite`. Modern browsers default to `Lax` (safe); older clients leave the cookie unrestricted, exposing the site to CSRF. Set `SameSite=Strict` or `Lax`. |
| `cookie.session-no-httponly` | warn | Cookie name matches `/sess|sid|auth|token|jwt|bearer/i` AND lacks `HttpOnly`. JS — including injected XSS — can read it via `document.cookie`. Add `HttpOnly`. |

Out of scope: localhost / 127.0.0.1 / `*.localhost` (same exemption family as hsts/xFrameOptions). On http pages the `cookie.no-secure` check is suppressed because Secure can't apply, but `cookie.no-samesite` and `cookie.session-no-httponly` still fire.

Multiple `Set-Cookie` headers per response are supported — Playwright's `allHeaders()` joins them with `\n`, the parser splits on that boundary. Header names and attribute names are matched case-insensitively (`set-cookie` / `Set-Cookie` / `SECURE` / `SameSite=Lax` all work).

**Fourth response-header detector — and the trigger for a deferred refactor.** Reads from the same `topLevelResponseHeaders` Map as hsts + xFrameOptions + referrerPolicy, BUT the per-cookie shape (one response can carry many `Set-Cookie` lines, each with its own attribute set) is genuinely different from the per-header shape the previous three share. A naive `headerDetector(headerName, parser, classifier)` helper would shoe-horn the mismatch — pattern-extraction stays deferred, with a sibling-cycle action item to design a `multiValueHeaderDetector` variant that handles repeat-header semantics first-class.

**Capture-layer fix landed alongside this detector.** Playwright's synchronous `response.headers()` strips `Set-Cookie` (verified empirically against the HTTPS fixture 2026-05-14). The accumulator was switched to `await response.allHeaders()`, which returns the full set including `set-cookie` (lowercase-keyed). Backward-compat for hsts/xframe/referrer is preserved because both forms return lowercase keys for the headers they care about.

### `xFrameOptions` — clickjacking-defence response header *(T76 — added 2026-05-14)*
Source: `src/xFrameOptions.ts`

| Finding | Sev | Catches |
|---|---|---|
| `frame-options.missing` | strict | https page has neither `X-Frame-Options` header NOR a `Content-Security-Policy: frame-ancestors` directive. Any origin can iframe → clickjacking attacks. |
| `frame-options.allowall` | warn | `Content-Security-Policy: frame-ancestors *` (or `X-Frame-Options: ALLOW-FROM *`). Effectively no protection — surface so the operator can confirm the open-embed is intentional. |
| `frame-options.invalid` | warn | `X-Frame-Options` set to a value other than `DENY` / `SAMEORIGIN` / `ALLOW-FROM <uri>`. Browsers ignore unrecognised values. |

CSP `frame-ancestors` supersedes `X-Frame-Options` when both are present. Either one with a non-wildcard value protects the page; the detector requires at least one. Localhost + http pages exempt (same exemptions as hsts).

**Second response-header detector.** Reads from the same `topLevelResponseHeaders` Map as `hsts` — no new capture path needed. Validates that the response-header pattern generalises with the same single-listener-many-detectors design.

### `mixedContent` — HTTPS-page-loads-HTTP-resource *(T76 — added 2026-05-14)*
Source: `src/mixedContent.ts` · Rust: `mixed_content.rs`

| Finding | Sev | Catches |
|---|---|---|
| `mixed-content.active` | strict | `<script>`, `<link rel=stylesheet>`, `<link rel=preload>`, `<iframe>`, `<embed>`, `<object>` with http:// URL on an https page. Browsers BLOCK these. |
| `mixed-content.passive` | warn | `<img>`, `<audio>`, `<video>`, `<source>`, `<picture>`, srcset, video poster with http:// URL on an https page. Browsers may auto-upgrade or block. |
| `mixed-content.form-action` | strict | `<form action="http://…">` on an https page — credentials/PII over the wire in the clear. |

The detector short-circuits when the page itself is http — mixed-content concept doesn't apply. Static markup analysis catches the bug even when browsers silently auto-upgrade (which they do inconsistently).

**Note:** This axis has no fixture route in `t76-detector-fixtures` — the fixture server runs on HTTP, so the page-is-https short-circuit fires and no findings can be observed via the gate. Coverage is via the 17 TS+Rust unit tests. Future: HTTPS fixture variant for full integration.

### `favicon` — page favicon link *(T76 — added 2026-05-14)*
Source: `src/favicon.ts` · Rust: `favicon.rs`

| Finding | Sev | Catches |
|---|---|---|
| `favicon.missing-link` | warn | No `<link rel="icon">` / `shortcut icon` / `apple-touch-icon` / `mask-icon` in head. Browsers fall back to fetching `/favicon.ico`; if that 404s, browser tabs show a generic glyph. |

The companion finding "broken favicon URL" is intentionally NOT in this detector — the existing `failed-requests` axis catches any /favicon.ico 404 when the browser auto-fetches.

### `metaDescription` — page `<meta name="description">` *(T76 — added 2026-05-14)*
Source: `src/metaDescription.ts` · Rust: `meta_description.rs`

| Finding | Sev | Catches |
|---|---|---|
| `meta-description.missing` | warn | No `<meta name="description">` in head. Search engines synthesize one from page text (poorly). |
| `meta-description.empty` | warn | Tag present but content empty/whitespace. |
| `meta-description.too-short` | warn | Trimmed content < 50 chars. No useful preview to show. |
| `meta-description.too-long` | warn | Trimmed content > 160 chars. Search engines truncate; tail invisible. |

All warn — missing description doesn't break the page; just suboptimizes discovery + previews.

### `viewportMeta` — viewport meta tag *(T76 — added 2026-05-14)*
Source: `src/viewportMeta.ts` · Rust: `viewport_meta.rs`

| Finding | Sev | Catches |
|---|---|---|
| `viewport.missing` | strict | No `<meta name="viewport">` in `<head>`. WCAG 1.4.10. |
| `viewport.no-device-width` | strict | Tag present but content lacks `width=device-width`. |
| `viewport.zoom-disabled` | strict | `user-scalable=no/0` OR `maximum-scale ≤ 1`. WCAG 1.4.4 AA. |

### `docTitle` — document title quality *(T76 — added 2026-05-14)*
Source: `src/docTitle.ts` · Rust: `doc_title.rs`

| Finding | Sev | Catches |
|---|---|---|
| `title.missing` | strict | No `<title>` in `<head>`. Screen readers announce "untitled document". |
| `title.empty` | strict | `<title></title>` or whitespace-only. Same effect as missing. |
| `title.generic` | warn | Word/IDE leftovers: "Document", "Untitled", "Untitled Document", "New Page", etc. (whole-string match, case-insensitive) |
| `title.too-short` | warn | ≤ 2 characters after trimming. |
| `title.too-long` | warn | ≥ 70 characters — search engines truncate. |

### `htmlLang` — `<html lang>` attribute *(T76 — added 2026-05-14)*
Source: `src/htmlLang.ts` · Rust: `html_lang.rs`

| Finding | Sev | Catches |
|---|---|---|
| `lang.missing` | strict | `<html>` has no `lang` attribute. WCAG 3.1.1 Level A. |
| `lang.empty` | strict | `<html lang="">` — same effect as missing. |
| `lang.invalid` | warn | Value doesn't structurally match BCP-47 (underscores, whitespace, wrong-length primary, doubled hyphens). |
| `lang.unknown-primary` | warn | Primary subtag isn't in the common ISO 639-1 set (catches typos like `engish`). |

### `skipLink` — "skip to content" link *(T76 — added 2026-05-14)*
Source: `src/skipLink.ts` · Rust: `skip_link.rs`

| Finding | Sev | Catches |
|---|---|---|
| `skip.missing` | warn | No skip link found. WCAG 2.4.1 Level A; warn (not strict) since landmark navigation provides a partial bypass for screen readers. |
| `skip.broken-target` | strict | Skip link href points at a non-existent id — pressing Enter does nothing. |
| `skip.permanently-hidden` | strict | Skip link is `display:none` / `visibility:hidden` — can never be focused. |
| `skip.not-first-focusable` | warn | Skip link exists but isn't the first focusable element on the page. WCAG technique G1. |

Heuristic for "what counts as a skip link": text matches `/skip/i` or `/jump.{0,4}content/i`, OR class contains `skip`, OR is one of the first 3 anchors with href targeting a `<main>` / `id="main"` / `id="content"` element.

### `autocomplete` — form autocomplete attribute hints *(T76 — added 2026-05-14)*
Source: `src/autocomplete.ts` · Rust: `autocomplete.rs`

| Finding | Sev | Catches |
|---|---|---|
| `autocomplete.missing-credentials` | strict | Credential field (email / password / username / login) without `autocomplete=` attribute. Password managers can't save/fill. WCAG 1.3.5 AA. |
| `autocomplete.missing-pii` | warn | PII field (name / phone / address / postal-code / DOB / credit-card) without `autocomplete=`. Slower form-fill, higher abandonment. |
| `autocomplete.invalid-token` | warn | Attribute value isn't `on`/`off`/a WHATWG token. Browser ignores it. |

Multi-token (`shipping street-address`) and section-prefixed (`section-billing cc-number`) values are accepted. Field type takes precedence over name/id heuristics: `type=email` is always credential; `type=tel` is always PII.

### `outboundLinks` — outbound-link safety *(T76 — added 2026-05-14)*
Source: `src/outboundLinks.ts` · Rust: `outbound_links.rs`

SECURITY-flavoured detector. Defence-in-depth against tabnabbing
(modern browsers default `target="_blank"` to noopener, but older /
embedded / downgraded clients don't — and `rel="opener"` opts back in
to the vulnerable behaviour).

| Finding | Sev | Catches |
|---|---|---|
| `link.tabnab-vulnerable` | strict | `target="_blank"` outbound link without `rel="noopener"`. Destination can navigate the original tab to a phishing URL via `window.opener`. |
| `link.opener-explicit` | strict | `rel="opener"` explicitly set — opts back in to tabnab vulnerability. |
| `link.outbound-no-noreferrer` | warn | Outbound link without `rel="noreferrer"` — leaks current URL (and any session-token query params) to the destination's analytics. |

"Outbound" = different `origin` from the page (resolved via `URL(href, document.baseURI)`). Same-origin and non-http(s) schemes (mailto:, tel:, javascript:, data:) are skipped.

---

## Axe rule de-duplication

The crawler runs `axe-core` for general WCAG coverage, but **drops
specific axe rules from the `a11y-violation` stream when a
first-class detector covers the same conceptual failure with
richer aggregation**. See `AXE_RULES_SUPERSEDED` in `src/audit.ts`.

Currently suppressed:

| Axe rule | Replaced by |
|---|---|
| `label` | `formLabels` (form.no-label / placeholder-only / required-no-indicator) |
| `label-title-only` | `formLabels` (treats title as last-ditch source, not a label) |
| `target-size` | `tapTargets` (with WCAG 2.5.8 inline exception) |

`form-field-multiple-labels` is intentionally NOT suppressed — no
first-class detector covers the multi-label collision case yet.

---

## Adding a new detector

1. **Decide if it's worth a first-class axis.** If `axe-core`
   already covers the rule cleanly, prefer extending the dedupe
   list to suppress the duplicate rather than adding a thin
   wrapper. Add a first-class detector when one of:
   - You can produce richer aggregation (single finding with
     example list + count, vs axe's per-element noise).
   - You can apply project-specific exceptions (e.g. inline-in-
     sentence WCAG 2.5.8 exception).
   - The rule isn't an axe rule at all (e.g. tapTargets at AA
     24×24, viewport-meta missing-tag).

2. **Implement TypeScript first.** Add `src/<name>.ts` with the
   shape:
   - `interface <Name>Snapshot` — capture-side typed result.
   - `interface <Name>Finding { severity; kind; detail; evidence }`.
   - `async function capture<Name>Snapshot(page: Page): Promise<...>`.
   - `function detect<Name>Issues(snap): Finding[]` — pure function,
     no I/O. This is the testable surface.
   - `async function check<Name>(page) { capture + detect }` —
     convenience wrapper.

3. **Add unit tests.** `src/<name>.test.ts` exercises pure-function
   detect logic with hand-built snapshots. Cover: clean state,
   each finding kind, boundaries, exemptions, examples cap.

4. **Wire into `src/main.ts`.**
   - `import { capture<Name>Snapshot, detect<Name>Issues, type <Name>Finding }`.
   - Add `const <name>FindingsByStep` accumulator + `check<Name>`
     async function (mirror existing detectors' shape).
   - Call `await check<Name>(...)` in the `if (step.kind === 'goto')`
     block.
   - Add `<name>Findings` and `<name>FindingsStrict` to
     `report.counts`.
   - Add the per-step JSON dump if `findingsByStep.length > 0`.
   - Add a console summary line.
   - Add the diff print line.

5. **Wire into `src/report.ts`.**
   - Add the kind to the `CapturedEvent['kind']` union.
   - Add `<name>Findings` + `<name>FindingsStrict` to the
     `Report.counts` interface.
   - Add `new<Name>Findings: CapturedEvent[]` to the `Diff`
     interface.
   - Initialize it in `diffReports`.
   - Filter into it for both the no-prior and prior branches.
   - Add the entry to the positive-signal axes array.

6. **Mirror in Rust.** Create
   `crates/crawler-detectors/src/<name>.rs`:
   - `pub const <NAME>_JS: &str = r##"..."##;` — verbatim copy of
     the TS file's `evalFn` string.
   - Typed `<Name>Snapshot` (deserialized from JS result).
   - `pub fn detect_<name>_issues(&snap) -> Vec<crate::AxisFinding>` —
     pure function, mirror of the TS detect logic.
   - `mod tests` with a `js_brackets_balanced` test (catches
     truncation in the JS string) plus parity tests for each
     finding kind.

7. **Register the module.** Add `pub mod <name>;` to
   `crates/crawler-detectors/src/lib.rs`.

8. **Update this doc.** Add the new axis to the table above with
   its finding kinds, severities, and a sentence on what it
   catches. Cross-reference axe rules superseded if any.

---

## Detector regression-guard fixture

`fixtures/t76-detectors/serve.py` is a deliberately-broken fixture
server. Each route is engineered to trigger ONE specific finding
kind across the T76 detector axes. The matching journey at
`journeys/t76-detector-fixtures.json` visits every route. Together
they let an audit prove that:

1. Every detector axis is alive (it fires when it should).
2. Every detector axis is well-calibrated (it stays silent when
   it shouldn't fire — see the `/control/` route).

**Why this matters:** the 2026-05-14 NaN-evalFn bug silently broke
the formLabels detector across every page of every audit. No test
caught it because every existing test exercised the *detector* in
isolation, not the *audit pipeline* through a real page.evaluate.
The fixture closes this gap — a single audit run against the
fixture server is a comprehensive liveness check.

### Automated check: `scripts/check-t76-detectors.sh`

```sh
scripts/check-t76-detectors.sh           # PASS or FAIL exit code
scripts/check-t76-detectors.sh --keep    # keep run dir for inspection
```

The script:

1. Starts the fixture server on port 8771 (kills stale instances first).
2. Runs the audit against `journeys/t76-detector-fixtures.json`.
3. Parses `runs/.../report.json` and matches each event to its
   page by URL (NOT by stepLabel — `report.eventsByStep` time-window
   slicing under-sizes goto+settle and findings spill into the wrong
   step's bucket).
4. For each label: asserts every expected finding kind in
   `expectedFindingsByLabel` was observed for that step's URL.
5. Cleans up the fixture server and run dir on exit (unless `--keep`).
6. Exits 0 on PASS, 1 on missing expected findings, 2 on infra
   failure (jq missing, server didn't start, etc.).

Manual run (without the script wrapper):

```sh
# Terminal 1: start the fixture server
python3 fixtures/t76-detectors/serve.py --port 8771

# Terminal 2: run the audit
npm run audit -- --journey journeys/t76-detector-fixtures.json
```

Wire to CI: any pre-merge check or scheduled job can shell out to
`scripts/check-t76-detectors.sh` and gate on its exit code.

---

## Pending detectors *(roadmap)*

Detectors queued for future T76 firings — each is high-leverage,
zero-overlap with existing axes:

- **`mixedFormSubmission`** — `<form action="http://...">` on
  https page. SECURITY.

Pick from this list for the next T76 cycle. Prefer those with no
existing axe-core coverage or where the project-specific aggregation
adds clear value.
