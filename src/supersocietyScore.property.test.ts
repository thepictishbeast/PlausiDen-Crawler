/**
 * supersocietyScore.property.test.ts — property-based invariants
 * on the score calculator. T76 cycle 66 (Tier 6 meta-validation).
 *
 * Hand-rolled property runner with a deterministic seeded PRNG
 * so the tests are reproducible without an external dep. Each
 * property runs 200 randomised cases; failure prints the seed
 * + the minimal input that broke the invariant.
 *
 * The properties encode what we BELIEVE about the score model.
 * If a future refactor or detector mapping change breaks one,
 * either the property is wrong (update it) or the refactor
 * introduced a bug (the property caught it). Either way the
 * dashboard's trustworthiness depends on these invariants
 * holding.
 *
 * Properties:
 *   1. composite ∈ [0, 100] for any input stream.
 *   2. grade is monotonic with composite.
 *   3. adding a single strict event can only DECREASE or KEEP
 *      composite (never increase).
 *   4. adding a single warn event can only DECREASE or KEEP
 *      composite (never increase).
 *   5. strict penalty ≥ warn penalty for the same kind.
 *   6. empty event stream → composite 100, grade A.
 *   7. unknown event kind → no category penalty (lands in
 *      unbucketed list).
 *   8. determinism: same input → same output.
 *   9. category scores are clamped to [0, 100].
 *  10. composite is the weighted average of category scores
 *      (within 1 unit of the rounded value).
 */
import {
  calculateSupersocietyScore,
  type ScoreInputEvent,
} from './supersocietyScore.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// Deterministic linear-congruential PRNG. Same seed → same
// sequence forever. Modulus 2^31, multiplier from Numerical
// Recipes; period 2^31 (long enough for 200 cases × ~20
// random draws each).
function makeRng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1103515245 + 12345) >>> 0;
    return (s & 0x7fffffff) / 0x7fffffff;
  };
}

// Kinds the score module maps to a category. We sample from
// these in property generation; an unknown kind would land in
// `unbucketed` and is tested separately.
const KNOWN_KINDS = [
  'hsts', 'mixed-content',                                  // transportSecurity
  'coop', 'coep', 'corp', 'x-frame-options',
  'permissions-policy',                                      // originIsolation
  'csp-policy', 'sri', 'inline-script', 'trusted-types',
  'document-policy',                                         // contentSecurity
  'cookie-security',                                         // cookieHygiene
  'cache-control', 'vary',                                   // cacheCorrectness
  'info-leak', 'referrer-policy',                            // infoDisclosure
  'reporting-endpoints', 'nel',                              // observability
  'response-error', 'request-failed',                        // reliability
  'a11y-violation', 'heading-order', 'runtime-landmarks',
  'link-text', 'placeholder-text', 'tap-targets',
  'form-labels', 'autocomplete', 'link-underline',
  'aria-drift', 'runtime-contrast', 'runtime-images',
  'runtime-focus',                                           // accessibility
  'viewport-meta', 'doc-title', 'html-lang', 'skip-link',
  'outbound-links', 'meta-description', 'favicon',
  'font-loading', 'web-vitals', 'cross-page-title',
  'cross-page-meta-description', 'css-health',
  'ui-overflow', 'origin-agent-cluster',                     // uxHygiene
];

const SEVERITIES: Array<'strict' | 'warn'> = ['strict', 'warn'];

function gen(rng: () => number, n: number): ScoreInputEvent[] {
  const out: ScoreInputEvent[] = [];
  for (let i = 0; i < n; i++) {
    const kind = KNOWN_KINDS[Math.floor(rng() * KNOWN_KINDS.length)];
    const severity = SEVERITIES[Math.floor(rng() * SEVERITIES.length)];
    out.push({ kind, severity });
  }
  return out;
}

const CASES = 200;
const MAX_EVENTS_PER_CASE = 60;

// --- Property 1: composite ∈ [0, 100] always.
{
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(12345);
  for (let i = 0; i < CASES; i++) {
    const n = Math.floor(rng() * MAX_EVENTS_PER_CASE);
    const events = gen(rng, n);
    const score = calculateSupersocietyScore(events);
    if (score.composite < 0 || score.composite > 100 || !Number.isFinite(score.composite)) {
      failures += 1;
      if (firstFail === null) firstFail = `composite=${score.composite} for ${events.length} events`;
    }
  }
  assert(failures === 0, 'composite ∈ [0, 100] always',
    `${failures}/${CASES} cases out of range. First: ${firstFail}`);
}

// --- Property 2: grade is monotonic with composite.
{
  const order: Record<string, number> = { F: 0, D: 1, C: 2, B: 3, A: 4 };
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(67890);
  for (let i = 0; i < CASES; i++) {
    const a = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const b = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const sa = calculateSupersocietyScore(a);
    const sb = calculateSupersocietyScore(b);
    // If sa.composite > sb.composite, then sa.grade order
    // should be >= sb.grade order.
    if (sa.composite > sb.composite && order[sa.grade] < order[sb.grade]) {
      failures += 1;
      if (firstFail === null)
        firstFail = `composite ${sa.composite}>${sb.composite} but grade ${sa.grade}<${sb.grade}`;
    }
    if (sb.composite > sa.composite && order[sb.grade] < order[sa.grade]) {
      failures += 1;
      if (firstFail === null)
        firstFail = `composite ${sb.composite}>${sa.composite} but grade ${sb.grade}<${sa.grade}`;
    }
  }
  assert(failures === 0, 'grade monotonic with composite',
    `${failures}/${CASES} cases violate. First: ${firstFail}`);
}

