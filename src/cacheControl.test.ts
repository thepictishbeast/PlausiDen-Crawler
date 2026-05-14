/**
 * cacheControl.test.ts — Cache-Control hygiene detector tests.
 * T76 cycle 28.
 */
import {
  buildCacheControlSnapshot,
  detectCacheControlIssues,
} from './cacheControl.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

// 1. No header → missing.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {}));
  assert(f.length === 1 && f[0].kind === 'cache-control.missing', 'no header → missing', JSON.stringify(f));
}

// 2. Localhost exempt.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot('https://localhost:3000/', {}));
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Public + Set-Cookie → strict cache-deception risk.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'public, max-age=3600',
    'set-cookie': 'sid=abc',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.public-with-cookie' && x.severity === 'strict'),
    'public + Set-Cookie → strict',
    JSON.stringify(f),
  );
}

// 4. Set-Cookie without no-store/private → warn.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'max-age=600',
    'set-cookie': 'sid=abc',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.no-private-with-cookie'),
    'Set-Cookie without no-store/private → warn',
    JSON.stringify(f),
  );
}

// 5. Set-Cookie with no-store → no finding (correct).
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'no-store',
    'set-cookie': 'sid=abc',
  }));
  assert(f.length === 0, 'no-store + Set-Cookie clean', JSON.stringify(f));
}

// 6. Set-Cookie with private → no finding (correct).
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'private, max-age=300',
    'set-cookie': 'sid=abc',
  }));
  assert(f.length === 0, 'private + Set-Cookie clean', JSON.stringify(f));
}

// 7. Cache-Control invalid (garbage, no =) → invalid.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': '   ,   ,   ',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.invalid'),
    'garbage → invalid warn',
    JSON.stringify(f),
  );
}

// 8. max-age > 1 year → unrealistic-maxage.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'public, max-age=99999999',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.unrealistic-maxage'),
    'max-age > 1 year → warn',
    JSON.stringify(f),
  );
}

// 9. max-age non-numeric → invalid.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'max-age=abc',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.invalid'),
    'non-numeric max-age → invalid',
    JSON.stringify(f),
  );
}

// 10. no-store + max-age → contradictory.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'no-store, max-age=60',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.contradictory'),
    'no-store + max-age → contradictory',
    JSON.stringify(f),
  );
}

// 11. public + private → contradictory.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'public, private',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.contradictory'),
    'public + private → contradictory',
    JSON.stringify(f),
  );
}

// 12. no-cache + immutable → contradictory.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'no-cache, immutable',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.contradictory'),
    'no-cache + immutable → contradictory',
    JSON.stringify(f),
  );
}

// 13. no-store + immutable → contradictory.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'no-store, immutable',
  }));
  assert(
    f.some((x) => x.kind === 'cache-control.contradictory'),
    'no-store + immutable → contradictory',
    JSON.stringify(f),
  );
}

// 14. Header name case-insensitive.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'Cache-Control': 'no-store',
  }));
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 15. Quoted max-age value tolerated.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'max-age="3600"',
  }));
  assert(
    !f.some((x) => x.kind === 'cache-control.invalid'),
    'quoted max-age accepted',
    JSON.stringify(f),
  );
}

// 16. Whitespace inside directives tolerated.
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': '  no-store  ,   max-age = 60  ',
  }));
  // Note: contradictory will fire (no-store + max-age) but not invalid.
  assert(
    !f.some((x) => x.kind === 'cache-control.invalid'),
    'whitespace tolerated',
    JSON.stringify(f),
  );
}

// 17. Static asset with public + max-age=long → no finding (legitimate).
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'public, max-age=31536000, immutable',
  }));
  assert(f.length === 0, 'static asset clean', JSON.stringify(f));
}

// 18. SPA HTML with no-store → no finding (legitimate).
{
  const f = detectCacheControlIssues(buildCacheControlSnapshot(PAGE, {
    'cache-control': 'no-store',
  }));
  assert(f.length === 0, 'SPA HTML clean', JSON.stringify(f));
}

console.log('\n=== cacheControl.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
