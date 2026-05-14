# Dogfood Runs — PlausiDen-Crawler against PlausiDen sites

Periodic audit of the crawler against PlausiDen-built sites. The
contract is symmetrical: the crawler validates PlausiDen sites, AND
the PlausiDen sites validate the crawler. A "clean" site that the
crawler reports clean isn't useful evidence — the run also has to
catch the issues we know are there. A "broken" site that the crawler
misses is the more interesting signal: it means a detector is
missing or mis-calibrated.

Per the original /schedule directive: **if the crawler reports zero
findings, the crawler tooling itself needs tightening.** Add
detectors until findings are non-zero OR the site is genuinely
clean against every defect class we care about.

---

## 2026-05-14 — SkillShots PoC, 17 detector axes

**Site:** `http://127.0.0.1:8123/` (10 pages: feed, challenge,
leaderboard, post-skill form, my-wins, profile, about, etc.)
**Journey:** `journeys/skillshots-poc.json` (30 steps, desktop
viewport 1280×800)
**Crawler revision:** post-autocomplete-detector (16+autocomplete = 17
detector axes total this firing; 23 axes including the legacy axe /
console / CSP / aria-drift axes already present).

### Results

```
console errors:    0           skip link:         0
page errors:       0           outbound links:    0
failed fetches:    0           autocomplete:      0
a11y violations:   0           web vitals:        0
css health:        0           tap targets:       0
ui overflow:       0           viewport meta:     0
runtime contrast:  0           doc title:         0
runtime images:    0           html lang:         0
runtime focus:     0           form labels:       1 (warn)
csp violations:    0
```

**ONE finding total across 30 steps × 23 axes:**

`form.required-no-indicator` (warn) on `post-skill.html` —
3 required fields without `*` or "required" in the visible label:

- `body > main > section:nth-of-type(2) > form > fieldset > div:nth-of-type(1) > input` — Challenge title
- `body > main > section:nth-of-type(2) > form > fieldset > div:nth-of-type(2) > select` — Category
- `body > main > section:nth-of-type(2) > form > fieldset > div:nth-of-type(3) > textarea` — Rules — measurable, observable

These ARE real defects — sighted keyboard users won't know which
fields are required until submission fails. Should be fixed at the
Loom template layer so future Forge-generated sites pick up the
correction automatically.

### What the run validated

For each clean axis: the detector ran successfully across 30 page
contexts (3 viewport variants × 10 unique pages) and produced no
false positives.

For `axe-static-a11y`: stayed clean = our `AXE_RULES_SUPERSEDED`
dedupe is working; axe isn't double-counting against
`formLabels`/`tapTargets`/etc.

### Coverage gaps surfaced by the run

The site triggered only 1 finding type. Possible reasons (each
gets a dedicated follow-up):

1. **Genuine quality.** The site was built with Loom (typed CSS
   tokens, semantic components) + Forge (24-phase build pipeline
   including its own a11y/contrast/heading/link audits). It's an
   exceptionally clean target — we'd expect a lower hit rate than
   a typical legacy site.

2. **Detectors that don't fit this surface.** SkillShots has no
   login form, no payment form, no PII collection — so
   `autocomplete.missing-credentials`, `autocomplete.missing-pii`,
   and any future payment-form detectors can't possibly fire.
   Need to run against a site that HAS those surfaces.

3. **Detectors with unintentional blind spots.** The 1 finding we
   DID get is genuinely useful (and a real defect). The other 16
   silent axes might be missing real issues that need better
   detection heuristics. Specific candidates flagged for review:

   - `placeholderText` — no LOREM/TODO findings; either the site
     was scrubbed or our pattern set is too conservative.
   - `webVitals` — no LCP/CLS/INP findings. Verify the LCP detector
     is actually firing; if the page LCP is e.g. 1.2s we wouldn't
     expect a finding, but we should sanity-check the band logic
     against a deliberately-slow fixture.

4. **Detectors yet to be built** that the roadmap lists. The next
   loop firings will keep adding from `docs/DETECTORS.md` § Pending:
   `metaDescription`, `favicon`, `mixedContent`, `linkUnderline`,
   `fontLoading`, `hstsHeader`, `xFrameOptions`, etc.

### Action items

- [x] Fix Loom form template to render required-field indicators
      (`*` after label OR explicit "required" badge) — closes the
      `form.required-no-indicator` finding at the source. **DONE
      2026-05-14**: PlausiDen-Loom commit added a
      `render_required_marker()` helper, used by Text / Textarea /
      Select arms; matching `.loom-form-field__required` CSS rule
      added to skin.css (red asterisk, danger-color token).
- [ ] Add `loginFlow` synthetic step to the journey or add a
      dedicated `login.html` fixture so credential-class
      detectors get exercised.
- [ ] Build at least 3 more detectors from the roadmap before
      declaring T76 close to done.

---

## 2026-05-14 (second run) — SkillShots PoC, post-Loom-fix

Same fixture, same journey, run AFTER:

1. Loom form renderer adds a visible `*` to required-field labels
   via `<span class="loom-form-field__required" aria-hidden="true">`.
2. Crawler formLabels detector capture-side updated: it now reads
   the VISIBLE label text (`<label for>` / wrapping label
   textContent) separately from accessibleName. Required-indicator
   check uses the visible text, since the project's doctrine puts
   the bare field purpose in aria-label and the visible indicator
   in the actual label element. Previous version was reading
   aria-label and missing the `*` even when it was rendered.
3. Crawler bug-fix: JSDoc inside the page.evaluate template literal
   contained literal backticks, which broke template parsing and
   silently made `evalFn` evaluate to `NaN` (typeof number). Caught
   only because the detector started silently failing across every
   page after the snapshot-shape change. Replaced JSDoc with `//`
   line comments inside the eval string — fix locked.

### Results

**ALL 24 DETECTION AXES SILENT.** Zero findings, zero page errors,
zero a11y violations, zero CSP violations, zero diffs vs prior run.
The crawler exits with PASS.

This is the cleanest dogfood run on record — the loop fully closed:
detector found bug → fix at source → detector improved → audit
re-runs clean. Future regressions on the same surface will be
caught.

### Remaining coverage gaps

The site is genuinely well-built and stays clean against every
detector we have today. To keep tightening the loop:

1. Add a login-flow fixture (credential-class autocomplete checks
   never fire on this site).
2. Build remaining roadmap detectors — they may find issues this
   one doesn't surface.