// --- Property 3: adding a single strict event never INCREASES composite.
{
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(11111);
  for (let i = 0; i < CASES; i++) {
    const base = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const extra: ScoreInputEvent = {
      kind: KNOWN_KINDS[Math.floor(rng() * KNOWN_KINDS.length)],
      severity: 'strict',
    };
    const baseScore = calculateSupersocietyScore(base);
    const withExtra = calculateSupersocietyScore([...base, extra]);
    if (withExtra.composite > baseScore.composite) {
      failures += 1;
      if (firstFail === null)
        firstFail = `adding strict ${extra.kind}: ${baseScore.composite} → ${withExtra.composite}`;
    }
  }
  assert(failures === 0, 'adding strict event never increases composite',
    `${failures}/${CASES} cases violate. First: ${firstFail}`);
}

// --- Property 4: adding a single warn event never INCREASES composite.
{
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(22222);
  for (let i = 0; i < CASES; i++) {
    const base = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const extra: ScoreInputEvent = {
      kind: KNOWN_KINDS[Math.floor(rng() * KNOWN_KINDS.length)],
      severity: 'warn',
    };
    const baseScore = calculateSupersocietyScore(base);
    const withExtra = calculateSupersocietyScore([...base, extra]);
    if (withExtra.composite > baseScore.composite) {
      failures += 1;
      if (firstFail === null)
        firstFail = `adding warn ${extra.kind}: ${baseScore.composite} → ${withExtra.composite}`;
    }
  }
  assert(failures === 0, 'adding warn event never increases composite',
    `${failures}/${CASES} cases violate. First: ${firstFail}`);
}

// --- Property 5: a single strict penalises >= a single warn (same kind).
{
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(33333);
  for (let i = 0; i < CASES; i++) {
    const kind = KNOWN_KINDS[Math.floor(rng() * KNOWN_KINDS.length)];
    const baseline = calculateSupersocietyScore([]);
    const withStrict = calculateSupersocietyScore([{ kind, severity: 'strict' }]);
    const withWarn = calculateSupersocietyScore([{ kind, severity: 'warn' }]);
    const strictDrop = baseline.composite - withStrict.composite;
    const warnDrop = baseline.composite - withWarn.composite;
    if (strictDrop < warnDrop) {
      failures += 1;
      if (firstFail === null)
        firstFail = `${kind} strict drop ${strictDrop} < warn drop ${warnDrop}`;
    }
  }
  assert(failures === 0, 'strict penalty >= warn penalty (same kind)',
    `${failures}/${CASES} cases violate. First: ${firstFail}`);
}

// --- Property 6: empty events → composite 100, grade A, 0 strict, 0 warn.
{
  const s = calculateSupersocietyScore([]);
  assert(
    s.composite === 100 && s.grade === 'A' && s.totalStrict === 0 && s.totalWarn === 0,
    'empty events → 100/A/0/0',
    JSON.stringify(s),
  );
}

// --- Property 7: unknown event kind doesn't penalise; lands in unbucketed.
{
  const s = calculateSupersocietyScore([{ kind: 'made-up-kind-cycle66', severity: 'strict' }]);
  assert(
    s.composite === 100 && s.unbucketed.includes('made-up-kind-cycle66'),
    'unknown kind → no penalty, listed in unbucketed',
    JSON.stringify(s),
  );
}

// --- Property 8: determinism — same input → same output.
{
  let failures = 0;
  const rng = makeRng(44444);
  for (let i = 0; i < CASES; i++) {
    const events = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const a = calculateSupersocietyScore(events);
    const b = calculateSupersocietyScore(events);
    if (JSON.stringify(a) !== JSON.stringify(b)) failures += 1;
  }
  assert(failures === 0, 'determinism: same input → same output', `${failures}/${CASES}`);
}

// --- Property 9: every category score is in [0, 100].
{
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(55555);
  for (let i = 0; i < CASES; i++) {
    const events = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const s = calculateSupersocietyScore(events);
    for (const c of s.categories) {
      if (c.score < 0 || c.score > 100 || !Number.isFinite(c.score)) {
        failures += 1;
        if (firstFail === null) firstFail = `${c.category}.score=${c.score}`;
      }
    }
  }
  assert(failures === 0, 'every category score ∈ [0, 100]',
    `${failures} out-of-range. First: ${firstFail}`);
}

// --- Property 10: composite ≈ Σ(c.score × c.weight) / Σ(c.weight)
// (within 1 unit because of rounding).
{
  let failures = 0;
  let firstFail: string | null = null;
  const rng = makeRng(66666);
  for (let i = 0; i < CASES; i++) {
    const events = gen(rng, Math.floor(rng() * MAX_EVENTS_PER_CASE));
    const s = calculateSupersocietyScore(events);
    let weightedSum = 0;
    let weightTotal = 0;
    for (const c of s.categories) {
      weightedSum += c.score * c.weight;
      weightTotal += c.weight;
    }
    const expected = weightTotal > 0 ? Math.round(weightedSum / weightTotal) : 100;
    if (Math.abs(expected - s.composite) > 0) {
      failures += 1;
      if (firstFail === null) firstFail = `expected ${expected}, got ${s.composite}`;
    }
  }
  assert(failures === 0, 'composite = weighted average of category scores',
    `${failures}/${CASES}. First: ${firstFail}`);
}

// --- Summary
console.log('=== supersocietyScore.property.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
for (const p of PASSED) console.log('  ✓ ' + p);
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  for (const f of FAILED) console.log(`  ✗ ${f.name}\n    ${f.reason}`);
  process.exit(1);
}
console.log(`All ${PASSED.length} property scenarios passed (200 cases each where applicable).`);
