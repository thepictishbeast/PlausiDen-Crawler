/**
 * supersocietyScore.drift.test.ts — automated drift detection
 * between detector-emitted event kinds and the
 * supersocietyScore module's KIND_TO_CATEGORY map.
 *
 * T76 cycle 74 (Tier 6).
 *
 * Cycle 73 surfaced 5 silent score-inflation bugs MANUALLY:
 * detector kinds emitted by main.ts but missing from
 * KIND_TO_CATEGORY. They fell into `unbucketed`, never
 * penalized the score. Cycle 74 makes this drift impossible
 * to ship by failing this test if any unmapped kind appears
 * in the source.
 *
 * Algorithm:
 *
 *   1. Read every `.ts` file under `src/` (recursive).
 *   2. Strip line + block comments so commented-out
 *      `kind: 'X'` references don't false-positive.
 *   3. Grep for the literal pattern
 *        `kind: 'X'` OR `kind: "X"`
 *      where X is a-z + - underscores. Capture X.
 *   4. Also accept `e.kind === 'X'` patterns — those are
 *      detector-implementation references; if they exist
 *      in the source the kind is real.
 *   5. Subtract:
 *        - kinds in KIND_TO_CATEGORY (mapped — fine).
 *        - the small allow-list of NON-finding kinds
 *          (`discover`, `goto`, `screenshot`, `wait`,
 *           `start`, `end` — journey-step labels, not
 *           detector findings).
 *   6. Any remaining kind is a DRIFT BUG. Fail with a
 *      message that names the kind + suggests a category.
 *
 * This is the Tier-6 "validator of validators of validators":
 *   property tests   → math bugs
 *   mutation tests   → gaps in property tests
 *   drift tests      → unmapped kinds the tests above can't
 *                       see (because the kind doesn't appear
 *                       in any randomised input)
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const SRC = new URL('.', import.meta.url).pathname.replace(/\/$/, '');

/**
 * Non-finding event kinds — these are step / lifecycle
 * markers from the journey runner, not detector findings.
 * Adding to this list is a DELIBERATE acknowledgement that
 * the kind is excluded from scoring.
 */
const NON_FINDING_KINDS = new Set([
  // Journey-step lifecycle (main.ts emits these to record
  // what the runner did; never penalize).
  'discover',
  'goto',
  'screenshot',
  'wait',
  'start',
  'end',
  'press',
  'click',
  'fill',
  'submit',
  'sw-online',
  'sw-offline',
  // Internal noise kinds the runner uses for its own log;
  // separate from detector findings.
  'crawler',
  'crawler-error',
  // T76 cycle 50+: detector status pings (not findings).
  'detector-loaded',
  // Journey step kinds (StepKind union in journey.ts).
  'type',
  'waitForSelector',
  'assertText',
  'scroll',
  'reload',
  'probe',
  // BrokenResource.kind union (telemetry.ts type, not event
  // stream): image / script / stylesheet / font / manifest /
  // other are RESOURCE kinds, never logged as score events.
  'image',
  'script',
  'stylesheet',
  'font',
  'manifest',
  'other',
]);

/** Recursive directory walk yielding *.ts file paths. */
function walkTs(dir: string): string[] {
  const out: string[] = [];
  let entries: string[] = [];
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const name of entries) {
    const p = join(dir, name);
    let s;
    try {
      s = statSync(p);
    } catch {
      continue;
    }
    if (s.isDirectory()) {
      // Skip node_modules / dist / build artefacts.
      if (name === 'node_modules' || name === 'dist' || name === '.git') continue;
      out.push(...walkTs(p));
    } else if (s.isFile() && (name.endsWith('.ts') || name.endsWith('.js'))) {
      // Skip test files — they use placeholder kinds
      // (`fake`, `probe`, `a`, `b`, `c`, etc.) that don't
      // correspond to real detector findings.
      if (name.endsWith('.test.ts') || name.endsWith('.test.js')
          || name.endsWith('.property.test.ts')
          || name.endsWith('.mutation.test.ts')
          || name.endsWith('.drift.test.ts')) {
        continue;
      }
      out.push(p);
    }
  }
  return out;
}

