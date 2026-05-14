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

## Pending detectors *(roadmap)*

Detectors queued for future T76 firings — each is high-leverage,
zero-overlap with existing axes:

- **`skipLink`** — missing "skip to content" link as the first
  focusable element. WCAG 2.4.1.
- **`metaDescription`** — missing or empty `<meta name="description">`,
  duplicate descriptions.
- **`favicon`** — missing favicon, broken favicon URL.
- **`outboundLink`** — outbound `<a>` without `rel="noopener"` —
  tabnabbing risk.
- **`mixedContent`** — `https://` page loading `http://` resources.
- **`linkUnderline`** — links indistinguishable from surrounding
  text (no underline + colour-only differentiation, fails WCAG 1.4.1).
- **`fontLoading`** — `font-display: swap` missing → invisible-text
  flash (FOIT).
- **`autocomplete`** — login/email/address forms missing
  `autocomplete` attribute hints.
- **`crossPageTitleDup`** — same `<title>` on every page of a
  multi-step journey. (Aggregates-layer detector — operates on
  the run report, not per-page snapshot.)

Pick from this list for the next T76 cycle. Prefer those with no
existing axe-core coverage or where the project-specific aggregation
adds clear value.
