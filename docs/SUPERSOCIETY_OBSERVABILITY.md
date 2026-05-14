# Supersociety Observability — design + ops manual

This document explains the **6-layer security telemetry pipeline** that
PlausiDen-Crawler audits and PlausiDen-Loom implements. Written for an
operator who's never seen the code: by the end you should be able to
diagnose why a CSP violation didn't reach your inbox, audit a new
PlausiDen surface, and add a detector without breaking the supersociety
score.

The system is the cumulative output of T76 cycles 22–76 (Crawler,
Loom, Forge, Sentinel-GUI). Every layer was built incrementally and
proves out under E2E test, mutation analysis, and dogfood audit.

> **Doctrine source**: `~/.claude/CLAUDE.md` AVP-2 protocol.
> **Threat model**: state-actor adversary, source-disclosed, breach
> in progress. Build defence-in-depth.

---

## TL;DR for the impatient

```
DETECT → ENFORCE → REPORT → COLLECT → AUDIT → REVIEW
  ↑         ↑         ↑         ↑        ↑       ↑
 specs    browser    spec     loom     crawler  loom
                              edit-     detector  report-
                              serve     audit     tail/stats
```

Six layers. Each one cycle of cumulative work. Each layer has E2E
tests pinning its wire format. The score module that judges the
overall result has property + mutation + drift tests on top.

If your dashboard reads **A 100/100 (16)** in `badges/supersociety.svg`,
all six layers are working across all 16 audited surfaces.

---

## Layer 1: DETECT (the policies)

**Where**: HTML response headers + meta tags.
**What**: declare which behaviours are blocked, which sinks are
unwritable, where reports go.

The PlausiDen surfaces emit (cycle 22+ for Loom; cycle 67 for
Sentinel-GUI):

| Header / directive | Cycle | Purpose |
|---|---|---|
| `Strict-Transport-Security` | 22 | force HTTPS |
| `Cross-Origin-Opener-Policy: same-origin` | 22 | process-isolate from cross-origin openers |
| `Cross-Origin-Embedder-Policy: require-corp` | 22 | block embedded resources without CORP |
| `Cross-Origin-Resource-Policy: same-origin` | 27 | only same-origin docs may embed our responses |
| `X-Frame-Options: DENY` | 22 | legacy clickjacking defence (CSP-L2 fallback) |
| `Origin-Agent-Cluster: ?1` | 45 | request dedicated agent cluster |
| `Referrer-Policy: no-referrer` | 22 | minimum information leak |
| `X-Content-Type-Options: nosniff` | 22 | block MIME sniffing |
| `Permissions-Policy: …` | 21 | gate camera / mic / geolocation / etc. |
| `Document-Policy: force-load-at-top` | 60 | predictable scroll restoration |
| `Reporting-Endpoints: default="/reports"` | 63 | route reports to collector |
| `Report-To: {…}` | 63 | legacy reporting routing |
| `NEL: {…}` | 65 | route transport-failure reports |
| `Content-Security-Policy: default-src 'self'; script-src 'self' 'sha256-…'; require-trusted-types-for 'script'; trusted-types <name>; report-to default` | 22, 28, 54, 57, 63 | the big one |

**Verify your surface declares them**: `curl -sD - https://YOUR-SURFACE/ -o /dev/null`.

**Audit them via the Crawler**: `npm run audit -- --journey YOUR-JOURNEY.json`.
The 50-axis sweep flags missing headers. Localhost pages are
exempted from header detectors so dev fixtures don't trip on
loopback (the response-header family doctrine).

---

## Layer 2: ENFORCE (the browser)

**Where**: the user's browser process.
**What**: actually block disallowed actions in real time.

This layer is the browser — Chrome / Firefox / Safari implementing
the W3C specs. You can't change it. You can audit:

- **Did it enforce?** → CSP violation reports in `violations.jsonl`
  (collected by the loom collector, layer 4).
- **Was the policy strong enough?** → cycle-30 `inlineScript`
  detector + cycle-54 hash-pinned CSP audit catch weak policies
  before they ship.
- **Is enforcement actually firing?** → cycle 57's `trustedTypes`
  runtime monitor proxies DOM sinks (`innerHTML`, `outerHTML`,
  `document.write`, `setTimeout(string)`, etc.) and reports
  policy-violation calls; pairs with `require-trusted-types-for`
  in the CSP.

