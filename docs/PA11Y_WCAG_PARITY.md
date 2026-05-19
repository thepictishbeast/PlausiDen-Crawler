# Pa11y / WCAG 2.1 AA Parity Audit

**Status:** parity audit. Maps every WCAG 2.1 SC at Level A + AA to a
Crawler detector. Identifies gaps Pa11y catches that Crawler does not.

**Method:** walked each SC in the WCAG 2.1 quick-reference, looked for
a Crawler detector or behaviour that catches violations of that SC,
classified as covered / partial / gap.

## Principle 1 — Perceivable

| SC      | Level | Title                                     | Crawler axis                                    | Status   |
|---------|-------|-------------------------------------------|-------------------------------------------------|----------|
| 1.1.1   | A     | Non-text Content (alt text)               | `image_dimensions` (alt presence partial)       | partial — need dedicated `image_alt_text` detector |
| 1.2.1   | A     | Audio-only / Video-only (Prerecorded)     | none                                            | gap |
| 1.2.2   | A     | Captions (Prerecorded)                    | none                                            | gap |
| 1.2.3   | A     | Audio Description or Media Alternative    | none                                            | gap |
| 1.2.4   | AA    | Captions (Live)                           | none                                            | gap (rare on static sites) |
| 1.2.5   | AA    | Audio Description (Prerecorded)           | none                                            | gap |
| 1.3.1   | A     | Info and Relationships                    | `heading_order`, `runtime_landmarks`, `form_labels`, `aria_required_attrs` | covered |
| 1.3.2   | A     | Meaningful Sequence                       | DOM walk inherent                                | covered |
| 1.3.3   | A     | Sensory Characteristics                   | none (language-level)                            | gap |
| 1.3.4   | AA    | Orientation                               | `viewport_meta` (no `orientation=portrait`)     | partial |
| 1.3.5   | AA    | Identify Input Purpose                    | `autocomplete`                                  | covered |
| 1.4.1   | A     | Use of Color                              | none                                            | gap — need `link_color_only` detector |
| 1.4.2   | A     | Audio Control                             | `autoplay_media`                                | covered |
| 1.4.3   | AA    | Contrast (Minimum)                        | `runtime_contrast`                              | covered |
| 1.4.4   | AA    | Resize Text                               | none                                            | gap |
| 1.4.5   | AA    | Images of Text                            | none                                            | gap |
| 1.4.10  | AA    | Reflow                                    | `ui_overflow`, `text_wrap_collapse`             | covered |
| 1.4.11  | AA    | Non-text Contrast                         | `runtime_contrast` (text only)                  | partial — need `non_text_contrast` |
| 1.4.12  | AA    | Text Spacing                              | none                                            | gap |
| 1.4.13  | AA    | Content on Hover or Focus                 | `runtime_focus` (partial)                       | partial |

## Principle 2 — Operable

| SC      | Level | Title                                     | Crawler axis                                    | Status   |
|---------|-------|-------------------------------------------|-------------------------------------------------|----------|
| 2.1.1   | A     | Keyboard                                  | `runtime_focus`                                 | covered |
| 2.1.2   | A     | No Keyboard Trap                          | `runtime_focus`                                 | covered |
| 2.1.4   | A     | Character Key Shortcuts                   | none                                            | gap |
| 2.2.1   | A     | Timing Adjustable                         | none                                            | gap |
| 2.2.2   | A     | Pause, Stop, Hide                         | `autoplay_media`                                | partial |
| 2.3.1   | A     | Three Flashes or Below                    | none                                            | gap |
| 2.4.1   | A     | Bypass Blocks                             | `skip_link`                                     | covered |
| 2.4.2   | A     | Page Titled                               | `doc_title`, `cross_page_title`                 | covered |
| 2.4.3   | A     | Focus Order                               | `runtime_focus`                                 | covered |
| 2.4.4   | A     | Link Purpose (In Context)                 | `link_text`                                     | covered |
| 2.4.5   | AA    | Multiple Ways                             | none                                            | gap |
| 2.4.6   | AA    | Headings and Labels                       | `heading_order`, `form_labels`                  | covered |
| 2.4.7   | AA    | Focus Visible                             | `runtime_focus`                                 | covered |
| 2.5.1   | A     | Pointer Gestures                          | none                                            | gap |
| 2.5.2   | A     | Pointer Cancellation                      | none                                            | gap |
| 2.5.3   | A     | Label in Name                             | `form_labels`                                   | partial |
| 2.5.4   | A     | Motion Actuation                          | none                                            | gap |
| 2.5.5   | AAA   | Target Size                               | `tap_targets` (24×24 + 44×44)                   | covered (AAA bonus) |

## Principle 3 — Understandable

