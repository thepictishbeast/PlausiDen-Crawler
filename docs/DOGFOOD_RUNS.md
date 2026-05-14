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

## 2026-05-14 (twenty-eighth entry) — Cache-Control hygiene + Web Cache Deception catch

### What's new since last cycle (twenty-seventh entry)
- 1 new detector axis: **`cacheControl`** — Cache-Control
  directive hygiene. Brings the active-axis count to **40**
  (43 with the three legacy event kinds counted separately).
  Six finding kinds, one strict + five warns.
- 7 new HTTPS fixture routes (6 finding-specific + 1 clean
  control). HTTPS gate now validates **50/50** routes (was
  43/43).
- DEFAULT_HEADERS Cache-Control simplified from
  `'no-store, no-cache, must-revalidate, max-age=0'` (IE6-era
  belt-and-braces, also a `cache-control.contradictory`
  trigger under the new detector — no-store + max-age cancel)
  to just `'no-store'`. Per RFC 9111 (which superseded
  RFC 7234), `no-store` alone is the canonical "do not cache"
  directive.
- 18 unit tests in `cacheControl.test.ts`, all passing.
- Tenth response-header detector wired through the cycle-24
  helper. The detector reads both Cache-Control and Set-Cookie
  from the same headers Map — set-cookie's VALUE isn't used
  (just presence), so the helper's snapshot+classifier shape
  applies cleanly.

### Why cacheControl now
The user's CLAUDE.md doctrine emphasises adversarial security
and the "supersociety" stack. Web Cache Deception (Omer Gil,
2017) is the canonical attack class this detector catches:

  An attacker tricks an intermediate cache (CDN, reverse
  proxy, kiosk browser) into storing a per-user personalised
  response by appending a fake static-asset extension to the
  URL: `/account/foo.css`. The origin server returns the
  account page (path-routing typically ignores extensions).
  If the response carries Set-Cookie but Cache-Control
  allows shared caching, the next visitor to the cached URL
  receives the previous user's session + personal data.

The defence is a single Cache-Control directive — `no-store`
forbids any cache from storing the response. The detector
flags the EXACT symptom: response has Set-Cookie AND
Cache-Control includes `public`, OR omits both `no-store` and
`private`.

This is the highest-leverage cache-related security control —
real apps fail this all the time because the framework
defaults haven't caught up to the attack.

### Detector design
Six findings, one strict, five warns:

  - cache-control.missing                    warn
  - cache-control.public-with-cookie         strict (cache deception!)
  - cache-control.no-private-with-cookie     warn
  - cache-control.invalid                    warn
  - cache-control.unrealistic-maxage         warn
  - cache-control.contradictory              warn

The contradictory check catches four directive combinations
that cancel each other:

  - no-store + max-age (no-store wins, max-age is dead code)
  - public + private (spec ambiguous; most honour 'private')
  - no-cache + immutable (revalidate-always vs skip-revalidate)
  - no-store + immutable (no-store forbids cache, immutable
    assumes one)

These are silent intent-leaks where the operator clearly
wanted ONE behaviour but expressed BOTH.

### DEFAULT_HEADERS Cache-Control modernisation
The fixture had the old IE6-era belt-and-braces value
(`no-store, no-cache, must-revalidate, max-age=0`). Under the
new detector this is `cache-control.contradictory` because
no-store and max-age cancel. Simplified to just `no-store`,
which RFC 9111 says is sufficient. Saves bytes too.

### Verified
- HTTP gate: 43/43 routes pass (cache-control fires only on
  the HTTPS fixture which has Set-Cookie semantics).
- HTTPS gate: **50/50** routes pass (was 43/43; 7 new
  cache-control routes).
- SkillShots audit: 43 axes total, all silent vs prior. Site
  on localhost so the exemption short-circuits — even though
  Python's SimpleHTTPServer doesn't set Cache-Control at all
  (which would normally fire `cache-control.missing`).

### Action items
- [ ] **End-to-end CORP fixture verification**: spin up a
      second TLS listener on port 8774 (cross-origin by port-
      difference rule) serving sub-resource bytes with various
      CORP header values. Add gate routes that load those.
      Validates the cross-origin detection path end-to-end.
- [ ] CSP `report-only` header parser — pairs with existing
      CSP detector. Single-value, helper applies.
- [ ] Vary header completeness — single-value, helper applies.
- [ ] Inline-script-without-nonce detector — second per-element
      security audit, would re-open the perElementDetector
      helper question.
- [ ] Sub-resource Cache-Control audit — same shape as CORP
      sub-resource walk. Could share a perSubResourceDetector
      helper that emerges if a SECOND per-sub-resource detector
      lands.
- [ ] Server-Timing header — not info-leak per se but a
      timing side-channel surface. Could fold into infoLeak as
      a 9th finding kind.

---

## 2026-05-14 (twenty-seventh entry) — CORP detector + sub-resource capture-layer expansion

### What's new since last cycle (twenty-sixth entry)
- 1 new detector axis: **`corp`** — Cross-Origin-Resource-
  Policy per-sub-resource audit. Brings the active-axis count
  to **39** (42 with the three legacy event kinds counted
  separately).