---

## Layer 3: REPORT (the spec)

**Where**: W3C Reporting-API (CSP-L3) + NEL (W3C 2018+).
**What**: when ENFORCE fires, the browser POSTs a JSON report to
the URL named in the `report-to <group>` directive that resolves
to a `Reporting-Endpoints` group.

Two endpoint paths Loom listens for (cycle 63):

```
/csp-report          ← legacy `application/csp-report`     (CSP-L1/L2)
/reports             ← modern  `application/reports+json`  (CSP-L3 + NEL + COEP + Document-Policy + Crash + Deprecation)
```

Both return `204 No Content` per spec — even when rate-limited or
when the disk is full. Returning anything else triggers browser
retry-storms that amplify a real attack.

Reports flow through the `default` group set up by:

```
Reporting-Endpoints: default="/reports"
Content-Security-Policy: …; report-to default
NEL: {"report_to": "default", …}
```

The cycle 64 `reportingEndpoints` detector cross-checks these:

- `reporting.csp-group-undeclared` — CSP says `report-to <X>` but
  Reporting-Endpoints doesn't declare `<X>`. Reports vanish.
- `reporting.endpoint-orphan` — `<X>` is declared but no CSP
  references it. Wasted config.
- `reporting.endpoint-not-https` — endpoint URL is plaintext
  http://. Reports leak in transit.

---

## Layer 4: COLLECT (loom edit-serve `/reports` + `/csp-report`)

**Where**: `loom-cli/src/main.rs::handle_security_report` (Loom).
**What**: write each incoming report to a JSONL log on disk.

The collector is intentionally minimal:

```jsonl
{"ts":1736380800,"endpoint":"reports","content_type":"application/reports+json","body":"<wire body, escaped>"}
```

Hand-built JSON (no serde dep) so the auditor stays trivially
verifiable — every byte in `violations.jsonl` is traceable to a
single small function.

### Hardening (cumulative cycles 63 → 71)

- **Cycle 63** (build): handler + JSONL writer + `204 No Content`
  return. E2E unverified.
- **Cycle 68** (pin): 4 E2E tests pin the wire format. Future
  changes that mutate the JSONL shape fail this gate.
- **Cycle 69** (DoS defence): per-IP sliding-window rate limit.
  100 reports/min/IP, 1024 distinct IPs capped (LRU evict on
  overflow). Limited requests still return 204 (no retry-storm)
  — drop is silent, visible only in stderr.
- **Cycle 71** (storage defence): size-based rotation. When
  `violations.jsonl` exceeds 50 MiB, rotate to
  `violations-<unix_secs>.<nanos>.jsonl`. Keep last 10
  rotations; LRU-prune older.

Env tunables (operators normally use defaults):

```
LOOM_REPORT_ROTATION_BYTES   default 50 MiB
LOOM_REPORT_ROTATION_KEEP    default 10
```

### File layout

`loom edit-serve` runs from a CMS root. The collector writes to
the SIBLING `reports/` directory:

```
<workdir>/
  cms/                       ← edited content
    test.json
  static/                    ← published artefacts
  reports/                   ← collector output (NEW; cycle 63)
    violations.jsonl              ← active log
    violations-1736380800.0.jsonl ← rotated (cycle 71)
    violations-1736650000.0.jsonl
```

### Threats the collector defends against