/** Strip line + block comments from TS source. */
function stripComments(src: string): string {
  // Block comments — non-greedy, multiline.
  let out = src.replace(/\/\*[\s\S]*?\*\//g, '');
  // Line comments — but NOT URLs (https://) — require the
  // `//` to either start the line OR be preceded by whitespace
  // / semicolon / open-paren / brace.
  out = out.replace(/(^|[\s;({[,])\/\/[^\n]*/g, '$1');
  return out;
}

/** Extract `kind: 'X'` and `kind === 'X'` references. */
function extractKinds(src: string): Set<string> {
  const found = new Set<string>();
  // Match kind: 'X' OR kind: "X"
  const re1 = /kind\s*:\s*['"]([a-z][a-z0-9-]*)['"]/g;
  // Match e.kind === 'X' (detector implementations)
  const re2 = /\.kind\s*===?\s*['"]([a-z][a-z0-9-]*)['"]/g;
  let m;
  while ((m = re1.exec(src))) found.add(m[1]);
  while ((m = re2.exec(src))) found.add(m[1]);
  return found;
}

/** Extract keys of the KIND_TO_CATEGORY map from the source. */
function extractMappedKinds(src: string): Set<string> {
  const found = new Set<string>();
  // The map literal looks like:
  //   const KIND_TO_CATEGORY: ... = {
  //     'hsts': 'transportSecurity',
  //     ...
  //   };
  // Match between `KIND_TO_CATEGORY: ... = {` and the closing
  // `};` then pick out 'X': pairs.
  const match = src.match(
    /KIND_TO_CATEGORY[^=]*=\s*\{([\s\S]*?)\n\};/,
  );
  if (!match) return found;
  const body = stripComments(match[1]);
  const re = /['"]([a-z][a-z0-9-]*)['"]\s*:/g;
  let m;
  while ((m = re.exec(body))) found.add(m[1]);
  return found;
}

/**
 * T76 cycle 78: extract the CapturedEvent.kind union members
 * from report.ts.
 *
 * The TS source declares:
 *   export interface CapturedEvent {
 *     kind:
 *       | 'console'
 *       | 'pageerror'
 *       ...
 *       | 'cross-page-meta-description';
 *     ...
 *   }
 *
 * The walker grabs the union by finding `kind:` followed by
 * `|` lines until a `;` closes the union. Each `'X'` literal
 * gets captured.
 */
function extractTypedKinds(src: string): Set<string> {
  const found = new Set<string>();
  // Match `kind:` followed by a multi-line union ending in `;`.
  const m = src.match(/kind\s*:\s*((?:\s*\|\s*['"][a-z0-9-]+['"]\s*)+);/);
  if (!m) return found;
  const re = /['"]([a-z][a-z0-9-]*)['"]/g;
  let lit;
  while ((lit = re.exec(m[1]))) found.add(lit[1]);
  return found;
}

// --- Run.
{
  const files = walkTs(SRC);
  // Aggregate kinds emitted across the entire source tree.
  let emitted = new Set<string>();
  for (const f of files) {
    let txt: string;
    try {
      txt = readFileSync(f, 'utf8');
    } catch {
      continue;
    }
    const stripped = stripComments(txt);
    for (const k of extractKinds(stripped)) emitted.add(k);
  }
  assert(emitted.size > 10, 'extracted detector kinds from source',
    `expected many; found ${emitted.size}`);

  // Load the score module to extract its mapped kinds.
  const scoreSrc = readFileSync(
    join(SRC, 'supersocietyScore.ts'), 'utf8');
  const mapped = extractMappedKinds(stripComments(scoreSrc));
  assert(mapped.size > 30, 'extracted KIND_TO_CATEGORY map',
    `expected >30 mappings; found ${mapped.size}`);

  // Compute the diff.
  const drift: string[] = [];
  for (const k of emitted) {
    if (mapped.has(k)) continue;
    if (NON_FINDING_KINDS.has(k)) continue;
    drift.push(k);
  }
  drift.sort();

  if (drift.length === 0) {
    PASSED.push('no drift between emitted kinds and KIND_TO_CATEGORY');
  } else {
    FAILED.push({
      name: 'drift between emitted kinds and KIND_TO_CATEGORY',
      reason: `${drift.length} kind(s) emit events but are not mapped in KIND_TO_CATEGORY ` +
        `(silent score inflation): ${JSON.stringify(drift)}\n\n` +
        `To fix: either map each kind to a category in supersocietyScore.ts ` +
        `KIND_TO_CATEGORY, OR add to NON_FINDING_KINDS in this test if the kind is ` +
        `a journey-step lifecycle marker (not a detector finding).`,
    });
  }

  // Reverse check: any kind in KIND_TO_CATEGORY that isn't
  // emitted anywhere is a stale mapping. Lower priority but
  // useful for repo cleanliness.
  const stale: string[] = [];
  for (const k of mapped) {
    if (emitted.has(k)) continue;
    stale.push(k);
  }
  stale.sort();
  if (stale.length === 0) {
    PASSED.push('no stale entries in KIND_TO_CATEGORY');
  } else {
    // Stale entries don't break correctness — they just
    // mean a detector was renamed/removed. Surface as info
    // (passing), not failure, so the operator can decide.
    PASSED.push(
      `KIND_TO_CATEGORY has ${stale.length} stale entry/entries (kind no longer emitted): ${JSON.stringify(stale).slice(0, 200)}`,
    );
  }

  // T76 cycle 78: cross-check against report.ts's CapturedEvent
  // kind union. Catches the OTHER class of forgotten-update
  // bugs: a kind in KIND_TO_CATEGORY that isn't in the type
  // union (TypeScript can't verify because string-literal
  // events are constructed from `as` casts) AND a kind in
  // the type union that has no category mapping.
  const reportSrc = readFileSync(
    join(SRC, 'report.ts'), 'utf8');
  const typed = extractTypedKinds(stripComments(reportSrc));
  assert(typed.size > 30, 'extracted CapturedEvent.kind union from report.ts',
    `expected >30 typed kinds; found ${typed.size}`);

  // Check 1: every mapped kind must be in the type union.
  // Otherwise the score module penalises events the type
  // says don't exist — the dispatcher's filter
  // `events.filter(e => e.kind === X)` would never fire on
  // them in production code.
  const mappedNotTyped: string[] = [];
  for (const k of mapped) {
    if (typed.has(k)) continue;
    if (NON_FINDING_KINDS.has(k)) continue;
    mappedNotTyped.push(k);
  }
  mappedNotTyped.sort();
  if (mappedNotTyped.length === 0) {
    PASSED.push('every KIND_TO_CATEGORY entry is in the CapturedEvent type union');
  } else {
    FAILED.push({
      name: 'KIND_TO_CATEGORY entry missing from CapturedEvent type union',
      reason: `${mappedNotTyped.length} kind(s) mapped but NOT in report.ts CapturedEvent.kind ` +
        `union: ${JSON.stringify(mappedNotTyped)}\n\n` +
        `To fix: add the kind to the union in report.ts ` +
        `(or remove from KIND_TO_CATEGORY if the detector was deleted).`,
    });
  }

  // Check 2: every typed kind must be either in the score
  // map OR in NON_FINDING_KINDS. A type-union member with
  // no mapping is silent score inflation in disguise — the
  // type tells you the kind exists, but events with that
  // kind hit the unbucketed pile.
  const typedNotMapped: string[] = [];
  for (const k of typed) {
    if (mapped.has(k)) continue;
    if (NON_FINDING_KINDS.has(k)) continue;
    typedNotMapped.push(k);
  }
  typedNotMapped.sort();
  if (typedNotMapped.length === 0) {
    PASSED.push('every CapturedEvent type-union kind is in KIND_TO_CATEGORY');
  } else {
    FAILED.push({
      name: 'CapturedEvent type-union kind missing from KIND_TO_CATEGORY',
      reason: `${typedNotMapped.length} kind(s) in report.ts CapturedEvent.kind ` +
        `union but NOT mapped to a score category: ${JSON.stringify(typedNotMapped)}\n\n` +
        `To fix: add the kind to KIND_TO_CATEGORY in supersocietyScore.ts ` +
        `(or remove from the union if the detector was deleted).`,
    });
  }
}

console.log('=== supersocietyScore.drift.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
for (const p of PASSED) console.log('  ✓ ' + p);
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  for (const f of FAILED) console.log(`  ✗ ${f.name}\n    ${f.reason}`);
  process.exit(1);
}
console.log(`All ${PASSED.length} drift checks passed.`);