| SC      | Level | Title                                     | Crawler axis                                    | Status   |
|---------|-------|-------------------------------------------|-------------------------------------------------|----------|
| 3.1.1   | A     | Language of Page                          | `html_lang`                                     | covered |
| 3.1.2   | AA    | Language of Parts                         | none                                            | gap |
| 3.2.1   | A     | On Focus                                  | `runtime_focus`                                 | partial |
| 3.2.2   | A     | On Input                                  | none                                            | gap |
| 3.2.3   | AA    | Consistent Navigation                     | `cross_page_title` (partial)                    | gap |
| 3.2.4   | AA    | Consistent Identification                 | none                                            | gap |
| 3.3.1   | A     | Error Identification                      | none                                            | gap |
| 3.3.2   | A     | Labels or Instructions                    | `form_labels`                                   | covered |
| 3.3.3   | AA    | Error Suggestion                          | none                                            | gap |
| 3.3.4   | AA    | Error Prevention (Legal, Financial, Data) | none                                            | gap |

## Principle 4 — Robust

| SC      | Level | Title                                     | Crawler axis                                    | Status   |
|---------|-------|-------------------------------------------|-------------------------------------------------|----------|
| 4.1.1   | A     | Parsing                                   | none — HTML5 parsing is forgiving               | covered (obsolete in WCAG 2.2) |
| 4.1.2   | A     | Name, Role, Value                         | `aria_required_attrs`                           | covered |
| 4.1.3   | AA    | Status Messages                           | none                                            | gap |

## Summary

* **Fully covered (24 SCs):** 1.3.1, 1.3.2, 1.3.5, 1.4.2, 1.4.3, 1.4.10, 2.1.1, 2.1.2, 2.4.1, 2.4.2, 2.4.3, 2.4.4, 2.4.6, 2.4.7, 2.5.5, 3.1.1, 3.3.2, 4.1.1, 4.1.2.
* **Partial coverage (8 SCs):** 1.1.1, 1.3.4, 1.4.11, 1.4.13, 2.2.2, 2.5.3, 3.2.1.
* **Gaps Pa11y catches that Crawler does NOT (21 SCs):**
  * Media: 1.2.1, 1.2.2, 1.2.3, 1.2.4, 1.2.5 — captions / audio
    descriptions / media alternatives.
  * Visual: 1.4.1 (link-color-only), 1.4.4 (resize text), 1.4.5
    (images of text), 1.4.12 (text spacing).
  * Operable: 2.1.4 (character shortcuts), 2.2.1 (timing), 2.3.1
    (three flashes), 2.5.1, 2.5.2, 2.5.4 (pointer / motion).
  * Understandable: 2.4.5 (multiple ways), 3.1.2 (language of
    parts), 3.2.2 (on input), 3.2.3, 3.2.4 (consistency),
    3.3.1, 3.3.3, 3.3.4 (error identification / suggestion /
    prevention).
  * Robust: 4.1.3 (status messages).

## Highest-priority gaps for follow-up

Marketing / SaaS pages tend to violate these most often:

1. **1.4.1 Use of Color** (link-color-only affordance) — most SaaS
   templates show links by color alone. Detector: scan styled
   `<a>` and check that color contrast > 3:1 AGAINST surrounding
   text, OR that `text-decoration` is `underline` (or
   `loom-link-underline` opt-in). Filed: separate task.

2. **3.3.1 / 3.3.3 Error Identification + Suggestion** — when
   `<form>` is submitted and the server returns an error,
   the page must (a) tell the user what went wrong in text and
   (b) suggest a fix. Detector: scan form-error markup (aria-
   describedby on inputs, role=alert containers).

3. **4.1.3 Status Messages** — `aria-live` regions on toast / alert
   / form-success messages. Detector: scan known interactive
   widgets (Crucible challenge result, signin success, search
   results) for an aria-live wrapper.

4. **2.4.5 Multiple Ways** — every page should be reachable via at
   least two of: navigation, search, sitemap, table of contents.
   Detector: cross-page audit looks for site-wide search +
   navigation presence.

5. **1.4.4 Resize Text** — page must remain functional at 200%
   zoom. Detector: run the layout audit at viewport scale 200%
   and check for `ui_overflow` + `text_wrap_collapse` regressions.

## Out of scope for an automated runner

The media SCs (1.2.x), animation/motion SCs (2.3.1, 2.5.4), and
content-quality SCs (1.3.3 Sensory Characteristics, 3.2.1 On Focus
language-level cases) need human review. Crawler can flag presence/
absence of structural hooks (track elements, prefers-reduced-motion
honored, etc.) but cannot judge appropriateness of captions or whether
a sensory cue is necessary.

## How to extend the runner

Each gap detector lands as a new `crawler-detectors/src/<axis>.rs`
file + a `EventKind::<Axis>` variant in `crawler-report` + a
`capture_<axis>(page, events, started_at)` in
`crawler-runner/src/main.rs` (mirror `text_wrap_collapse` or
`placeholder_text` shape). Each ships with positive + negative unit
tests on a snapshot struct.

## Maintenance

This document is point-in-time (audit performed against Crawler at
commit e2cf934, WCAG 2.1 Recommendation). Re-audit on every Crawler
detector addition. Add the new SC coverage row above and remove from
the gap list. Filed as a recurring task in the loop's PRIORITY 4
("Improve Crawler detectors").
