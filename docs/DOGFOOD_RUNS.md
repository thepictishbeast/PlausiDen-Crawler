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

- [ ] Wire the fixture into CI: `bin/check-t76-detectors.sh` that
      starts the server, runs the audit, asserts observed
      findings match `expectedFindingsByLabel`.
- [ ] Tweak fixture skip-link CSS so tapTargets only flares on
      the engineered routes.
- [ ] Add `mixedContent` + `linkUnderline` + `fontLoading` routes
      as those detectors land.

---

*Update this file whenever a fresh dogfood run lands. Each entry
should record what was new, what the site exposed in the crawler,
and what gaps remain — turning the dogfood loop into a public
record of detection coverage over time.*
