/**
 * infoLeakHeaders.test.ts — opsec hygiene detector tests. T76.
 */
import {
  buildInfoLeakSnapshot,
  detectInfoLeakIssues,
} from './infoLeakHeaders.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

// 1. No headers → no findings.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {}));
  assert(f.length === 0, 'no headers → no findings', JSON.stringify(f));
}

// 2. Localhost exempt.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot('https://localhost:3000/', {
    'server': 'nginx/1.20.1',
    'x-powered-by': 'PHP/7.4.3',
  }));
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Server with version → server-version warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'server': 'nginx/1.20.1',
  }));
  assert(
    f.length === 1 && f[0].kind === 'info-leak.server-version' && f[0].severity === 'warn',
    'Server with version → warn',
    JSON.stringify(f),
  );
}

// 4. Server WITHOUT version → no finding (bare product name).
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'server': 'nginx',
  }));
  assert(f.length === 0, 'bare Server name not flagged', JSON.stringify(f));
}

// 5. Server: cloudflare → no finding.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'server': 'cloudflare',
  }));
  assert(f.length === 0, 'Server: cloudflare not flagged', JSON.stringify(f));
}

// 6. Apache with version + OS → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'server': 'Apache/2.4.41 (Ubuntu)',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.server-version'),
    'Apache version → warn',
    JSON.stringify(f),
  );
}

// 7. X-Powered-By PHP → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-powered-by': 'PHP/7.4.3',
  }));
  assert(
    f.length === 1 && f[0].kind === 'info-leak.x-powered-by',
    'X-Powered-By → warn',
    JSON.stringify(f),
  );
}

// 8. X-Powered-By Express (no version) → still warns.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-powered-by': 'Express',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-powered-by'),
    'X-Powered-By: Express → warn (any value)',
    JSON.stringify(f),
  );
}

// 9. X-AspNet-Version → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-aspnet-version': '4.0.30319',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-aspnet-version'),
    'X-AspNet-Version → warn',
    JSON.stringify(f),
  );
}

// 10. X-AspNetMvc-Version → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-aspnetmvc-version': '5.2',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-aspnetmvc-version'),
    'X-AspNetMvc-Version → warn',
    JSON.stringify(f),
  );
}

// 11. X-Runtime → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-runtime': '0.123456',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-runtime'),
    'X-Runtime → warn',
    JSON.stringify(f),
  );
}

// 12. X-Debug-Token (Symfony) → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-debug-token': 'ab12cd',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-debug-token'),
    'X-Debug-Token → warn',
    JSON.stringify(f),
  );
}

// 13. X-Debug-Token-Link variant → same finding.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-debug-token-link': 'http://example.com/_profiler/ab12cd',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-debug-token'),
    'X-Debug-Token-Link → warn',
    JSON.stringify(f),
  );
}

// 14. Via → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'via': '1.1 internal-proxy.corp.example (varnish/6.0.8)',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.via'),
    'Via → warn',
    JSON.stringify(f),
  );
}

// 15. X-Generator → warn.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'x-generator': 'Drupal 9 (https://www.drupal.org)',
  }));
  assert(
    f.some((x) => x.kind === 'info-leak.x-generator'),
    'X-Generator → warn',
    JSON.stringify(f),
  );
}

// 16. Pile-on: every header set → all 8 findings.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'server': 'nginx/1.20.1',
    'x-powered-by': 'PHP/7.4.3',
    'x-aspnet-version': '4.0.30319',
    'x-aspnetmvc-version': '5.2',
    'x-runtime': '0.123',
    'x-debug-token': 'abc',
    'via': '1.1 proxy',
    'x-generator': 'WordPress 6.0',
  }));
  assert(f.length === 8, 'all 8 headers → 8 findings', `count=${f.length}: ${JSON.stringify(f.map((x) => x.kind))}`);
}

// 17. Header name case-insensitive.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'Server': 'nginx/1.20.1',
    'X-Powered-By': 'PHP/7.4.3',
  }));
  assert(f.length === 2, 'header names case-insensitive', JSON.stringify(f.length));
}

// 18. Unrelated headers ignored.
{
  const f = detectInfoLeakIssues(buildInfoLeakSnapshot(PAGE, {
    'content-type': 'text/html',
    'cache-control': 'no-store',
    'date': 'Thu, 14 May 2026 10:00:00 GMT',
  }));
  assert(f.length === 0, 'unrelated headers ignored', JSON.stringify(f));
}

console.log('\n=== infoLeakHeaders.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