| Attack | Defence | Cycle |
|---|---|---|
| Endpoint forgery (browsers can't carry session cookies on report POSTs) | Unauthenticated by design + `frame-ancestors 'self'` on the host page | 63 |
| Flash burst / DoS spam | Per-IP rate limit | 69 |
| IP-spray to OOM the bucket map | LRU evict at 1024 IPs | 69 |
| Patient long-run growth | Size-based rotation + retention | 71 |
| Body-size DoS | 64 KiB body cap (read truncates) | 63 |
| Wire-format regression | E2E test suite (6 tests) | 68 |
| Misconfigured directives → silent black hole | Cycle 64 cross-consistency detector | 64 |

---

## Layer 5: AUDIT (PlausiDen-Crawler)

**Where**: `PlausiDen-Crawler/src/main.ts` runs ~50 detection axes.
**What**: walk a journey, flag every defect.

### Run an audit

```
npm run audit -- --journey journeys/YOUR.json
npm run audit -- --journey journeys/YOUR.json --no-baseline   # surface frozen warns
```

Output goes to `runs/<journey>-<timestamp>/`:

| File | Contents |
|---|---|
| `report.json` | every captured event |
| `diff.json` | events new vs prior run |
| `supersociety-score.json` | weighted composite + per-category breakdown |
| `<axis>.json` | per-detector findings |
| `aria.txt` (per step) | accessibility tree snapshot |
| `<step>.png` + `.annotated.png` | screenshots with red boxes around violations |

### The 50 axes

Listed in `docs/DETECTORS.md`. Categories (cycle 32 weights):

| Category | Weight | Axes |
|---|---|---|
| transportSecurity | 2.0 | hsts, mixed-content |
| originIsolation | 2.0 | coop, coep, corp, x-frame-options, permissions-policy, origin-agent-cluster |
| contentSecurity | 2.0 | csp-policy, sri, inline-script, trusted-types, document-policy |
| cookieHygiene | 2.0 | cookie-security |
| cacheCorrectness | 1.5 | cache-control, vary |
| infoDisclosure | 1.0 | info-leak, referrer-policy |
| observability | 1.0 | reporting-endpoints, nel |
| reliability | 1.5 | console-error, response-error, request-failed, blank-main, ui-error-text, stuck-loading, error-boundary-visible |
| accessibility | 1.5 | a11y-violation, heading-order, runtime-landmarks, runtime-contrast, runtime-focus, link-text, placeholder-text, tap-targets, form-labels, doc-title, html-lang, skip-link, autocomplete, aria-drift, link-underline, runtime-images |
| uxHygiene | 1.0 | viewport-meta, meta-description, favicon, outbound-links, cross-page-title, cross-page-meta-description, font-loading, web-vitals, ui-overflow, css-health |

Per-category score: `max(0, min(100, 100 - strict*25 - warn*5))`.
Composite: weighted average, rounded.
Grade: A≥90, B≥80, C≥70, D≥60, F<60.

### Score module is itself audited (cycles 66, 73, 74, 75, 76)

The score module is **THE auditor of the auditor**. If its math is
wrong, the green badge means nothing. So we have a 4-layer
validation stack on top of it:

```
property tests (12 × 200 cases)  catch MATH bugs                → cycle 66
mutation tests (5 scenarios)     catch GAPS in property tests   → cycle 73
drift detector (4 checks)        catch UNMAPPED-KIND regressions → cycle 74
production audit                  catches REAL-WORLD bugs        → every cycle
```

One-shot runner: `npm run test:meta` (cycle 75).

### Live aggregate badge

`badges/supersociety.svg` updates after every audit run. It
shows the worst-of-N composite across every audited journey
(cycle 61). Linked from the README. Filtered to exclude
`t76-detector-fixtures*` (intentional-fail journeys).

---

## Layer 6: REVIEW (loom report-tail / report-stats)

**Where**: `loom-cli/src/main.rs::cmd_report_tail` and `cmd_report_stats`.
**What**: operator-readable views over the collector log.

### `loom report-tail` (cycle 70)

Live per-entry detail:

```
$ loom report-tail
2025-01-09 00:00:00Z  csp-violation     [csp-report]  {"csp-report":{"violated-directive":"script-src",…}}
2025-01-09 00:01:40Z  deprecation       [reports]     [{"type":"deprecation","body":{"id":"X"}}]
```

Flags:

```
--lines N        recent N entries (default 20)
--kind X         substring filter on the body field
--follow         poll every 1s, print new lines as they arrive
```

Colourised when stdout is a TTY (red for csp-violation,
magenta for trusted-types, cyan for nel, etc.).

### `loom report-stats` (cycle 72)

Cross-rotation summary:

```
$ loom report-stats
kind             count  first-seen           last-seen            top-url
csp-violation    47     2025-01-09 03:00:00Z 2025-01-09 17:42:11Z https://x.example/
nel              3      2025-01-09 12:00:00Z 2025-01-09 17:40:00Z https://y.example/
deprecation      12     2025-01-09 01:00:00Z 2025-01-09 16:00:00Z (none)
```

Reads `violations.jsonl` PLUS every rotated sibling. Lexical sort
of the rotation suffix = chronological per cycle 71's
fixed-width `<unix_secs>.<nanos>` format.

Flags:

```
--since <unix>   filter to entries with ts >= this (default 0)
--kind X         substring filter
--json           emit single-line JSON for jq / dashboards / SIEM
```

Both subcommands share the same JSON walker + date formatter +
classifier (cycle 76 dedupe). Hand-rolled — no serde / chrono dep.
The auditor is byte-verifiable.

---

## Adding a new detector (the safe path)

1. Create `src/yourDetector.ts` exporting `buildXSnapshot()`,
   `detectXIssues()`, plus an `XFinding` type.
2. Wire it in `src/main.ts`: import, allocate
   `xFindingsByStep`, register `checkX = makeResponseHeaderCheck({…})`
   (or hand-roll if it's a per-element walker), call
   `await checkX(step.label || \`goto-\${i}\`)` in the per-step
   loop.
3. Add the kind to `KIND_TO_CATEGORY` in
   `src/supersocietyScore.ts` so events get scored.
4. Add the diff field to `src/report.ts` (the `Diff` interface,
   the empty-init, the per-event-kind switch, and the
   axis-summary table).
5. Run `npm run test:meta` — the **drift detector** will fail if
   you skip step 3 (cycle 74). The property + mutation suites
   prove the math still holds.
6. Run `npm run audit -- --journey …` to confirm the new axis
   appears in the per-axis table and the supersociety score
   reflects it.
7. Commit with an `AVP-PASS-N: <date>` annotation.

---

## Adding a new audited surface (the dogfood path)

1. Spin up the surface on a free localhost port (the response-
   header detectors exempt localhost — that's intentional for
   dev fixtures).
2. Create `journeys/<surface>.json`:

```json
{
  "name": "<surface>",
  "description": "T76 cycle N: dogfood loop extension. <One-line context>.",
  "baseUrl": "http://127.0.0.1:<PORT>",
  "viewport": { "width": 1280, "height": 900 },
  "steps": [
    { "kind": "goto", "url": "http://127.0.0.1:<PORT>/", "label": "<surface>-index" }
  ]
}
```

3. Run `npm run audit -- --journey journeys/<surface>.json --no-baseline`.
4. Read the per-axis table; for each finding kind that fired,
   apply the cumulative fix-pattern library:
   - `favicon.missing-link` / `meta-description.missing` →
     add inline-SVG favicon + meta description (cycle 52).
   - `tap.below-recommended` → bump min-height to 44px (cycle 55).
   - `runtime-landmarks` → add `<header>`, `<main id="main">`,
     `<section aria-label="…">` (cycle 67).
   - `skip.missing` → add visually-hidden skip-link (cycle 67).
   - `inline-script.no-csp-but-inline` → add hash-pinned CSP
     (cycle 54).
   - `tt.directive-missing` → add `require-trusted-types-for
     'script'; trusted-types <name>` (cycle 57).
   - `reporting.no-endpoints` → add Reporting-Endpoints +
     Report-To + NEL headers (cycle 63 + 65).
   - `runtime-contrast` (esp. dark mode) → set EXPLICIT colors
     on every interactive element (cycle 67b — skip-link
     contrast bug).
5. Iterate. The compression value: PlausiDen-Loom took 28 cross-
   repo commits to reach A 100/100 (cycles 38-67). PlausiDen-
   Sentinel-GUI took ONE rewrite (cycle 67). New surfaces
   inherit the fix-pattern library and converge fast.

---

## Operating the system

### "I deployed something — did it break?"

```
npm run audit -- --journey journeys/YOUR.json
```

PASS = no new regressions vs baseline. FAIL = the diff has new
strict findings. Read `runs/<latest>/diff.json` for detail.

### "Are there frozen issues hiding in the baseline?"

```
npm run audit -- --journey journeys/YOUR.json --no-baseline
```

Surfaces every extant finding. Use to confirm a clean state
before declaring "done", or to find baseline-frozen bugs that
weren't there when the baseline was first captured.

### "Did the browser actually enforce my CSP?"

```
loom report-tail --kind csp-violation
loom report-stats --kind csp-violation --since $(date -d '24 hours ago' +%s)
```

If CSP violations are firing in production, they land in
`violations.jsonl`. If they're NOT firing but you expect
violations (e.g., you intentionally injected a bad inline
script), check:

1. Is `Content-Security-Policy` header actually present?
   (`curl -sD - URL -o /dev/null`)
2. Does CSP `report-to <group>` reference a group declared in
   `Reporting-Endpoints`? (`npm run audit` — cycle 64 detector
   catches this)
3. Is the collector running? (`ss -tlnp | grep loom`)

### "What's the current overall state?"

```
npm run audit -- --journey journeys/loom-edit-server.json
cat badges/supersociety-aggregate.json | jq .
```

Or look at `badges/supersociety.svg` in any markdown viewer
that renders SVG. The badge updates after every audit; the
README references it for any visitor.

### "How do I know the score is trustworthy?"

```
npm run test:meta
```

If this prints `=== Tier-6 meta-validation: ALL THREE LAYERS PASSED ===`,
the score module's math is provably sound under randomised
stress (12 properties × 200 cases each), under deliberate
mutation (5 scenarios), and against detector-coverage drift
(4 checks).

---

## What this still isn't

- **No SIEM integration**: cycle 72's `report-stats --json`
  emits a clean shape suitable for ingestion, but no built-in
  forwarder yet.
- **No log encryption at rest**: `violations.jsonl` is
  plaintext. Acceptable for the current operator-on-bare-VPS
  threat model; would change if PII or session tokens
  accidentally leaked into report bodies.
- **No replay protection on report POSTs**: an attacker who
  captures a legitimate browser report can replay it
  indefinitely (within rate-limit). Mitigated by the 64 KiB
  body cap + per-IP rate limit.
- **No alarming**: the operator polls `loom report-stats` or
  `tail -f violations.jsonl`. No webhook out, no Slack /
  PagerDuty integration.
- **Atrium is egui-native**: no HTTP surface, so this stack
  doesn't apply directly. Atrium integration would be via the
  BleachBit-bridge pivot (cycles 78+).

These are real, named gaps — not pretended-perfection. AVP-2
doctrine: ship explicit risk acceptance, not silent omission.

---

## File index

| File | Purpose | Cycle |
|---|---|---|
| `PlausiDen-Loom/loom-cli/src/main.rs::respond_html` | DETECT layer headers | 22-67 |
| `PlausiDen-Loom/loom-cli/src/main.rs::handle_security_report` | COLLECT layer | 63 |
| `PlausiDen-Loom/loom-cli/src/main.rs::cmd_report_tail` | REVIEW (live) | 70 |
| `PlausiDen-Loom/loom-cli/src/main.rs::cmd_report_stats` | REVIEW (summary) | 72 |
| `PlausiDen-Loom/loom-cli/tests/report_collector_e2e.rs` | COLLECT pinning | 68, 69, 71 |
| `PlausiDen-Loom/loom-cli/tests/report_tail_e2e.rs` | REVIEW pinning | 70 |
| `PlausiDen-Loom/loom-cli/tests/report_stats_e2e.rs` | REVIEW pinning | 72 |
| `PlausiDen-Crawler/src/main.ts` | AUDIT layer dispatcher | 22+ |
| `PlausiDen-Crawler/src/<axis>.ts` | per-axis detector | 22+ |
| `PlausiDen-Crawler/src/supersocietyScore.ts` | scoring model | 32 |
| `PlausiDen-Crawler/src/supersocietyBadgeAggregate.ts` | badge SVG | 61 |
| `PlausiDen-Crawler/src/supersocietyScore.test.ts` | example tests | 32 |
| `PlausiDen-Crawler/src/supersocietyScore.property.test.ts` | property tests | 66 |
| `PlausiDen-Crawler/src/supersocietyScore.mutation.test.ts` | mutation analysis | 73 |
| `PlausiDen-Crawler/src/supersocietyScore.drift.test.ts` | unmapped-kind gate | 74 |
| `PlausiDen-Crawler/docs/DETECTORS.md` | per-axis reference | 22+ |
| `PlausiDen-Crawler/docs/DOGFOOD_RUNS.md` | full cycle log (this is your time machine) | 38+ |
| `PlausiDen-Crawler/badges/supersociety.svg` | aggregate badge | 61 |

Cumulative as of cycle 76: 32 Loom commits + 3 Forge + 1
Sentinel-GUI + 11 crawler enhancements + 3 E2E suites +
property + mutation + drift suites + meta-runner.

---

*This document is part of the supersociety doctrine artifact set.
If you're maintaining PlausiDen and need to defend a design
choice, the answer is probably in `docs/DOGFOOD_RUNS.md`. If you
need to understand WHY the choice exists, the answer is here.*
