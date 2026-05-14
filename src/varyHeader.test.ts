/**
 * varyHeader.test.ts — Vary correctness detector tests. T76 cycle 29.
 */
import { buildVarySnapshot, detectVaryIssues } from './varyHeader.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

// 1. No headers → no findings (nothing to vary on).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {}));
  assert(f.length === 0, 'no headers → no findings', JSON.stringify(f));
}

// 2. Localhost exempt.
{
  const f = detectVaryIssues(buildVarySnapshot('https://localhost:3000/', {
    'set-cookie': 'sid=abc',
    'cache-control': 'public',
  }));
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Set-Cookie + cacheable + no Vary at all → warn.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'set-cookie': 'sid=abc',
    'cache-control': 'max-age=600',
  }));
  assert(
    f.some((x) => x.kind === 'vary.no-cookie-with-set-cookie-and-cacheable'),
    'Set-Cookie + cacheable + no Vary → warn',
    JSON.stringify(f),
  );
}

// 4. Set-Cookie + Vary: Cookie → no finding.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'set-cookie': 'sid=abc',
    'cache-control': 'max-age=600',
    'vary': 'Cookie',
  }));
  assert(f.length === 0, 'Set-Cookie + Vary: Cookie clean', JSON.stringify(f));
}

// 5. Set-Cookie + no-store → no finding (uncacheable).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'set-cookie': 'sid=abc',
    'cache-control': 'no-store',
  }));
  assert(f.length === 0, 'no-store short-circuits', JSON.stringify(f));
}

// 6. Set-Cookie + private → no finding (uncacheable to shared caches).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'set-cookie': 'sid=abc',
    'cache-control': 'private, max-age=600',
  }));
  assert(f.length === 0, 'private short-circuits', JSON.stringify(f));
}

// 7. Vary: * → warn (use Cache-Control: no-store instead).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': '*',
  }));
  assert(
    f.some((x) => x.kind === 'vary.star'),
    'Vary: * → warn',
    JSON.stringify(f),
  );
}

// 8. Vary: * with Set-Cookie does NOT trigger no-cookie-warn (star covers it).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': '*',
    'set-cookie': 'sid=abc',
    'cache-control': 'max-age=60',
  }));
  assert(
    !f.some((x) => x.kind === 'vary.no-cookie-with-set-cookie-and-cacheable'),
    'Vary: * covers cookie key',
    JSON.stringify(f),
  );
  assert(
    f.some((x) => x.kind === 'vary.star'),
    'Vary: * still flagged with star warn',
    JSON.stringify(f),
  );
}

// 9. Vary: invalid tokens (commas in wrong place produce empty tokens).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': '@@@invalid_token@@@',
  }));
  assert(
    f.some((x) => x.kind === 'vary.invalid'),
    'invalid token → invalid warn',
    JSON.stringify(f),
  );
}

// 10. Vary: duplicate-token (case-insensitive).
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': 'Cookie, cookie, Accept-Language',
  }));
  assert(
    f.some((x) => x.kind === 'vary.duplicate-tokens'),
    'duplicate tokens → warn',
    JSON.stringify(f),
  );
}

// 11. Vary: Cookie alone, no Set-Cookie → no finding.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': 'Cookie',
  }));
  assert(f.length === 0, 'Vary: Cookie alone clean', JSON.stringify(f));
}

// 12. Header name case-insensitive.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'Vary': 'Cookie',
    'Set-Cookie': 'sid=abc',
    'Cache-Control': 'max-age=600',
  }));
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 13. Vary: Accept-Language honored as cookie key when cookie also present? NO —
// only `cookie` token covers Set-Cookie variation. Accept-Language alone doesn't.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': 'Accept-Language',
    'set-cookie': 'sid=abc',
    'cache-control': 'max-age=600',
  }));
  assert(
    f.some((x) => x.kind === 'vary.no-cookie-with-set-cookie-and-cacheable'),
    'Vary: Accept-Language alone does not cover Cookie',
    JSON.stringify(f),
  );
}

// 14. Multiple legitimate Vary tokens including cookie → no finding.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': 'Accept-Encoding, Cookie, Accept-Language',
    'set-cookie': 'sid=abc',
    'cache-control': 'max-age=600',
  }));
  assert(f.length === 0, 'multi-token Vary including Cookie clean', JSON.stringify(f));
}

// 15. No Cache-Control at all + Set-Cookie → cacheable (heuristic) → warn.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'set-cookie': 'sid=abc',
  }));
  assert(
    f.some((x) => x.kind === 'vary.no-cookie-with-set-cookie-and-cacheable'),
    'no Cache-Control + Set-Cookie → still warn (default cacheable)',
    JSON.stringify(f),
  );
}

// 16. Whitespace tolerated.
{
  const f = detectVaryIssues(buildVarySnapshot(PAGE, {
    'vary': '   Cookie   ,    Accept-Language   ',
    'set-cookie': 'sid=abc',
    'cache-control': 'max-age=600',
  }));
  assert(f.length === 0, 'whitespace-padded tokens parsed', JSON.stringify(f));
}

console.log('\n=== varyHeader.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
