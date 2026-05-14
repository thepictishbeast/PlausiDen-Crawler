/**
 * corp.test.ts — Cross-Origin-Resource-Policy detector tests.
 * T76 cycle 27.
 */
import { buildCorpSnapshot, detectCorpIssues } from './corp.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/page';

// Helper: build the all-headers Map from a list.
function mapFrom(entries: Array<[string, Record<string, string>]>): Map<string, Record<string, string>> {
  return new Map(entries);
}

// 1. No sub-resources → no findings.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([]));
  assert(detectCorpIssues(snap).length === 0, 'no sub-resources → no findings', '');
}

// 2. Localhost exempt.
{
  const snap = buildCorpSnapshot('https://localhost:3000/', {}, mapFrom([
    ['https://other.example/lib.js', {}],
  ]));
  assert(detectCorpIssues(snap).length === 0, 'localhost exempt', '');
}

// 3. Same-origin sub-resource without CORP → no finding.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://example.com/local.js', {}],
  ]));
  assert(detectCorpIssues(snap).length === 0, 'same-origin sub-resource ignored', '');
}

// 4. Cross-origin sub-resource without CORP, no COEP → warn.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://other.example/lib.js', {}],
  ]));
  const f = detectCorpIssues(snap);
  assert(
    f.length === 1 &&
      f[0].kind === 'corp.cross-origin-resource-no-corp' &&
      f[0].severity === 'warn',
    'cross-origin no CORP, no COEP → warn',
    JSON.stringify(f),
  );
}

// 5. Cross-origin sub-resource without CORP, page COEP=require-corp → strict.
{
  const snap = buildCorpSnapshot(
    PAGE,
    { 'cross-origin-embedder-policy': 'require-corp' },
    mapFrom([['https://other.example/lib.js', {}]]),
  );
  const f = detectCorpIssues(snap);
  assert(
    f.length === 1 &&
      f[0].kind === 'corp.cross-origin-resource-no-corp' &&
      f[0].severity === 'strict',
    'cross-origin no CORP + COEP=require-corp → strict',
    JSON.stringify(f),
  );
  assert(
    typeof f[0].detail === 'string' && f[0].detail.includes('BLOCKS'),
    'strict detail mentions blocking',
    f[0].detail,
  );
}

// 6. Cross-origin with valid CORP → no finding.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://other.example/lib.js', { 'cross-origin-resource-policy': 'cross-origin' }],
  ]));
  assert(detectCorpIssues(snap).length === 0, 'CORP=cross-origin clean', '');
}

// 7. Cross-origin with same-origin CORP → still clean (just restrictive).
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://other.example/lib.js', { 'cross-origin-resource-policy': 'same-origin' }],
  ]));
  assert(detectCorpIssues(snap).length === 0, 'CORP=same-origin still clean', '');
}

// 8. Invalid CORP value → warn.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://other.example/lib.js', { 'cross-origin-resource-policy': 'whatever' }],
  ]));
  const f = detectCorpIssues(snap);
  assert(
    f.some((x) => x.kind === 'corp.cross-origin-resource-invalid'),
    'invalid CORP → warn',
    JSON.stringify(f),
  );
}

// 9. Aggregation: 4 cross-origin sub-resources without CORP → 1 finding count=4.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://other.example/a.js', {}],
    ['https://other.example/b.js', {}],
    ['https://cdn.example.org/c.css', {}],
    ['https://fonts.googleapis.com/css', {}],
  ]));
  const f = detectCorpIssues(snap);
  const fnd = f.find((x) => x.kind === 'corp.cross-origin-resource-no-corp');
  assert(
    fnd !== undefined && (fnd.evidence.count as number) === 4,
    'aggregates count=4',
    JSON.stringify(f),
  );
}

// 10. Examples capped at 5.
{
  const entries: Array<[string, Record<string, string>]> = [];
  for (let i = 0; i < 8; i++) {
    entries.push([`https://other.example/${i}.js`, {}]);
  }
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom(entries));
  const f = detectCorpIssues(snap);
  const fnd = f.find((x) => x.kind === 'corp.cross-origin-resource-no-corp');
  assert(
    fnd && (fnd.evidence.examples as string[]).length === 5,
    'examples capped at 5',
    JSON.stringify(fnd?.evidence),
  );
  assert(
    fnd && (fnd.evidence.count as number) === 8,
    'count still reflects all 8',
    JSON.stringify(fnd?.evidence),
  );
}

// 11. Header name case-insensitive on both page and sub-resource.
{
  const snap = buildCorpSnapshot(
    PAGE,
    { 'Cross-Origin-Embedder-Policy': 'require-corp' },
    mapFrom([
      ['https://other.example/lib.js', { 'Cross-Origin-Resource-Policy': 'cross-origin' }],
    ]),
  );
  assert(detectCorpIssues(snap).length === 0, 'header names case-insensitive', '');
}

// 12. CORP value case-insensitive.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://other.example/lib.js', { 'cross-origin-resource-policy': 'CROSS-ORIGIN' }],
  ]));
  assert(detectCorpIssues(snap).length === 0, 'CORP value case-insensitive', '');
}

// 13. data: / blob: / about: URLs ignored.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['data:text/css,body{}', {}],
    ['blob:https://example.com/x', {}],
    ['about:blank', {}],
  ]));
  assert(detectCorpIssues(snap).length === 0, 'non-http schemes ignored', '');
}

// 14. Mixed: clean cross-origin + missing CORP → only the missing one fires.
{
  const snap = buildCorpSnapshot(PAGE, {}, mapFrom([
    ['https://cdn.example.org/clean.js', { 'cross-origin-resource-policy': 'cross-origin' }],
    ['https://cdn.example.org/dirty.js', {}],
  ]));
  const f = detectCorpIssues(snap);
  const fnd = f.find((x) => x.kind === 'corp.cross-origin-resource-no-corp');
  assert(
    fnd !== undefined && (fnd.evidence.count as number) === 1,
    'mixed → only missing one flagged',
    JSON.stringify(f),
  );
}

// 15. pageRequiresCorp evidence reflected in snapshot.
{
  const snap = buildCorpSnapshot(
    PAGE,
    { 'cross-origin-embedder-policy': 'require-corp' },
    mapFrom([['https://other.example/lib.js', {}]]),
  );
  const f = detectCorpIssues(snap);
  const fnd = f.find((x) => x.kind === 'corp.cross-origin-resource-no-corp');
  assert(
    fnd && fnd.evidence.pageRequiresCorp === true,
    'evidence carries pageRequiresCorp flag',
    JSON.stringify(fnd?.evidence),
  );
}

console.log('\n=== corp.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