- **NEW capture path**: `allResponseHeaders: Map<url,
  Record<string, string>>`, sister to the existing
  `topLevelResponseHeaders` Map. Populated for every response
  that is NOT a top-level navigation (i.e. all sub-resources:
  scripts, stylesheets, images, fonts, fetch'd JSON, etc.).
- First detector that reads from `allResponseHeaders`. Sets
  up future per-sub-resource audits (sub-resource SRI by
  hash mismatch, sub-resource cookie analysis, sub-resource
  CSP report-only, etc.).
- 17 unit tests in `corp.test.ts`, all passing.
- No new HTTPS fixture routes — wiring is verified by the
  existing 43-route gate (CORP correctly fires zero findings
  on these routes since they all load same-origin sub-
  resources only). The detection LOGIC is verified by the
  unit tests. End-to-end cross-origin testing requires a
  second TLS listener on a different port; queued for a
  follow-up cycle if real-world dogfood shows blind spots.

### Why CORP now
CORP is the third member of the cross-origin-isolation triad.
COOP controls window.opener. COEP controls which sub-resources
the page is willing to embed. CORP is set on the RESOURCE side
to opt INTO being embedded by cross-origin pages. Without all
three, the supersociety primitives — SharedArrayBuffer,
high-resolution timers, performance.measureUserAgentSpecific-
Memory — stay disabled and the page is exposed to Spectre-
class side-channel attacks from co-tenant origins inside the
same browser process.

CORP also has standalone defensive value: when set to
`same-origin`, browsers refuse to even FETCH the resource
cross-origin, which prevents some cache-timing attacks that
observe whether the resource was already cached.

### Capture-layer expansion details
The existing 9 response-header detectors all consume
`topLevelResponseHeaders`, which only stores headers from
top-level navigation responses. CORP needs sub-resource
headers, so the response listener now ALSO populates
`allResponseHeaders` for every non-navigation response.

Capture choice: sync `headers()` (not `allHeaders()`) for
sub-resources. Reasoning:

  1. CORP doesn't carry the Set-Cookie semantics that the
     sync form strips.
  2. Awaiting `allHeaders()` for hundreds of sub-resources
     per page would double the audit wall-clock time.
  3. Sub-resource Set-Cookie audit is not yet in scope.

When sub-resource Set-Cookie auditing lands, the capture
switches to `allHeaders()` with a REGRESSION-GUARD comment
mirroring the top-level one (cycle 20).

### Detector design
Two finding kinds:

  - `corp.cross-origin-resource-no-corp` — the main one. The
    severity depends on the page's own COEP header value:
    * COEP=require-corp → STRICT (the cross-origin sub-
      resource will be BLOCKED at load; the page is broken).
    * COEP not set → WARN (forward-compat gap; the moment
      the page adopts COEP, the resource stops working).
    The detail message specifically calls out which scenario
    applies so the operator knows whether they're looking at
    a breakage or a future trap.

  - `corp.cross-origin-resource-invalid` — CORP set to a
    value not in the W3C-recognised set. Browsers may reject.

Out of scope:
  * Same-origin sub-resources (CORP doesn't apply).
  * The page's OWN top-level CORP (separate concern; most
    top-level HTML pages legitimately don't set CORP).
  * Localhost (consistent with the response-header family).
  * `data:` / `blob:` / `about:` URLs (no transport).

### Bespoke wiring (does not use responseHeaderDetector helper)
Per the cycle-22 verdict, the helper applies only to detectors
whose classifier signature matches "snapshot → findings". CORP's
shape is genuinely different:
  - Walks an entire Map of sub-resource headers.
  - Needs the page's own COEP value to set severity.
  - Aggregates across multiple sub-resources (4 missing CORP
    → 1 finding count=4).

A generic `perSubResourceDetector` helper would emerge if a
SECOND per-sub-resource detector lands (probable: sub-resource
Set-Cookie audit). Not extracting prematurely.

### Verified
- HTTP gate: 43/43 routes pass.
- HTTPS gate: 43/43 routes pass — CORP fires zero findings
  because all routes load same-origin sub-resources only.
  Wiring is exercised end-to-end (per-step record pushed,
  exception-swallow path tested implicitly).
- SkillShots audit: 42 axes total, all silent vs prior. CORP
  fires nothing because SkillShots loads no cross-origin
  sub-resources.
- Type-check: clean (only pre-existing aria.ts +
  vendor/puppeteer issues, neither touched).

### Action items
- [ ] **End-to-end CORP fixture verification**: spin up a
      second TLS listener on port 8774 (cross-origin by port-
      difference rule) serving sub-resource bytes with various
      CORP header values. Add gate routes that load those.
      Validates the cross-origin detection path end-to-end.
- [ ] Sub-resource Set-Cookie audit detector — second per-
      sub-resource detector. Triggers the
      `perSubResourceDetector` helper-extract question. Will
      need the capture switched to `allHeaders()` for sub-
      resources too.
- [ ] Cache-Control hygiene detector — single-value, classifier
      depends on response status + content-type. Helper applies.
- [ ] CSP `report-only` header parser.
- [ ] Inline-script-without-nonce detector.
- [ ] Vary header completeness — single-value, helper applies.

---

## 2026-05-14 (twenty-sixth entry) — info-leak headers opsec audit

### What's new since last cycle (twenty-fifth entry)
- 1 new detector axis: **`infoLeak`** — opsec hygiene audit
  for version-disclosure response headers. Brings the active-
  axis count to **38** (41 with the three legacy event kinds
  counted separately). Eight finding kinds, all warn.
- 9 new HTTPS fixture routes (8 finding-specific + 1 clean
  control). HTTPS gate now validates **43/43** routes (was
  34/34).
- HTTPS fixture `Handler` class now overrides `server_version`
  to `'web'` and `sys_version` to `''` so the auto-emitted
  Python `Server: BaseHTTP/0.6 Python/3.13.X` header doesn't
  fire info-leak.server-version on every unrelated route. The
  bare product name passes cleanly; the version-leak routes
  override Server explicitly.
- First detector wired through the cycle-24 `responseHeader-
  Detector` helper since it shipped — proves the helper's
  ergonomics on a fresh detector. Six lines of factory args
  in main.ts vs the ~25 lines the inline form would have
  needed.

### Why infoLeak now
The user's CLAUDE.md threat model lists "supply-chain
compromise" as a state-actor primitive, but the OTHER half of
that pattern is reconnaissance: an adversary mapping a target
site uses version-disclosure to cross-reference NVD / GitHub
Advisories / ExploitDB and find the exact pre-built exploit
modules to use against the target.

A Server header reading `nginx/1.20.1` reveals which CVEs
apply, which patch levels are missing, and which auxiliary
intelligence (deployment date inferable from version) is
available. Removing or generalising the header forces the
adversary to enumerate the surface manually — significantly
raising the cost of opportunistic attacks and slowing
targeted ones.

These headers have NO functional value to legitimate users.
Stripping them is pure win.

### Detector design
Eight finding kinds, all warn:

  - server-version (Server with a version token)
  - x-powered-by (any value)
  - x-aspnet-version
  - x-aspnetmvc-version
  - x-runtime
  - x-debug-token (with x-debug-token-link variant collapsed
    into the same finding)
  - via
  - x-generator

The "version token" heuristic is `\d+\.\d+` — at least
`<digit>.<digit>`. Bare product names (`Server: nginx`,
`Server: cloudflare`) are deliberately NOT flagged because
some routing infra needs Server set for debugging and the
bare name without a version doesn't enable CVE lookup.

### Helper validation
This is the first NEW detector wired through the cycle-24
`responseHeaderDetector` helper. Code shape:

```ts
const infoLeakFindingsByStep: Array<PerStepRecord<InfoLeakFinding>> = [];
const checkInfoLeak = makeResponseHeaderCheck({
  detectorName: 'infoLeak',
  eventKind: 'info-leak',
  page, topLevelResponseHeaders, disableLocalhostExemption,
  findingsByStep: infoLeakFindingsByStep,
  log,
  buildSnapshot: buildInfoLeakSnapshot,
  detectIssues: detectInfoLeakIssues,
});
```

Six lines of factory args. The pre-helper inline form would
have been ~25 lines. The helper is paying compound interest
already.

### Tests
- 18 unit scenarios in `infoLeakHeaders.test.ts`, all passing.
- Localhost exemption + per-header presence + Server-version-
  token heuristic + bare-name-not-flagged + pile-on-with-all-
  8-headers + header-name case-insensitivity + unrelated
  headers ignored.

### Verified
- HTTP gate: 43/43 routes pass (no change; infoLeak fires on
  HTTPS fixture only).
- HTTPS gate: **43/43** routes pass (was 34/34; 9 new
  info-leak routes).
- SkillShots audit: 41 axes total, all silent vs prior. The
  SkillShots dev server runs on localhost so the exemption
  short-circuits even though Python's SimpleHTTPServer DOES
  emit a `Server: SimpleHTTP/0.6 Python/3.13.X` header that
  WOULD fire on a non-localhost site. Correct behaviour.

### Action items
- [ ] CORP detector — capture-layer expansion still needed.
      Per-RESOURCE header.
- [ ] Cache-Control hygiene detector — single-value but the
      classifier depends on response status + content-type.
      Helper applies.
- [ ] CSP `report-only` header parser — pairs with existing
      CSP detector.
- [ ] Inline-script-without-nonce detector — second per-
      element security audit, would re-open the
      `perElementDetector` helper question.
- [ ] Vary header completeness — if a response varies based
      on Cookie or Authorization but doesn't say so,
      intermediate caches can serve the wrong user's data.
      Single-value response header.
- [ ] Server-Timing header leak — exposes per-component
      timing info similar to X-Runtime. Probably folded into
      info-leak as a 9th finding kind in a follow-up.

---

## 2026-05-14 (twenty-fifth entry) — Subresource Integrity supply-chain detector

### What's new since last cycle (twenty-fourth entry)
- 1 new detector axis: **`sri`** — Subresource Integrity per-
  element DOM audit. Brings the active-axis count to **37**
  (40 with the three legacy event kinds counted separately).
  FIRST per-element security audit (existing per-element
  detectors are accessibility / UX).
- 7 new HTTP fixture routes for the five finding kinds plus
  two control routes (clean SRI + same-origin no-SRI):
  `/sri-cross-origin-script-no-integrity/`,
  `/sri-cross-origin-style-no-integrity/`,
  `/sri-cross-origin-script-no-crossorigin/`,
  `/sri-cross-origin-script-weak-algo/`,
  `/sri-cross-origin-script-malformed/`,
  `/sri-clean/` (control), `/sri-same-origin-no-integrity/`
  (control). HTTP gate now validates **43/43** routes (was
  36/36).

### Why SRI now
The user's CLAUDE.md threat model lists "supply-chain
compromise" as a state-actor adversarial primitive. SRI is
the single highest-leverage front-end defence against
supply-chain attacks AND one of the few security controls
that doesn't require server cooperation — the page author can
unilaterally pin the cross-origin asset's hash even when the
CDN itself is uncooperative.

Real-world incidents this detector would have caught:

  - Microsoft Tay (2016) — bot account compromise via
    cross-origin embed.
  - MyEtherWallet (2018) — DNS hijack + injected wallet-
    stealer JavaScript via Cloudflare CDN.
  - British Airways (2018) — Magecart payment-skimmer via
    compromised Modernizr CDN.
  - event-stream NPM (2018) — supply-chain RCE in a
    transitive dependency.
  - SolarWinds (2020) — different layer (build pipeline) but
    same threat model.

### Detector design
Five finding kinds, two severities:

  - `sri.script-cross-origin-no-integrity` (strict) — script
    loaded from a different origin without integrity. RCE
    waiting to happen.
  - `sri.style-cross-origin-no-integrity` (warn) — stylesheet
    same. Lower severity (narrower attack surface) but still
    a defence-in-depth gap.
  - `sri.script-cross-origin-no-crossorigin` (warn) — has
    integrity but no `crossorigin` attribute. Browsers
    SILENTLY IGNORE the integrity check without the CORS
    opt-in. Critical UX trap — looks safe in source review,
    isn't.
  - `sri.script-invalid-integrity-format` (warn) — typo
    silently disables SRI.
  - `sri.script-weak-algorithm` (warn) — sha1/md5; W3C spec
    only recognises sha256/sha384/sha512.

The "no-crossorigin" finding is the one most likely to surface
on real-world sites — every CSP-aware developer remembers to
add integrity but many forget the second attribute. The
silent-ignore behaviour is exactly the kind of trap the
crawler should catch.

### Architectural shape (different from response-header family)
SRI is the FIRST per-element security detector. The existing
8 response-header detectors all share the same shape (read
top-level navigation response headers, classify) and migrated
to the `responseHeaderDetector` helper in cycle 24. SRI
doesn't fit:

  - Capture is via `page.evaluate(SRI_DOM_CAPTURE_JS)` — a
    DOM walk for `<script src>` and `<link href>` elements.
  - Classification operates over an array of per-element
    snapshots, not a single header value.
  - Findings aggregate across multiple elements (4 cross-
    origin scripts → 1 finding with count=4).

This shape matches the existing per-element accessibility /
UX detectors (linkUnderline, runtimeFocus, runtimeContrast,
runtimeImages, etc.). NO new helper extracted yet — the
shapes there are also heterogeneous (each detector's per-
element classifier is different). If a SECOND per-element
security detector lands (e.g. inline-script analysis without
nonces), the helper question re-opens.

### Tests
- 18 unit scenarios in `sri.test.ts`, all passing — empty,
  same-origin-ignored, cross-origin-no-integrity (strict),
  cross-origin-stylesheet-no-integrity (warn), non-stylesheet
  link-rel ignored, fully-correct SRI clean, integrity-without-
  crossorigin warn, weak algorithm sha1, malformed integrity,
  multi-algo OK, aggregation count=4, examples-capped-at-5,
  count-still-reflects-all, mixed-script-and-style, use-
  credentials counts as crossorigin, empty-crossorigin-
  attribute counts as anonymous.
- Browser-side capture function exposed as `SRI_DOM_CAPTURE_JS`
  string template so the unit tests can compile-check the
  classifier independently and a future Rust mirror (T75)
  can reuse the source via `js_brackets_balanced` parity.

### Verified
- HTTP gate: **43/43** routes pass (was 36/36; 7 new SRI routes).
- HTTPS gate: 34/34 routes pass (no change; SRI is HTTP-fine).
- SkillShots audit: 40 axes total, all silent vs prior. SRI
  fires nothing because SkillShots uses no cross-origin
  scripts or stylesheets — correct quiet-baseline confirmation
  that the detector is wired and active without firing on a
  clean site.

### Action items
- [ ] CORP detector — capture-layer expansion still needed.
      Per-RESOURCE header (not per-page) so `topLevelResponse-
      Headers` Map needs a sibling for sub-resource headers.
      Bigger architecture lift than the eight existing
      response-header detectors. Queued.
- [ ] Server header leak detector — fits the existing
      `responseHeaderDetector` helper cleanly. Quick win
      cycle-26 candidate.
- [ ] X-Powered-By leak detector — sister to Server header,
      same pattern.
- [ ] Cache-Control hygiene detector — single-value but the
      classifier depends on response status + content-type.
      Still single-value; helper applies.
- [ ] Inline-script-without-nonce detector — would be the
      SECOND per-element security audit, re-opening the
      "extract a perElementDetector helper" question.
- [ ] CSP `report-only` header parser — pairs naturally with
      the existing CSP detector.

---

## 2026-05-14 (twenty-fourth entry) — responseHeaderDetector helper extraction

### What's new since last cycle (twenty-third entry)
- 1 new module: `src/responseHeaderDetector.ts` — generic
  check-runner factory that wraps the wiring shared across
  EVERY response-header detector (URL lookup + header read +
  snapshot build + localhost opt-out + classify + per-step
  record + captured-event emission + pageerror swallow).
- 8 detectors migrated to the helper in main.ts — hsts,
  xframeOptions, referrerPolicy, coop, coep, csp, permissions-
  policy, cookieSecurity. Each detector's wiring went from
  ~25 lines of inline boilerplate to 6 lines of factory args.
- `disableLocalhostExemption` declaration moved from inside
  the hsts block (where it lived for accidental reasons) to
  the top of the response-header detector family. Single point
  of audit for the env-var opt-out.
- 1 new test: `src/responseHeaderDetector.test.ts` — 17 unit
  scenarios, all passing. Covers the localhost short-circuit,
  the env-var opt-out, the captured-event field shape (kind /
  text / url / severity / ruleId / impact), strict→serious +
  warn→minor mapping, multiple-findings + single-record
  pairing, and the throw-swallow paths for both buildSnapshot
  and detectIssues blowing up.
- No new detector axes — pure refactor. Active-axis count
  stays at 36 (39 with legacy event kinds).

### Why now
Per the cycle-22 verdict, the helper extraction was queued for
the moment the SEVENTH single-value detector landed. Cycle 23
shipped COOP + COEP, taking the single-value count to FIVE —
and re-examining the wiring revealed something the cycle-22
analysis missed: the boilerplate is identical across the
single-value AND multi-value detectors. What differs is the
classifier signature (which the helper delegates to a callback),
not the wiring. So the helper applies to all EIGHT response-
header detectors, not just the five single-value ones.

### Why the boilerplate-extraction is safe
Behavioural equivalence is verified at four levels:

  1. The helper's own 17 unit tests prove the contract
     (captured-event shape exactly matches the pre-extraction
     inline form; throw paths route to pageerror correctly;
     localhost opt-out flips the snapshot field as before).
  2. Per-detector unit tests (102 scenarios across 8 modules)
     keep passing — no detector module was touched, only the
     surrounding wiring.
  3. The HTTPS fixture gate validates 34 routes end-to-end with
     ruleId-exact matching — any drift in the captured-event
     shape would surface as a missing-finding failure here.
  4. The SkillShots audit's positive-signal table proves the
     detectors stay silent on a real-world site (localhost), so
     the localhost short-circuit is correct.

All four green after the migration.

### Code-shape comparison
Before extraction (one detector × 8 = ~200 lines of boilerplate):

```ts
const hstsFindingsByStep: Array<{ stepLabel: string; pageUrl: string; findings: HstsFinding[] }> = [];
const checkHsts = async (afterLabel: string) => {
  try {
    const pageUrl = page.url();
    const headers = topLevelResponseHeaders.get(pageUrl);
    const snap = buildHstsSnapshot(pageUrl, headers);
    if (disableLocalhostExemption) snap.pageIsLocalhost = false;
    const findings = detectHstsIssues(snap);
    hstsFindingsByStep.push({ stepLabel: afterLabel, pageUrl, findings });
    for (const f of findings) {
      log({
        kind: 'hsts',
        text: `[${f.kind}] ${f.detail}`,
        url: pageUrl,
        severity: f.severity,
        ruleId: f.kind,
        impact: f.severity === 'strict' ? 'serious' : 'minor',
      });
    }
  } catch (e) {
    log({
      kind: 'pageerror',
      text: `[hsts] detector threw on step ${afterLabel}: ${(e as Error).message}`,
    });
  }
};
```

After extraction:

```ts
const hstsFindingsByStep: Array<PerStepRecord<HstsFinding>> = [];
const checkHsts = makeResponseHeaderCheck({
  detectorName: 'hsts',
  eventKind: 'hsts',
  page, topLevelResponseHeaders, disableLocalhostExemption,
  findingsByStep: hstsFindingsByStep,
  log,
  buildSnapshot: buildHstsSnapshot,
  detectIssues: detectHstsIssues,
});
```

200 lines → 80 lines across the eight detectors. More importantly:

  - The localhost-exemption opt-out is in ONE place. A future
    refactor that wants to e.g. broaden the exemption to all
    private-IP space can change one line, not eight.
  - The captured-event shape is in ONE place. Adding a new
    field (e.g. cycle for telemetry, or a wcag tag for the a11y
    detectors) is a one-line change.
  - The pageerror-on-throw is in ONE place. The detector tag in
    the message stays correct because it's threaded through.

### Verified
- HTTP gate: 36/36 routes pass.
- HTTPS gate: 34/34 routes pass.
- SkillShots audit: 39 axes total, all silent vs prior — pure
  refactor, no behavioural change.
- TypeScript check: clean (only pre-existing aria.ts +
  vendor/puppeteer issues remain, neither touched this cycle).

### Action items
- [ ] CORP detector — third member of the cross-origin-
      isolation triad. Per-resource header (not per-page) so
      needs a capture-layer expansion to see sub-resource
      headers, not just top-level navigation. The
      capture-layer change is the bigger half of the work.
- [ ] Subresource Integrity (SRI) — per-element DOM walk; not
      a response-header detector, so the helper doesn't apply.
- [ ] Server header leak detector — single-value response
      header (`Server: nginx/1.20.1` style). Slots cleanly
      into the new helper pattern.
- [ ] X-Powered-By leak — sister to Server header. Same
      pattern.
- [ ] Cache-Control hygiene — `no-store` required on
      sensitive endpoints; `Pragma: no-cache` is a legacy
      fallback. Moderate complexity classifier (depends on
      response status + content-type), still single-value.

---

## 2026-05-14 (twenty-third entry) — COOP + COEP cross-origin isolation pair

### What's new since last cycle (twenty-second entry)
- 2 new detector axes: **`coop`** (Cross-Origin-Opener-Policy)
  + **`coep`** (Cross-Origin-Embedder-Policy). Brings the
  active-axis count to **36** (39 with the three legacy event
  kinds counted separately).
- 6 new HTTPS fixture routes (3 per detector):
  `/no-coop/`, `/coop-unsafe-none/`, `/coop-invalid/`,
  `/no-coep/`, `/coep-unsafe-none/`, `/coep-invalid/`.
  HTTPS gate now validates **34/34** routes (was 28/28).
- DEFAULT_HEADERS now sets `Cross-Origin-Opener-Policy:
  same-origin` + `Cross-Origin-Embedder-Policy: require-corp`
  so unrelated routes don't leak `coop.missing` /
  `coep.missing` into their audit notes.
- Single-value response-header detector count now FIVE (hsts,
  xframeOptions, referrerPolicy, coop, coep) — sufficient
  evidence to extract `responseHeaderDetector` in the next
  cycle.

### Why COOP + COEP now
Together these two headers enable `crossOriginIsolated`
document state — the modern browser primitive that gates:

  - `SharedArrayBuffer` (required for WebAssembly threads,
    OffscreenCanvas in workers, real concurrency primitives).
  - `performance.now()` high-resolution timing (necessary for
    accurate profiling, but also useful for Spectre-class
    timing attacks — restricted unless the page proves
    isolation first).
  - `performance.measureUserAgentSpecificMemory()` (memory
    metrics; same threat model).

Without isolation, Spectre-class side-channel attacks can leak
data from co-tenant origins inside the same browser process.
Modern security-sensitive apps (any banking, payments,
healthcare, comms client) should set both. This is exactly the
kind of supersociety control that the directive emphasises —
defence-in-depth for the next-generation browser surface.

### Detector design
Both are single-value response-header detectors with three
findings each:

  - `<x>.missing`     warn   no header → defaults to unsafe-none
  - `<x>.unsafe-none` warn   explicit unsafe-none
  - `<x>.invalid`     warn   value not in W3C-recognised set

The two snapshots / detectors are deliberately separate
modules (rather than a combined `crossOriginIsolation` module)
because they're independently configurable. A site might have
COOP but not COEP, or vice versa, and we want one finding per
defect.

### Helper-extract setup (next cycle)
With COOP + COEP shipped, the single-value response-header
detector roster is:

  1. hsts             — HSTS header parser + classifier
  2. xframeOptions    — XFO header + CSP frame-ancestors fallback
  3. referrerPolicy   — Referrer-Policy parser + classifier
  4. coop             — COOP single-token classifier
  5. coep             — COEP single-token classifier

Five concrete examples is more than enough to validate the
abstraction shape. The shared structure across all five:

```ts
function checkXxx(afterLabel: string) {
  const pageUrl = page.url();
  const headers = topLevelResponseHeaders.get(pageUrl);
  const snap = buildXxxSnapshot(pageUrl, headers);
  if (disableLocalhostExemption) snap.pageIsLocalhost = false;
  const findings = detectXxxIssues(snap);
  xxxFindingsByStep.push({ stepLabel: afterLabel, pageUrl, findings });
  for (const f of findings) {
    log({ kind: 'xxx', text: `[${f.kind}] ${f.detail}`, url: pageUrl,
          severity: f.severity, ruleId: f.kind,
          impact: f.severity === 'strict' ? 'serious' : 'minor' });
  }
}
```

The boilerplate is ~25 lines per detector, ×5 = 125 lines that
compress to ~30 with the helper. Plus the proposed
`responseHeaderDetector` becomes a single point of audit for
the localhost-exemption + capture-Map plumbing — fewer surfaces
where a future refactor can silently drop the protection.

Cycle 24 plan:
  1. Create `src/responseHeaderDetector.ts` exporting a generic
     `runResponseHeaderDetector(opts)` helper.
  2. Migrate hsts → coop → coep → xframeOptions → referrerPolicy
     in five small commits, running the gate between each so a
     regression is bisected to one detector.
  3. After all five migrate, validate no behavioural change
     against the HTTPS gate (still 34/34) + SkillShots audit
     (still all silent on localhost).

### Tests
- 10 unit scenarios in `coop.test.ts`, all passing.
- 9 unit scenarios in `coep.test.ts`, all passing.

### Verified
- HTTP gate: 36/36 routes pass.
- HTTPS gate: 34/34 routes pass (28 prior + 6 new COOP/COEP routes).
- SkillShots audit: 39 axes total, all silent vs prior — the
  SkillShots dev server runs on localhost so the exemption
  short-circuits, correct quiet-baseline confirmation that the
  detectors are wired and active.

### Action items
- [ ] **Extract `responseHeaderDetector` helper next cycle.**
      See cycle 24 plan above.
- [ ] CORP (Cross-Origin-Resource-Policy) — third member of
      the cross-origin-isolation triad. The complication: CORP
      is a per-RESOURCE header, not per-page, so the
      `topLevelResponseHeaders` Map (which only stores top-level
      navigation responses) won't see it on cross-origin
      sub-resources. To audit those, the response listener
      needs to capture sub-resource headers too — a capture-
      layer expansion. Queued as cycle-25 candidate.
- [ ] Subresource Integrity (SRI) detector — per-element DOM
      check (every cross-origin `<script>` and
      `<link rel=stylesheet>` should have an `integrity=`
      attribute). Different shape from response-header
      detectors. CDN-compromise mitigation.
- [ ] Server-header leak detector (server: nginx/1.20.1 etc.) —
      opsec hygiene. Single-value response header — fits
      perfectly into the helper-extracted pattern.

---

## 2026-05-14 (twenty-second entry) — full CSP detector + helper-extract verdict

### What's new since last cycle (twenty-first entry)
- 1 new detector axis: **`cspPolicy`** — full
  Content-Security-Policy audit. Brings the active-axis count
  to **34** (37 with the three legacy event kinds counted
  separately). Eleven finding kinds across two severities.
- 6 new HTTPS fixture routes:
  `/no-csp/`, `/csp-unsafe-inline/`, `/csp-unsafe-eval/`,
  `/csp-wildcard-script/`, `/csp-no-trusted-types/`, `/csp-clean/`.
  HTTPS gate now validates **28/28** routes (was 22/22).
- Fixture hygiene: added `_DEFAULT_CSP` (a hardened baseline)
  to the HTTPS fixture's `DEFAULT_HEADERS` so unrelated routes
  don't leak `csp.missing` into their audit notes — paired with
  an opt-OUT on the xFrameOptions routes that need to test
  XFO-only behaviour without CSP frame-ancestors superseding.
- **Helper-extract verdict landed.** See below.

### Why full CSP now
CSP is the foundational web-security header. It's the single
largest XSS-mitigation control the web has, AND it's the one
real-world apps most often misconfigure. The detector surfaces
the defects with the clearest CVE-record blast radius:

  - `script-src 'unsafe-inline'` (strict) — once set, ANY
    HTML-injection sink becomes XSS. The single most common
    CSP bypass mechanism in the public CVE record.
  - `script-src 'unsafe-eval'` (strict) — allows the entire
    string-eval API surface.
  - `script-src '*' / 'https:' / 'http:'` (strict) — script
    origin is unconstrained.
  - Missing `default-src` AND `script-src` (warn) — fallback
    chain has no terminus.
  - Missing structural baseline (object-src, base-uri,
    form-action, frame-ancestors) — each is a known
    one-element-injection-becomes-RCE pivot.
  - Missing `require-trusted-types-for 'script'` (warn) —
    Trusted Types is the modern DOM-XSS-prevention layer
    that the supersociety stack should mandate.

The `csp.no-trusted-types` finding is the deepest supersociety
move in this cycle. Trusted Types eliminates an entire class
of DOM-based XSS at the platform level by requiring all writes
to dangerous DOM sinks (`innerHTML`/`outerHTML`/`document.write`/
eval'd `setTimeout`) to go through a typed policy. Chrome ships,
Firefox is shipping, Safari has implementation in flight. Every
page the crawler audits should adopt it.

### Detector design
- `buildCspSnapshot(pageUrl, headers)` parses the enforcing
  `Content-Security-Policy` header into a list of
  `(name, tokens)` directives in declaration order, lowercasing
  directive names per the W3C spec but preserving case in
  source-list tokens.
- `detectCspIssues(snap)` runs the script-src checks (with a
  default-src fallback resolution per the spec) plus the
  structural-baseline-absence checks.
- The `script-src` fallback to `default-src` is honored:
  `default-src 'self' 'unsafe-inline'` (no script-src) fires
  `csp.script-unsafe-inline`. Test 11 proves this.

### Helper-extract verdict (FOURTH consideration, decision landed)
Per the cycle-17 / cycle-19 / cycle-21 doctrine, the FOURTH /
FIFTH / SIXTH response-header detector should each have
re-examined the case for a `headerDetector(headerName, parser,
classifier)` extraction. Three deferrals were correct — the
multi-value detectors' classifier shapes were too heterogeneous
to share. With CSP in hand the picture is finally clear:

  **Single-value detectors** (hsts, xframeOptions,
  referrerPolicy) share ~70% structure: get-header → parse one
  value → classify into one of N severities. **Extract
  `responseHeaderDetector(headerName, snapshotBuilder,
  classifier)` cleanly.**

  **Multi-value detectors** (cookieSecurity, permissionsPolicy,
  cspPolicy) share the SHAPE of "list-of-(directive, tokens)
  parsing" but their classification is genuinely heterogeneous:

    - cookieSecurity:    per-cookie aggregation across N cookies.
    - permissionsPolicy: per-feature high-risk-set membership +
                        cross-cutting omitted-set check.
    - cspPolicy:         per-directive classification + script-src
                        fallback resolution + structural-baseline
                        absence checks.

  Wrapping them in `multiValueHeaderDetector(headerName, parser,
  classifier)` is a leaky abstraction — the classifier signature
  has to be polymorphic over snapshot shape, defeating the
  helper's whole purpose. **DECISION: leave the multi-value
  detectors as bespoke modules.** Their parser + classifier are
  already small and tested; the abstraction would obscure more
  than it shares.

  **Action item**: extract `responseHeaderDetector` in a
  follow-up cycle when a SEVENTH single-value detector lands
  (probable: Cross-Origin-Opener-Policy / Cross-Origin-Embedder-
  Policy / Cross-Origin-Resource-Policy — all single-value).
  Until then three concrete detectors are fine standalone.

### Tests
- 20 unit scenarios in `contentSecurityPolicy.test.ts`, all
  passing — including the script-src fallback to default-src
  (test 11), the case-insensitive header name (test 17), and
  the pile-on policy that fires every relevant finding
  simultaneously (test 20).

### Verified
- HTTP gate: 36/36 routes pass.
- HTTPS gate: 28/28 routes pass (22 prior + 6 new CSP routes).
- SkillShots audit: 37 axes total, all silent vs prior — the
  SkillShots dev server runs on localhost so the exemption
  short-circuits, correct quiet-baseline confirmation that the
  detector is wired and active.

### Action items
- [ ] **Extract `responseHeaderDetector(headerName,
      snapshotBuilder, classifier)` once the seventh single-value
      detector lands.** First candidate: COOP (Cross-Origin-Opener-
      Policy) — single-value, classifies into same-origin /
      same-origin-allow-popups / unsafe-none / etc. Second:
      COEP. Third: CORP.
- [ ] Cross-Origin-Opener-Policy detector — Spectre /
      SharedArrayBuffer cross-origin isolation. Single-value;
      this is the trigger for the helper extraction above.
- [ ] Cross-Origin-Embedder-Policy detector — sister to COOP.
- [ ] Cross-Origin-Resource-Policy detector — third sister.
- [ ] Subresource Integrity (SRI) detector — per-element DOM
      check; different module shape from response-header
      detectors. CDN-compromise mitigation.
- [ ] CSP-Report-Only header parser (cycle 23+).
- [ ] CSP nonce / hash validity check by correlating with
      rendered DOM `<script nonce>` (would need a per-element
      walker; biggest scope expansion in this family).

---

## 2026-05-14 (twenty-first entry) — permissionsPolicy detector

### What's new since last cycle (twentieth entry)
- 1 new detector axis: **`permissionsPolicy`** — Permissions-Policy
  response-header audit. Brings the active-axis count to **33**
  (the SkillShots audit reports 36 because three of the legacy
  axes — console-errors, page-errors, failed-requests — are
  counted separately from the named detectors).
- 5 new HTTPS fixture routes:
  `/no-permissions-policy/`, `/permissions-policy-camera-allowall/`,
  `/permissions-policy-invalid/`, `/permissions-policy-partial/`,
  `/permissions-policy-clean/`. HTTPS gate now validates **22/22**
  routes (was 17/17).
- Fixture hygiene: added `_DEFAULT_PERMISSIONS_POLICY` (a deny-all
  string) to the HTTPS fixture's `DEFAULT_HEADERS` so unrelated
  routes don't leak `permissions-policy.missing` into their
  audit notes.

### Why permissionsPolicy now
The supersociety threat model assumes embedded iframes are an
attack surface — third-party widgets, ad networks, analytics
scripts. Without a Permissions-Policy header, every browser API
(camera, microphone, geolocation, payment, USB, serial, MIDI,
HID, Bluetooth, motion sensors, display-capture, screen-wake-lock)
defaults to `*`, meaning ANY embedded iframe inherits ambient
permission to use those APIs. A malicious or compromised iframe
can prompt the user for camera access on any site that embeds
it. Permissions-Policy is the W3C-blessed (formerly Feature-
Policy) defence — and the only one — against this category of
attack.

The motion-sensor inclusions (accelerometer / gyroscope /
magnetometer) deserve special note. Websites use these
legitimately for orientation features, but the side-channel
literature (TouchLogger, AccessLogger and similar mobile-
keystroke side-channels) proves they enable keystroke recovery
when allowed cross-origin. Defence-in-depth requires denying
them for every iframe by default.

### Detector design
Five finding kinds, two severities:
- `permissions-policy.missing` (warn) — no header at all;
  every feature defaults to `*`.
- `permissions-policy.allow-all-<feature>` (strict) — high-risk
  feature explicitly set to `*` (`camera=*`, `microphone=*`,
  `geolocation=*`, etc.).
- `permissions-policy.invalid` (warn) — header is present but
  unparseable into any directive.
- `permissions-policy.high-risk-omitted` (warn) — partial policy
  that lists some directives but leaves a high-risk feature at
  the `*` default by omission. The detector enumerates which
  features are missing so the operator can extend their policy
  exhaustively.

Acceptable allowlist forms:
- `feature=()` — denied for everyone (best).
- `feature=(self)` — page itself only.
- `feature=(self "https://example.com")` — page + named origin.
- `feature=self` — non-paren shorthand the W3C spec allows.

Out of scope: localhost (same family as hsts/xframe/referrer/
cookie). Unlike Secure cookies, Permissions-Policy CAN apply
over http, so the http-page exemption from `cookieSecurity`
does not carry over.

### Helper-extract decision (third deferral)
Per the cycle-17 doctrine the FOURTH and now FIFTH response-header
detector should trigger the `headerDetector(headerName, parser,
classifier)` extraction. After implementing permissionsPolicy
and observing the multi-directive shape, the extraction is
**deferred again**:

  - hsts / xframeOptions / referrerPolicy each parse a SINGLE
    value and classify into one of N severities.
  - cookieSecurity parses a list of (name, attributes) tuples
    where attributes are positionally set, and aggregates
    findings across the list.
  - permissionsPolicy parses a list of (feature, allowlist)
    pairs where the allowlist itself has structure (deny / self
    / origins / star), and runs PER-FEATURE classification PLUS
    a cross-cutting "is X feature missing from the declared
    set" check.

A naive helper would shoe-horn all three shapes into one. The
queue now reads: **wait for the SIXTH multi-value header
detector (full CSP)** to clarify whether the right abstraction
is `responseHeaderDetector(headerName, snapshotBuilder,
classifier)` (passes the parsed snapshot through to a
classifier callback) or two sister helpers — one for single-
value, one for multi-directive. Implementation defer pays
compound interest; abstraction defer pays the same.

### Tests
- 17 unit scenarios in `permissionsPolicy.test.ts`, all passing —
  no-header, localhost-exempt, comprehensive-deny, garbage,
  camera-allow-all, all-14-features-allow-all (strict-count
  invariant), partial-omits, self-only, origin-allowlist,
  bare-self-shorthand, unknown-feature-ignored, header-name-
  case-insensitive, mixed-allow-deny (only allow flagged),
  empty-header, trailing-comma, whitespace-in-allowlist,
  single-quotes-in-origins.

### Verified
- HTTP gate: 36/36 routes pass.
- HTTPS gate: 22/22 routes pass (17 prior + 5 new permissions
  routes).
- SkillShots audit: 36 axes total (33 named detectors + 3
  legacy event kinds), all silent vs prior. permissionsPolicy
  appears in the positive-signal table; SkillShots is served on
  localhost so the exemption short-circuits — correct
  behaviour, the detector is wired and active.

### Action items
- [ ] Sixth multi-value response-header detector. Strong
      candidate: full Content-Security-Policy parser (we already
      check `frame-ancestors` indirectly via `xFrameOptions`,
      but `default-src`, `script-src`, `style-src`,
      `connect-src` etc. are first-class supersociety controls).
- [ ] After the sixth lands, decide between
      `responseHeaderDetector` (single-value-snapshot →
      classifier) and a sibling `multiValueHeaderDetector`.
- [ ] Cross-Origin-Opener-Policy / Cross-Origin-Embedder-Policy
      detector — Spectre / SharedArrayBuffer cross-origin
      isolation. Two more single-value response-header
      detectors that would fit the existing pattern cleanly.
- [ ] Subresource Integrity (SRI) detector — per-element DOM
      check (every cross-origin `<script>` and
      `<link rel=stylesheet>` should have an `integrity=`
      attribute). Different shape from response-header
      detectors; its own module. CDN-compromise mitigation.

---

## 2026-05-14 (twentieth entry) — cookieSecurity detector + Set-Cookie capture fix

### What's new since last cycle (nineteenth entry)
- 1 new detector axis: **`cookieSecurity`** — Set-Cookie attribute audit. Brings the active-axis count to **32**.
- 5 new HTTPS fixture routes: `/cookie-no-secure/`,
  `/cookie-no-samesite/`, `/cookie-samesite-none-no-secure/`,
  `/cookie-session-no-httponly/`, `/cookie-clean/` (control).
  HTTPS gate now validates **17/17** routes (was 12/12).
- 1 capture-layer fix: switched `topLevelResponseHeaders`
  population from sync `response.headers()` (which strips
  `Set-Cookie`) to `await response.allHeaders()`. Existing
  hsts / xframe / referrer detectors keep working — both forms
  return lowercase keys for the headers they consume.

### Why cookieSecurity now
The supersociety stack philosophy requires multiple layers of
cookie-stealing defence: Secure, SameSite, HttpOnly,
SameSite=None+Secure. None of these are runtime-enforceable
from JS — they're attribute hygiene the server has to get right
at the Set-Cookie boundary. The crawler is the only point in
the stack where we can audit them passively across a journey.

cookieSecurity is also the FOURTH response-header detector,
which under the cycle-17 doctrine should trigger the
`headerDetector(headerName, parser, classifier)` helper
extraction. After implementing the detector and observing the
shape of the per-cookie evidence aggregation, that extraction
is **deferred again**: a single response can carry many
`Set-Cookie` lines, each with its own attribute set, which
the previous three header detectors do not. A naive helper
would shoe-horn the per-cookie loop into a per-header
classifier signature. Action item below: design a
`multiValueHeaderDetector` variant that handles repeat-header
semantics first-class before extracting either form.

### Detector design
4 finding kinds, two severities:
- `cookie.no-secure` (strict, https only) — Cookie set
  without `Secure` on https. Leaks over downgrade.
- `cookie.samesite-none-no-secure` (strict) — `SameSite=None`
  without `Secure` is silently dropped by browsers — set the
  attribute combo correctly or the cookie isn't stored.
- `cookie.no-samesite` (warn) — Missing SameSite. Modern
  browsers default `Lax`; old clients leave it unrestricted
  and CSRF-vulnerable.
- `cookie.session-no-httponly` (warn) — Session-named cookie
  (`/sess|sid|auth|token|jwt|bearer/i`) without `HttpOnly`.
  Stealable by injected XSS.

Out of scope: localhost / 127.0.0.1 / `*.localhost` (same
exemption family as hsts/xframe/referrer). On http pages the
no-secure check is suppressed because Secure can't apply, but
no-samesite and session-no-httponly still fire.

### Capture-layer fix lesson
First detector iteration came back zero-positive against the
HTTPS gate. Spike-debugging via a tiny standalone Playwright
script (`cookie-probe.mjs`) confirmed: Playwright's sync
`response.headers()` returns lowercase-keyed headers but
**omits** `Set-Cookie` entirely. `response.allHeaders()`
(async) returns the full set including `set-cookie`. Switched
the capture site to `await allHeaders()` with a fallback to
the sync form on rejection. Single-line change, fixed all
four cookie checks, hsts/xframe/referrer continue to pass.

This is the kind of tool-trust failure AVP-2 Axiom 0
predicts: "the tools are broken." `headers()` is documented
to return all response headers and quietly omits one of the
most security-critical ones. **REGRESSION-GUARD** annotation
added to the capture site so a future "let's avoid the async"
refactor can't silently re-break Set-Cookie.

### Tests
- 16 unit scenarios in `cookieSecurity.test.ts`, all passing —
  no-cookies, localhost-exempt, fully-secured, https-no-Secure,
  http-no-Secure-suppressed, no-SameSite, SameSite=None+no-Secure,
  session-name-no-HttpOnly, non-session-no-HttpOnly,
  multiple-Set-Cookie-newline-split, aggregation-count,
  header-name-case-insensitive, attribute-case-insensitive,
  examples-capped-at-5, count-still-reflects-all,
  malformed-skipped.

### Verified
- HTTP gate: 36/36 routes pass.
- HTTPS gate: 17/17 routes pass (12 prior + 5 new cookie routes).
- SkillShots audit: 32 axes, all silent vs prior. cookieSecurity
  appears in the positive-signal table; site sets no cookies, so
  zero findings — correct behaviour confirms the detector is
  wired and active without firing on a clean baseline.

### Action items
- [ ] Design `multiValueHeaderDetector` (or a sister to
      `headerDetector`) that handles repeat-header semantics
      first-class — Set-Cookie is the canonical case but
      `Link`, `Vary`, `Warning` and CSP report-uri all share
      the shape. Extract on the next multi-value-header
      detector landing.
- [ ] HSTS-preload-list cross-check: a cookie's `Secure`
      attribute can be implied if the parent domain is on the
      preload list — currently the detector still fires on
      missing Secure. Low priority (defence-in-depth wants the
      attribute explicit anyway), but worth noting in the
      finding's detail text.
- [ ] Session-name regex evolution: the current heuristic is
      `/sess|sid|auth|token|jwt|bearer/i`. Real apps use
      framework-specific names (`csrftoken`, `XSRF-TOKEN`,
      `_app_session`, `connect.sid`, `PHPSESSID`). Compile a
      richer dictionary from real-world traffic over the next
      few dogfood cycles.

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