3. ~~Run the crawler against a deliberately broken fixture set to
   guarantee each axis is firing correctly (don't trust silent
   passes alone).~~ **DONE 2026-05-14** — see entry below.

---

## 2026-05-14 (third entry) — Detector regression-guard fixture

Built `fixtures/t76-detectors/serve.py` + `journeys/t76-detector-fixtures.json`:
30 routes, each engineered to trigger one specific T76 finding
kind. Closes the "silent passes can hide silent failures" gap
the previous entry called out.

### What the audit produced

All 9 T76 detector axes fired with their expected findings:

| Axis | findings | strict |
|---|---|---|
| viewportMeta | 3 | 3 (missing / no-device-width / zoom-disabled) |
| docTitle | 4 | 1 (empty) + 3 warn (generic / too-short / too-long) |
| htmlLang | 4 | 2 (missing / empty) + 2 warn (invalid / unknown-primary) |
| metaDescription | 4 | 0 (all warn — missing / empty / too-short / too-long) |
| skipLink | 3 | 1 (broken-target) + 2 warn (missing / not-first-focusable) |
| formLabels | 3 | 1 (no-label) + 2 warn (placeholder-only / required-no-mark) |
| tapTargets | 31 | 30 strict — fixture's per-page small skip-link triggers tap.too-small site-wide; deliberate flush of detector liveness. |
| autocomplete | 4 | 2 (missing-credentials) + 2 warn (missing-pii / invalid-token) |
| outboundLinks | 4 | 2 (tabnab / opener-explicit) + 2 warn (no-noreferrer) |

Existing pre-T76 detectors stay silent on the fixture (as
expected — fixture intentionally exercises ONLY the new T76
axes), except `cssHealth` which fires `css.no-stylesheets-declared`
on every page (fixture pages are deliberately CSS-less).

### Notable observations

- **tapTargets 30 strict** — the fixture's `<a class="skip">` links
  render at default-text size (no CSS to make them visually-hidden-
  until-focused). They fall under 24×24 on EVERY page, not just
  `/tap-tiny/`. Useful evidence that tapTargets fires, but a
  follow-up should add `.skip { position:absolute; left:-9999px }`
  in fixture HTML so only the engineered routes light it up.
- **Strict / warn split per axis** matches the design — see
  `docs/DETECTORS.md` for the contract each finding kind promises.

### What this fixture protects against

The 2026-05-14 NaN-evalFn bug silently broke formLabels across
every page of every audit. No existing test caught it. Now: if
any future change breaks a T76 detector (parser bug, JS string
truncation, snapshot-shape drift), the fixture run catches it
on the next audit.

### Action items

