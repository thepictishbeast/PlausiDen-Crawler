/**
 * placeholderText.test.ts — pure-function tests for
 * detectPlaceholderTextIssues. Browser-eval (snapshot capture)
 * is exercised via the live crawler journey, not here.
 */
import {
  detectPlaceholderTextIssues,
  type PlaceholderTextSnapshot,
  type PlaceholderHit,
} from './placeholderText.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (hits: PlaceholderHit[] = []): PlaceholderTextSnapshot => ({
  pageUrl: 'http://t/',
  hits,
  scannedChars: 1234,
});

const hit = (overrides: Partial<PlaceholderHit> = {}): PlaceholderHit => ({
  selector: 'body > main > p',
  category: 'dev-marker',
  match: 'TODO',
  context: 'TODO: write copy',
  ...overrides,
});

// 1. Clean snapshot — no findings.
{
  const f = detectPlaceholderTextIssues(snap());
  assert(f.length === 0, 'clean snapshot emits no findings', JSON.stringify(f));
}

// 2. Single TODO → strict dev-marker finding.
{
  const f = detectPlaceholderTextIssues(snap([hit()]));
  assert(
    f.length === 1 &&
      f[0].kind === 'placeholder.dev-marker' &&
      f[0].severity === 'strict',
    'single TODO emits strict dev-marker finding',
    JSON.stringify(f),
  );
}

// 3. Lorem ipsum → strict lorem-ipsum finding.
{
  const f = detectPlaceholderTextIssues(
    snap([hit({ category: 'lorem-ipsum', match: 'Lorem ipsum' })]),
  );
  assert(
    f.length === 1 &&
      f[0].kind === 'placeholder.lorem-ipsum' &&
      f[0].severity === 'strict',
    'lorem ipsum emits strict finding',
    JSON.stringify(f),
  );
}

// 4. Coming-soon → warn, not strict.
{
  const f = detectPlaceholderTextIssues(
    snap([hit({ category: 'coming-soon', match: 'coming soon' })]),
  );
  assert(
    f.length === 1 &&
      f[0].kind === 'placeholder.coming-soon' &&
      f[0].severity === 'warn',
    'coming-soon downgraded to warn',
    JSON.stringify(f),
  );
}

// 5. Multiple hits in same category → single bucketed finding with count.
{
  const hits: PlaceholderHit[] = [];
  for (let i = 0; i < 7; i++) {
    hits.push(hit({ selector: `body > main > p:nth-of-type(${i + 1})` }));
  }
  const f = detectPlaceholderTextIssues(snap(hits));
  assert(
    f.length === 1 &&
      f[0].kind === 'placeholder.dev-marker' &&
      (f[0].evidence as { count: number }).count === 7,
    'multiple same-category hits collapse to one bucketed finding',
    JSON.stringify(f),
  );
}

// 6. Mixed categories → one finding per category, each with own severity.
{
  const hits: PlaceholderHit[] = [
    hit({ category: 'lorem-ipsum', match: 'lorem ipsum' }),
    hit({ category: 'dev-marker', match: 'FIXME' }),
    hit({ category: 'coming-soon', match: 'coming soon' }),
    hit({ category: 'template', match: 'sample text' }),
  ];
  const f = detectPlaceholderTextIssues(snap(hits));
  assert(f.length === 4, 'four distinct categories → four findings', `${f.length}`);
  const strictCount = f.filter((x) => x.severity === 'strict').length;
  const warnCount = f.filter((x) => x.severity === 'warn').length;
  assert(
    strictCount === 3 && warnCount === 1,
    'three strict (lorem/dev/template), one warn (coming-soon)',
    `strict=${strictCount} warn=${warnCount}`,
  );
}

// 7. Examples capped at 5 even with many hits.
{
  const hits: PlaceholderHit[] = [];
  for (let i = 0; i < 15; i++) hits.push(hit({ match: `TODO #${i}` }));
  const f = detectPlaceholderTextIssues(snap(hits));
  const evidence = f[0].evidence as { count: number; examples: PlaceholderHit[] };
  assert(
    evidence.count === 15 && evidence.examples.length === 5,
    'evidence.examples capped at 5 while count reflects all hits',
    JSON.stringify(evidence),
  );
}

// 8. Template category strict severity sanity-check.
{
  const f = detectPlaceholderTextIssues(
    snap([hit({ category: 'template', match: 'delete me' })]),
  );
  assert(
    f.length === 1 && f[0].severity === 'strict' && f[0].kind === 'placeholder.template',
    'template category fires strict',
    JSON.stringify(f),
  );
}

console.log('\n=== placeholderText.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