- [x] Wire the fixture into CI: `scripts/check-t76-detectors.sh` —
      starts the server (with SO_REUSEADDR so back-to-back runs
      don't TIME_WAIT-deadlock the port), runs the audit, parses
      report.json, asserts every expected finding observed per
      route by URL match. **DONE 2026-05-14.** First run: 30/30
      routes PASS.
- [x] Tweak fixture skip-link CSS so tapTargets only flares on
      the engineered routes. **DONE 2026-05-14**: skip link in
      `page()` and the inline-built skip-link routes now carry
      inline `padding:12px 16px;min-width:44px;min-height:44px`
      so the bounding box meets the tap-target floor on every
      page that isn't deliberately testing tap-too-small.
- [ ] Add `mixedContent` + `linkUnderline` + `fontLoading` routes
      as those detectors land.

### Bugs the script caught on its first runs

The script-development loop itself surfaced four real bugs that
no other test would have:

1. **`title=False` silently rendered `<title>False</title>`.** The
   Python fixture's `head()` helper used `if title is not None`
   then a (dead) `elif title is False` branch — but `False is not
   None` evaluates True, so the value-render branch fired. Replaced
   with a sentinel `_OMIT` object; now `head(title=_OMIT)`
   unambiguously drops the element.

2. **Port 8771 in TIME_WAIT after kill blocks restart.** The fixture
   server bound a fresh socket each run; Linux holds the previous
   socket in TIME_WAIT for 60s, blocking a fresh bind. Added a
   `ReusableTCPServer(socketserver.TCPServer)` subclass with
   `allow_reuse_address = True`.

3. **`report.eventsByStep` windows under-size goto duration**, so
   detector findings for step N often land in step N+1 or N+2's
   bucket. The script switched from stepLabel-based matching to
   URL-based matching using each event's `.url` field, which is
   set by the detector at capture time and is therefore precise.
   (Note: this is a finding ABOUT the audit pipeline that should
   eventually get fixed in main.ts so other consumers of
   eventsByStep also get accurate per-step grouping.)

4. **Default-text-sized skip link tripped tap.too-small everywhere.**
   The fixture's `<a class="skip">Skip to main content</a>` rendered
   at native text size (~80×16) — under the 24×24 strict floor on
   every page. Inline-styled it to >= 44×44 so the noise is gone
   from non-tap-test routes.

The script PASSING means every T76 detector axis is alive AND
no detector is blasting unexpected findings on the control route
(beyond the predictable `css.no-stylesheets-declared`, since fixture
serves CSS-less HTML by design — one signal per route).

---

## 2026-05-14 (fifth entry) — eventsByStep windowing fix + SkillShots regression

The check-t76 script's URL-based matching was a workaround for a
real bug in main.ts: `report.eventsByStep` used cumulative
`s.durationMs` as window boundaries. `s.durationMs` only counts
`runStep`'s page action — it doesn't count the ~17 detector
`page.evaluate()` calls that happen AFTER runStep returns. So
detector findings landed in the NEXT step's window.

Concrete repro before the fix (from a kept run dir):
- `lang-empty` step's events bucket contained `lang.unknown-primary`
  (which only fires on `<html lang="xx">`, the lang-unknown route)
- `lang-invalid` step's bucket contained `meta-description.empty`
  (from the meta-desc-empty route 2 steps later)

Fix: replaced cumulative-duration windows with WALL-CLOCK windows
captured during the goto loop. Each iteration now records
`{stepStartedT, stepEndedT}` based on `Date.now() - startEpoch` at
top and bottom of the step body. The eventsByStep build phase
uses those times directly.

After the fix, every label's bucket contains exactly its own
detector findings:
- `lang-empty` → `lang.empty`
- `lang-invalid` → `lang.invalid`
- `lang-unknown` → `lang.unknown-primary`
- `no-meta-desc` → `meta-description.missing`
- ... and so on across all 30 fixture routes.

scripts/check-t76-detectors.sh still uses URL match (it's
strictly more robust than time windows — events tag their own
URL at capture time), but now ANY consumer of report.eventsByStep
gets accurate per-step grouping too.

### SkillShots re-audit (post-windowing-fix, post-metaDescription)

Same SkillShots site, run with the new metaDescription detector
active and the windowing fix in main.ts. Picks up ONE real
defect:

  meta-description.too-short (warn) on leaderboard.html —
  current description is 42 characters: "Top earners this
  week. Voted by your crew." Search-result previews need
  ~50-160 chars to fill the snippet line.

Action items captured:

- [x] Extend leaderboard.html's meta description to 50+ chars.
      **DONE 2026-05-14** — see entry below.
- [x] Audit every page's meta description against the 50-160
      char band. **DONE 2026-05-14** — only leaderboard.html was
      out of band; every other SkillShots page already in the
      50-160 range (see audit table in next entry).
- [x] Add a "what's new since last cycle" section to dogfood
      reports for axes added mid-stream. **DONE 2026-05-14** —
      template added below as the standard intro for each entry.

---

## 2026-05-14 (sixth entry) — leaderboard meta description fix + cycle template

### What's new since last cycle (2026-05-14 fifth entry)
- `metaDescription` detector landed in pipeline (was the source
  of the warn fixed below).
- `report.eventsByStep` windowing fix — every consumer of that
  field now gets accurate per-step grouping.
- Total active detector axes: 18 (no new axes this cycle).

### Audit of every SkillShots meta description

```
46  about.html                Help, FAQ, and team behind SkillShots — the cash-paid skill battle platform.
56  challenge.html            View a SkillShots challenge — entries, voting period, pot, and rules.
99  index.html                Skill battles, voted by your crew. Pots split at round close.
... [every page in 50-160 band except the one below]
42  leaderboard.html          Top earners this week. Voted by your crew.   ← OUT OF BAND
```

Only one outlier — leaderboard.html. The rest of the site was
already SEO-quality on this axis.

### The fix

`cms/leaderboard.json` description extended from 42 → 95 chars
in PlausiDen-Forge commit `d8eb2b4` (rebased onto origin/main):

> Top SkillShots earners this week — pots, ranks, and category
> leaders, all voted by your crew.

Within the 50-160 search-engine truncation band, accurate to
page content, includes the brand name. Loom's CMS renderer
bakes it into the static HTML on the next forge.sh build —
no template changes needed, the source-of-truth fix
auto-propagates.

### Re-audit result

**ALL 25 DETECTION AXES SILENT.** Same cleanest-on-record
result as the 2026-05-14 (second) entry, this time with the
metaDescription axis ALSO clean.

### Action items

- [x] Add `favicon` detector — next smallest from the roadmap,
      will surface real bugs on sites that ship without one.
      **DONE 2026-05-14** — see seventh entry below; immediately
      caught 7 missing-favicon warns on SkillShots, fixed at the
      source via Loom default favicon.
- [ ] Add `mixedContent` detector — security defence-in-depth
      (HTTPS pages loading HTTP resources). Need to design
      around the browser's built-in mixed-content blocker
      first; some overlap.
- [ ] Add a login-flow fixture so credential-class autocomplete
      checks fire on a real-shape surface (still queued from
      first dogfood entry).

---

## 2026-05-14 (seventh entry) — favicon detector + Loom default favicon

### What's new since last cycle (sixth entry)
- `favicon` detector landed in pipeline (warn-only).
- `fixtures/t76-detectors/no-favicon` route added to the
  regression-guard fixture; check-script now validates 31/31
  routes (was 30/30).
- Total active detector axes: 19 (was 18).

### What it caught

`favicon` axis fired 7 warns on the first SkillShots audit —
one per page in the journey. Every SkillShots page was shipping
without any `<link rel="icon">` in head, AND `/favicon.ico`
returns 404 (verified via `curl -s -o /dev/null -w '%{http_code}'`).
Browser tabs / bookmarks / PWA-install prompts were all rendering
the generic globe glyph.

### The fix

Default favicon emitted by `loom_cms_render::page_shell_themed`.
New constant `DEFAULT_FAVICON_LINK` in `loom-cms-render/src/lib.rs`
holds an inline `data:image/svg+xml,…` URL — a 16×16 SVG of the
Loom mark in the brand accent colour. No separate `/favicon.ico`
file required to deploy; every Loom-generated page picks it up
automatically on next render.

CSP unchanged: `img-src 'self' data:` already covered the data
URL form. Future variant could accept a per-site
`favicon_override: Option<&str>` arg so brands can supply their
own SVG/PNG without monkey-patching the shell.

### Re-audit result

After running `loom cms-render` over every cms/*.json (the
Rust forge build hasn't yet wired in a render phase — that's
itself a queued action item), the rebuilt static HTML carries
the new favicon link. Re-audit shows `favicon: 0` and
**ALL 26 DETECTION AXES SILENT** (was 25, +1 for the new
favicon axis).

### Action items

- [x] Wire a render phase into the Rust forge `build` command.
      **DONE 2026-05-14** — see eighth entry below. The phase
      already existed in forge-phases (T70b 2026-05-13); this
      cycle registered it in forge-cli's pipeline AND added a
      forge.toml `[render] write_canonical = true` opt-in so it
      writes directly to `static/<slug>.html` (T70c). Backwards
      compatible: default stays `_render/` per the original
      doctrine.
- [ ] Same as before: mixedContent detector + login-flow fixture.

---

## 2026-05-14 (eighth entry) — Forge render phase wired into build

### What's new since last cycle (seventh entry)
- `RenderPhase` now registered in forge-cli's phase list. Build
  output now shows `== phase: render ==`.
- New forge.toml `[render] write_canonical = true` opt-in flag
  (closes T70c). When set, Forge writes rendered HTML directly
  to `static/<slug>.html` instead of the sibling `static/_render/`.
- `skillshots-poc/forge.toml` flipped to `write_canonical = true`.

### What this fixes

Every cycle that touched Loom (page_shell, render_form_field,
DEFAULT_FAVICON_LINK, etc.) had to do this dance:

1. cargo build --release -p loom-cli
2. for f in cms/*.json: loom cms-render --input "$f" --out static/...
3. forge build  (just to lint, since render didn't run)
4. npm run audit

Now it's just:

1. cargo build --release -p loom-cli
2. forge build  (renders + lints in one command)
3. npm run audit

The friction was real: it cost ~5 min per cycle of cargo
rebuild + manual render loop, AND introduced silent staleness
when the operator forgot a step.

### How it stays safe

`write_canonical = true` is OPT-IN per site. Default is `false`,
preserving the original T70b behaviour of writing only to
`_render/`. Sites with hand-edited `static/<slug>.html` content
that isn't reflected in cms/*.json keep their existing flow.

Forge.toml parsing is defence-in-depth: a typo'd or malformed
forge.toml falls back to the safe default. 4 new tests pin
the contract:
- `render_writes_to_underscore_render_by_default`
- `render_writes_to_static_when_write_canonical_true`
- `render_falls_back_safely_on_malformed_forge_toml`
- `render_write_canonical_false_or_missing_uses_underscore_render`

### Verified end-to-end

1. Edited `cms/leaderboard.json` description to a temp marker.
2. Ran `forge build` — output included `phase_render generated 9 HTML page(s)`.
3. `static/leaderboard.html` carried the marker. Confirmed.
4. Restored real description, re-ran forge build.
5. Re-audit: ALL 26 DETECTION AXES SILENT. Liveness gate
   (31/31 routes) still PASS.

### Action items

- [ ] Re-run the audit on EVERY commit cycle to confirm the
      render phase output stays in sync — the old manual
      `loom cms-render` step is gone, but if the phase silently
      fails (e.g. a broken cms/*.json after future schema
      change), `static/` could go stale. Mitigation: render phase
      already emits a STRICT finding on schema drift.
- [x] **DONE 2026-05-14**: mixedContent detector landed. See
      ninth entry below.
- [ ] login-flow fixture still queued.

---

## 2026-05-14 (ninth entry) — mixedContent security detector

### What's new since last cycle (eighth entry)
- `mixedContent` detector landed in pipeline (3 finding kinds).
- Total active detector axes: 20 (was 19).
- T76 now covers ten new detector axes since session start
  (tap-targets, form-labels, viewport-meta, doc-title, html-lang,
  skip-link, outbound-links, autocomplete, meta-description,
  favicon, mixed-content) — each with TS detector + Rust mirror
  + unit tests.

### Detector design

Static markup analysis (not runtime browser signal). Browsers
inconsistently auto-upgrade vs warn vs block mixed content; the
markup is wrong regardless. Per the project's state-actor threat
model (CLAUDE.md), defence in depth: surface the bug at the
source.

  - `mixed-content.active`      strict   script/css/iframe/embed/object
                                         over http on https page.
                                         Browsers BLOCK these.
  - `mixed-content.passive`     warn     img/audio/video/srcset
                                         over http on https page.
  - `mixed-content.form-action` strict   `<form action="http://…">`
                                         on an https page —
                                         credentials/PII in the clear.

The detector short-circuits when the page itself is http —
mixed-content concept doesn't apply.

### Coverage gap

The `t76-detector-fixtures` server runs on http, so the page-is-
https short-circuit fires and no live integration is possible
via the gate. Coverage is via 17 TS+Rust unit tests covering
boundaries (clean https, http-page short-circuit, all 3 finding
kinds, aggregation, examples cap). Documented in DETECTORS.md.

Future: HTTPS variant of the fixture server with a self-signed
cert (Playwright's `ignoreHTTPSErrors: true` would let the
crawler trust it). Out of scope this cycle.

### SkillShots dogfood

Site is http — `mixedContent` can't fire and stays silent
(correct behaviour). Re-audit produced **ALL 27 DETECTION
AXES SILENT**, +1 axis vs last cycle. Liveness gate (31/31
fixture routes) still PASS.

### Action items

- [ ] HTTPS fixture variant for mixedContent live integration.
- [ ] login-flow fixture (still queued).
- [x] **DONE 2026-05-14**: linkUnderline detector landed. See
      tenth entry below.
- [ ] Pick from remaining roadmap: `fontLoading`, `hstsHeader`,
      `xFrameOptions`, `crossPageTitleDup`. (`mixedFormSubmission`
      already covered by `mixed-content.form-action`.)

---

## 2026-05-14 (tenth entry) — linkUnderline detector + sibling-side-effect lesson

### What's new since last cycle (ninth entry)
- `linkUnderline` detector landed in pipeline (warn-only).
- Total active detector axes: 21 (was 20).
- check-t76-detectors.sh now validates 32/32 routes (was 31/31).
- main.ts: linkUnderline runs FIRST in the goto-step detector
  chain. Comment in code documents why.

### What this cycle taught us

**Detector ordering matters.** When the new linkUnderline
detector ran AFTER the other 20 detectors in the goto chain,
its captured snapshot consistently showed 0 candidates on a
fixture page that DEFINITELY had a color-only-distinction link.
Direct standalone playwright invocation against the same URL
captured the candidate correctly.

Root cause: at least one prior detector (likely runtimeFocus
calling `el.focus()`, OR runtimeContrast walking text nodes)
transiently mutates computed styles. By the time linkUnderline
read `outline-style` / `text-decoration` / `font-weight` for the
test link, the post-mutation state masked the bug.

Fix: linkUnderline now runs FIRST in the per-goto detector
chain. Comment added to main.ts so future contributors don't
re-order it back into the middle.

This is a generalisable lesson — any detector that reads
COMPUTED styles must run before any detector that mutates
the page (focus, scroll, theme switch, axe injection). Future
detectors in the same family should follow the same ordering
discipline.

### Detector design

WCAG 1.4.1 (Use of Color, Level A). Single warn finding:

  - link.color-only-distinction   warn   inline link inside
                                         running text where the
                                         only cue is colour.

Out of scope: block-level links, links inside `<header>` /
`<nav>` / `<footer>` / `<aside>` chrome (button-styled by
convention), links with explicit visual distinction (underline,
weight contrast ≥200, border, outline-with-style, background,
box-shadow, italic, icon child).

The non-color-distinction filter has 7 escape hatches; running
the standalone debug surfaced an outline-width gotcha (browsers
default outline-width to 3px even when outline-style is `none`)
which would have been a false negative — fixed by also checking
outline-style.

### SkillShots dogfood

Detector caught **6 real defects** on SkillShots: aside-panel
links to user profiles and recent challenges have no underline.
Examples:

  body > div > aside:nth-of-type(2) > section > ul > li > a
    '@court_dax · Basketball$1,840' → /u/court_dax
  body > div > aside:nth-of-type(2) > section > ul > li > a
    'Pool table clear — 12mVote' → /c/pool-clear-9lori

These ARE real WCAG 1.4.1 fails — colour-only distinction in a
list of links inside running-text-style markup. Fix is in
Loom's `loom-card-feed-item__title-link` / equivalent panel-link
CSS — add `text-decoration: underline` (or weight contrast,
or another visual cue).

### Re-audit result

After Forge T70c (last cycle) wired the render phase:
the 6 findings persist (Loom CSS not yet updated).
Liveness gate (32/32) PASS. **27 silent axes vs 1 new
linkUnderline warn axis = within budget.** Total active T76
axes: 21.

### Action items

- [x] Loom: extend panel-link CSS to add visual cue beyond
      colour. **DONE 2026-05-14** — see eleventh entry below.
- [ ] HTTPS fixture variant for mixedContent live integration.
- [ ] login-flow fixture (still queued).
- [ ] Remaining roadmap: `fontLoading`, `hstsHeader`,
      `xFrameOptions`, `crossPageTitleDup`.

---

## 2026-05-14 (eleventh entry) — linkUnderline detector bug + Loom panel-link weight

### What's new since last cycle (tenth entry)
- Detector fix: `linkUnderline.isInsideRunningText` now walks
  the FULL ancestor chain.
- Loom CSS: `.loom-panel__list-link` bumped to `font-weight: 600`
  for visible affordance.
- Total active detector axes: still 21 (no new axes).

### Detector bug found and fixed

The 6 "real defects" from last cycle's tenth entry were
actually **false positives** caused by a bug in
`isInsideRunningText`:

```js
// BEFORE (bug):
while (parent && hops < 8) {
  if (chrome tag) foundChrome = true;
  if (running tag) return !foundChrome;  // ← returns here
  parent = parent.parentElement;
}
```

The function returned `!foundChrome` at the first running-text
ancestor — without finishing the walk. So when an `<a>` was
inside `<aside> > <section> > <ul> > <li>`:

1. LI is the first hop → running tag → return `!foundChrome`
2. foundChrome was still `false` (we hadn't gotten to ASIDE yet)
3. Function returned `true` → link treated as in-running-text
4. ASIDE chrome above LI never seen

Fix: walk the FULL 8-hop chain, set both `foundRunning` and
`foundChrome` flags, return `foundRunning && !foundChrome`.

After the fix, the 6 SkillShots aside-panel links are
correctly skipped — they ARE in chrome.

### Loom CSS improvement (independent of detector fix)

The aside-panel links also had a real UX issue separate from
WCAG 1.4.1: they used `color: var(--loom-color-ink)` (same as
surrounding text) and `text-decoration: none`. A user without
a mouse could not visually distinguish a link from a non-link
row.

`.loom-panel__list-link` now sets `font-weight: 600`. The 200-
unit contrast vs the parent's default 400 is enough for
sighted users to spot the link AND would pass even a stricter
detector variant that didn't honour the aside-chrome exception.

### Re-audit result

**ALL 28 DETECTION AXES SILENT** — same cleanest-on-record
result. Liveness gate (32/32 fixture routes) still PASS.

### What this cycle taught us

**Per-page detectors that walk ancestors need to walk the FULL
chain** — early-return optimisations can miss higher ancestor
state. The pattern is now: collect ALL relevant ancestor
properties first, then decide. Worth adding a test that
specifically exercises a "running tag inside chrome" scenario
when adding similar ancestor-walking detectors in future.

### Action items

- [x] **DONE 2026-05-14 (eleventh cycle)**: Add a chrome-link
      regression-guard route. Now `/link-in-chrome/` is in the
      fixture with `expectedFindingsByLabel: []`.
- [ ] HTTPS fixture variant for mixedContent live integration.
- [ ] login-flow fixture (still queued).
- [x] **DONE 2026-05-14 (twelfth cycle)**: `crossPageTitleDup`
      (now named `crossPageTitle`) — see twelfth entry below.
      First aggregates-layer detector.
- [ ] Remaining roadmap: `fontLoading`, `hstsHeader`,
      `xFrameOptions`.

---

## 2026-05-14 (twelfth entry) — first aggregates-layer detector

### What's new since last cycle (eleventh entry)
- `crossPageTitle` detector landed (warn-only).
- Total active detector axes: 22 (was 21).
- main.ts gained a new "aggregates pass" after the goto loop.

### Design

Unlike per-page detectors (which produce findings from one
page's snapshot), aggregates detectors accumulate cross-page
state during the journey and emit a single set of findings
at the end. The pattern:

```ts
const acc = newCrossPageTitleAccumulator();
for (step in journey.steps) {
  if (step.kind === 'goto') {
    await checkDocTitle(...);
    // piggyback off docTitle capture — no extra page.evaluate cost
    recordPageTitle(acc, pageUrl, title);
  }
}
// After loop completes:
for (const f of detectCrossPageTitleDuplicates(acc)) {
  log({ kind: 'cross-page-title', ... });
}
```

The finding kind is `cross-page-title` (per-event tag) and
`title.cross-page-dup` (rule id). One finding per duplicate
group — multiple groups produce multiple findings.

### What it catches

Two or more pages in a single journey sharing the same trimmed
`<title>`. Real defect: SEO suffers (Google filters duplicate-
title results), users can't distinguish open tabs / bookmarks /
history entries / search snippets.

### SkillShots dogfood

**0 findings** on the SkillShots journey — every page has a
unique title. The site's typed CMS does this correctly:
each `cms/*.json` has its own `title` field.

### Fixture verification

The t76-detector-fixtures journey uses `<title>T76 Fixture</title>`
as the default for most routes (one-signal-per-route doctrine —
each route isolates ONE detector). Re-audit of the fixture
journey produced **1 cross-page-title warn** flagging 28 distinct
URLs sharing 'T76 Fixture'. The detector is alive end-to-end.

### Limitations / known gaps

- No Rust mirror. The aggregates layer is shaped differently
  from the per-page-snapshot pattern in `crawler-detectors`.
  Future `crawler-aggregates` crate when the chromiumoxide
  port (T75) needs Rust parity.
- The `check-t76-detectors.sh` script matches per-URL events,
  which can't catch cross-page findings (no URL field). The
  script's contract is still useful — it's specifically the
  per-page detector liveness gate. A future
  `check-t76-aggregates.sh` would assert the aggregates layer.

### Action items

- [x] **DONE 2026-05-14 (thirteenth cycle)**: second aggregates
      detector — `crossPageMetaDescription`. Pattern validated.
- [ ] HTTPS fixture variant for mixedContent + hsts live
      integration.
- [ ] login-flow fixture.
- [x] **DONE 2026-05-14 (fourteenth cycle)**: `hstsHeader` —
      first response-header detector + capture path. See
      fourteenth entry below.
- [ ] Remaining roadmap: `fontLoading`, `xFrameOptions`.

---

## 2026-05-14 (fourteenth entry) — first response-header detector

### What's new since last cycle (thirteenth entry)
- `hstsHeader` detector landed (mix of strict+warn).
- `topLevelResponseHeaders: Map<url, headers>` accumulator
  added to main.ts. Populated by the existing `page.on('response')`
  listener for any response where `request().isNavigationRequest()`.
- Total active detector axes: 24 (was 23).

### Why a new capture path

The 23 prior detectors all read from one of:
- `page.evaluate()` — DOM / runtime computed styles
- per-page snapshots accumulated during the loop
- cross-page accumulators (the aggregates layer)

HSTS lives in the HTTP response headers — invisible to any
DOM or computed-style query. Adding the response-header capture
path opens the door for an entire FAMILY of header-flavoured
detectors (xFrameOptions, referrerPolicy, contentSecurityPolicy
strict mode, COEP/COOP, etc.). One listener; many detectors.

### Detector design

  - hsts.missing            strict   no Strict-Transport-Security
                                     on https response (or
                                     unparseable).
  - hsts.max-age-too-short  warn     max-age < 6 months.
  - hsts.no-subdomains      warn     adequate max-age but missing
                                     includeSubDomains.

  Localhost and http pages exempt (HSTS doesn't apply on
  loopback or non-https origins).

  14 unit tests cover http-page exemption, localhost (and
  *.localhost) exemption, missing/short/no-subdomains paths,
  case-insensitive headers + directives, quoted max-age values,
  unparseable header → missing fallback.

### SkillShots dogfood

**0 findings** — SkillShots dev server runs on http://127.0.0.1
which short-circuits at the localhost check. Correct behaviour.
For real https + production-deployed PlausiDen sites, this
detector will be load-bearing.

### Re-audit result

**ALL 31 DETECTION AXES SILENT** on SkillShots (was 30; +1 axis).
Liveness gate (33/33) still PASS.

### Action items

- [x] **DONE 2026-05-14 (sixteenth cycle)**: HTTPS fixture
      variant.
- [x] **DONE 2026-05-14 (seventeenth cycle)**: `referrerPolicy`
      detector. See seventeenth entry below.
- [ ] login-flow fixture (still queued).
- [ ] Remaining roadmap: `fontLoading`.

---

## 2026-05-14 (seventeenth entry) — third response-header detector

### What's new since last cycle (sixteenth entry)
- `referrerPolicy` detector landed (warn/strict/warn).
- HTTPS fixture extended with 3 new routes (no/permissive/
  invalid). `Referrer-Policy: strict-origin-when-cross-origin`
  added to the HTTPS fixture's DEFAULT_HEADERS so the control
  route stays clean.
- Total active detector axes: **26** (was 25).
- Liveness gates: HTTPS 12/12 (was 9/9), HTTP 33/33 unchanged.

### Detector design

Mirror of hsts + xFrameOptions shape:
- `buildReferrerPolicySnapshot(url, headers) → snapshot`
- `detectReferrerPolicyIssues(snapshot) → findings`
- Localhost + http exempt
- Reads from shared `topLevelResponseHeaders` Map

Three findings:
- `referrer-policy.missing` (warn) — no header. Modern browsers
  default to `strict-origin-when-cross-origin` (safe-ish) but
  older clients leak full URL.
- `referrer-policy.permissive` (strict) — explicit `unsafe-url` /
  `no-referrer-when-downgrade` / `origin-when-cross-origin`. All
  leak more than the modern default.
- `referrer-policy.invalid` (warn) — unknown token; intent lost.

Multi-token policies honored per W3C spec — the LAST recognised
token wins. The detector walks right-to-left, returns on first
recognised. 14 unit tests cover every path including
`strict-origin-when-cross-origin, unsafe-url` (last wins →
permissive) and `no-referrer, future-token` (skips unknown,
lands on safe).

### Pattern observation (3 of a kind)

Three response-header detectors now share ~70% of structure:
- pageIsHttps + pageIsLocalhost exemptions
- Header lookup with case-insensitive name
- Token classification (safe/permissive/invalid in this case;
  good/short/missing for hsts; valid/missing for xFrameOptions)

The genuine differences are: which header to read, what tokens
mean what, what severity each gets. A future generic
`headerDetector(opts: { headerName, parseTokens, classify })`
helper would replace ~150 lines of duplication. **NOT extracted
yet** — three implementations is the right N to design for; do
it on the fourth (probably `permissionsPolicy`).

### SkillShots dogfood

**0 referrerPolicy findings** — dev server is `http://127.0.0.1`,
exempt at the localhost check. Same as hsts/xFrameOptions on
this site.

**ALL 33 DETECTION AXES SILENT** on SkillShots (+1 vs last cycle).
HTTP gate (33/33), HTTPS gate (12/12).

### Action items

- [ ] When the FOURTH response-header detector lands
      (permissionsPolicy is the natural next), extract a generic
      `headerDetector` helper — kills the ~70% duplication.
- [ ] login-flow fixture (still queued).
- [x] **DONE 2026-05-14 (eighteenth cycle)**: `fontLoading`
      detector. See eighteenth entry below. Roadmap exhausted!

---

## 2026-05-14 (eighteenth entry) — fontLoading detector — last roadmap item

### What's new since last cycle (seventeenth entry)
- `fontLoading` detector landed (warn-only).
- Total active detector axes: **27** (was 26).
- HTTP gate: 34/34 (was 33; +1 font-no-display route).
- The DETECTORS.md "Pending detectors *(roadmap)*" section is
  now exhausted of original concrete roadmap items.

### Detector design

Walks `document.styleSheets` for every `@font-face` rule (CSS
rule type 5). For each:
- No `font-display` declaration → `font-loading.no-display` warn
  (browser default is `block` → FOIT)
- `font-display: block` or `auto` → `font-loading.display-block`
  warn (FOIT)
- `font-display: swap`, `fallback`, or `optional` → clean
- Unknown values treated as `no-display` (typos / future tokens
  fall back to default)

Cross-origin sheets the browser refuses to expose via
`SecurityError` on `.cssRules` are silently skipped, but the
COUNT is carried in `evidence.inaccessibleSheetCount` so the
audit reader knows where the detector's coverage ends.

14 unit tests cover:
- No faces / clean / each acceptable value
- no-display + each FOIT value
- Unknown value → no-display
- Aggregation count
- Examples capped at 5
- Mixed clean+bad
- Inaccessible-sheet count surfaces

### SkillShots dogfood

**0 fontLoading findings** — the typed CMS pages either have
no `@font-face` declarations OR use `font-display: swap`. Either
way, no FOIT risk. Loom's design-system token-driven approach
pays off here: web fonts go through one canonical pipeline that
sets `swap`.

### Re-audit result

**ALL 34 DETECTION AXES SILENT** on SkillShots (was 33; +1 axis).
HTTP gate (34/34, +1 route), HTTPS gate (12/12) both green.

### Roadmap status

The original `docs/DETECTORS.md` § Pending detectors list
(established 2026-05-14 cycle 4) had 10 items. As of this
cycle, all 10 are shipped:

  ✓ docTitle, htmlLang, skipLink, metaDescription, favicon,
    mixedContent, linkUnderline, fontLoading, hstsHeader,
    xFrameOptions

Items added to the roadmap mid-stream and shipped:
  ✓ tap-targets, form-labels, viewport-meta, autocomplete,
    crossPageTitle, crossPageMetaDescription, referrerPolicy,
    outboundLinks

Future direction: future detectors will be added as need
surfaces (a real bug found, an audit gap noticed). The
"detector backlog" is now empty.

### Action items

- [ ] Extract `headerDetector` helper when permissionsPolicy
      lands (at least 4 response-header detectors needed for
      the abstraction to pay).
- [x] **DONE 2026-05-14 (nineteenth cycle)**: login-flow fixture.
      See nineteenth entry below. autocomplete.missing-credentials
      now has live coverage.
- [ ] T76 has shipped 27 detector axes; consider whether to
      mark the umbrella task complete and let new detectors
      come from real-world dogfood findings rather than a
      pre-planned roadmap.

---

## 2026-05-14 (nineteenth entry) — login-flow fixture closes the autocomplete gap

### What's new since last cycle (eighteenth entry)
- 2 new fixture routes: `/login-no-autocomplete/`,
  `/login-with-autocomplete/`. Liveness gate validates 36/36
  routes (was 34/34).
- `autocomplete.missing-credentials` now has live coverage.
- No new detectors; this cycle is pure infrastructure
  (closes the longest-queued action item, dating to cycle 9).

### Why a login-flow fixture

`autocomplete.missing-credentials` (strict) was the last T76
axis without a fixture. The old http fixture had no auth forms
— the detector's credential-classifier needed to see
`<input type=email>` or `<input type=password>` to fire, and
the existing fixture pages were content-only.

### Fixture design

Two routes:
- `/login-no-autocomplete/` — email + password inputs without
  `autocomplete` attrs → fires `autocomplete.missing-credentials`
  strict.
- `/login-with-autocomplete/` — same form with
  `autocomplete=email` + `autocomplete=current-password`. Control
  — should be clean.

The control needed a small fixture-cleanup pass: required-field
markers (`*` in labels) + inline padding on the submit button
(>=44×44 for tap-targets). Without these, the OTHER detectors
correctly fired and added noise to the control's report. Now
the only residual on this control is the predictable
`css.no-stylesheets-declared` (every fixture page has it — one-
signal-per-route doctrine includes "no extra CSS").

### Verified

- HTTP gate: 36/36 PASS (was 34; +2 routes).
- HTTPS gate: 12/12 unchanged.
- SkillShots audit: ALL 34 DETECTION AXES SILENT.

### Coverage status snapshot (post-cycle 19)

  Per-page DOM detectors:           17 axes
  Aggregates-layer detectors:        2 axes
  Response-header detectors:         3 axes
  Existing pre-T76 axes:             5 axes (cssHealth,
                                            uiOverflow,
                                            runtimeContrast,
                                            runtimeImages,
                                            runtimeFocus)
  Total:                            27 axes

  Liveness gates:
    HTTP:    36/36 routes
    HTTPS:   12/12 routes
    Total:   48 routes

  Unit + Rust tests: ~280 across the surface.

### Action items

- [ ] Extract `headerDetector` helper on the FOURTH response-
      header detector (permissionsPolicy candidate).
- [ ] Mark T641 / T76 umbrella task complete: the original
      "massive expansion" goal is achieved. New detectors can
      be added per-need from real findings, not a pre-planned
      roadmap.

---

## 2026-05-14 (sixteenth entry) — HTTPS fixture variant

### What's new since last cycle (fifteenth entry)
- New `fixtures/t76-detectors-https/` dir with self-signed
  cert + key (100-year validity, `CN=localhost`).
- New `fixtures/t76-detectors-https/serve.py` — HTTPS server
  on port 8773 with per-route response-header overrides.
- New `journeys/t76-detector-fixtures-https.json` — 9 routes
  exercising hsts (4) + xFrameOptions (3) + mixedContent (2).
- New `scripts/check-t76-https-detectors.sh` — gate that
  spins the HTTPS fixture, sets `CRAWLER_IGNORE_HTTPS_ERRORS=1`
  + `CRAWLER_DISABLE_LOCALHOST_EXEMPTION=1`, runs the audit,
  asserts each route's expected findings.

### Why a separate fixture

Three detectors (mixedContent, hsts, xFrameOptions) short-
circuit on http or localhost. The existing http fixture can't
exercise them. Unit-tested but never live-validated. This
fixture closes that gap.

### Two opt-in env vars

Both default to OFF; production audits must NEVER set either:

  - `CRAWLER_IGNORE_HTTPS_ERRORS=1` — Playwright trusts the
    fixture's self-signed cert. Without this, the navigation
    fails at the cert check.
  - `CRAWLER_DISABLE_LOCALHOST_EXEMPTION=1` — flips the
    snapshot's `pageIsLocalhost` to false in main.ts, so the
    detectors actually fire on 127.0.0.1. Without this, the
    detectors' built-in localhost exemption masks every
    finding.

The HTTPS check-script wrapper sets both before invoking the
audit. Production never goes through this code path.

### What the script-development loop caught

A subtle bash bug — `set -e` + `curl ... 2>/dev/null | grep -q`
caused the wait loop's first iteration to silently kill the
script. Refactored to var-capture: `code=$(curl -ks ... || true)`
+ `[ "$code" = "200" ]`. Then a SECOND issue: curl was exiting
non-zero AFTER printing %{http_code}=200 (TLS body-read flake
on the self-signed cert). Wrapped with `|| true` to swallow
the exit code; the printed status code is what we trust.

### Result

**9/9 routes PASS.** Every HTTPS-detector finding kind fires:
hsts.missing / max-age-too-short / no-subdomains,
frame-options.missing / allowall / invalid, mixed-content.active
/ passive. All three detectors now have unit + live coverage.

The existing HTTP gate (33/33) still PASSes. SkillShots audit
still ALL 32 AXES SILENT.

### Action items

- [ ] Add referrerPolicy detector (third response-header
      consumer) and exercise it via the new HTTPS fixture.
- [ ] login-flow fixture (still queued).
- [ ] Remaining roadmap: `fontLoading`.

---

## 2026-05-14 (fifteenth entry) — second response-header detector

### What's new since last cycle (fourteenth entry)
- `xFrameOptions` detector landed. Same shape as `hsts`.
- Total active detector axes: 25 (was 24).
- main.ts: second consumer of `topLevelResponseHeaders` Map.

### Why a second response-header detector

The first one (hsts, last cycle) added the capture path —
extending the existing `page.on('response')` listener to stash
full headers for navigation responses. The second one validates
the path is consumable: a future contributor can ship a third
header-flavoured detector by writing ~100 lines of detector code
without touching main.ts's listener, capture, or storage shape.

`xFrameOptions` is a near-clone of `hsts` shape:
- `build<Name>Snapshot(url, headers)` — pure function
- Localhost / http exemption (same)
- `detect<Name>Issues(snapshot)` — pure function
- 3 finding kinds (one strict, two warn) instead of hsts's 3
- Reads from same `topLevelResponseHeaders` Map

### Detector design

Real-world: clickjacking is a top web vuln. The page's framing
policy decides whether other origins can iframe it. Two headers
control this:

  - X-Frame-Options (legacy): DENY / SAMEORIGIN / ALLOW-FROM <uri>
  - Content-Security-Policy: frame-ancestors ... (modern,
    supersedes XFO when present)

Either one with a non-wildcard value protects the page. The
detector requires at least one. Findings:

  - frame-options.missing     strict   neither header (or
                                       both empty)
  - frame-options.allowall    warn     CSP frame-ancestors '*'
                                       (or XFO ALLOW-FROM *) —
                                       intentional but
                                       worth-confirming open
  - frame-options.invalid     warn     XFO value not in the
                                       3-token vocabulary

  14 unit tests cover all paths.

### SkillShots dogfood

**0 findings** — dev server runs on http://127.0.0.1; localhost
exempt. Same scope as hsts.

### Re-audit result

**ALL 32 DETECTION AXES SILENT** on SkillShots (was 31; +1 axis).
Liveness gate (33/33) still PASS.

### Pattern note

The two response-header detectors share a pattern:

  build<Name>Snapshot(pageUrl, headers) → snapshot
  detect<Name>Issues(snapshot) → findings

If a third lands, ~70% structural overlap is candidate for a
generic `headerDetector(headerName, parser, classifier)` helper.
For two implementations, duplication is fine.

### Action items

- [ ] HTTPS fixture variant covers all three header-flavoured
      detectors at once.
- [ ] referrerPolicy detector (third consumer of capture path).
- [ ] login-flow fixture (still queued).
- [ ] Remaining roadmap: `fontLoading`.

---

## 2026-05-14 (thirteenth entry) — second aggregates detector

### What's new since last cycle (twelfth entry)
- `crossPageMetaDescription` detector landed — same shape as
  `crossPageTitle`, validates the aggregates pattern.
- Total active detector axes: 23 (was 22).
- main.ts aggregates pass now runs TWO detectors.

### Why a second aggregates detector

The first one (crossPageTitle, last cycle) established the
pattern. The second one tests whether the pattern generalises:
can a future contributor follow the same template and ship
correctly?

`crossPageMetaDescription` is a near-clone of `crossPageTitle`:
same accumulator type, same record function shape, same detect
shape, same piggyback approach. The only differences:
- Field name (title → description)
- Truncation in the detail (descriptions are 50-160 chars,
  truncated to 80 + ellipsis for readability)
- Different SEO rationale in the detail text

This validates that the pattern is teachable. If a third
aggregates detector lands, the ~80% structural overlap is a
candidate for a generic `dupGroupDetector(acc, kind, label)`
helper — but for two implementations, duplication is fine.

### SkillShots dogfood

**0 findings** on the SkillShots journey — every page has both
a unique title AND a unique description. The PlausiDen-Forge
typed CMS does this correctly (inject_seo.py + cms/*.json).

### Fixture verification

The t76-detector-fixtures journey uses TWO different default
description strings depending on which path through the
fixture-page-builder a route takes:
- `CLEAN_HEAD` constant ("...fits **in** the search-result...") on
  routes using head_override = CLEAN_HEAD
- `head()` function default ("...fits the search-result...") on
  routes using the head() builder

These are intended to be the same — a one-char drift, "in" vs
no-"in", landed silently. The fixture's `crossPageMetaDescription`
audit catches BOTH groups:
- 20 URLs share the CLEAN_HEAD variant
- 9 URLs share the head() variant

So the fixture self-audits the inconsistency in its own
defaults. The detector is alive end-to-end.

### Action items

- [ ] Fix the fixture's two-default-description drift (cosmetic;
      either pick one or document the two intentional groups).
- [ ] HTTPS fixture variant for mixedContent live integration.
- [ ] login-flow fixture.
- [ ] Remaining roadmap: `fontLoading`, `hstsHeader`,
      `xFrameOptions`.

---

*Update this file whenever a fresh dogfood run lands. Each entry
should record what was new, what the site exposed in the crawler,
and what gaps remain — turning the dogfood loop into a public
record of detection coverage over time.*
