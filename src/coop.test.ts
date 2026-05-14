/**
 * coop.test.ts — Cross-Origin-Opener-Policy detector tests. T76.
 */
import { buildCoopSnapshot, detectCoopIssues } from './coop.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. No header → missing.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {}));
  assert(f.length === 1 && f[0].kind === 'coop.missing', 'no header → missing', JSON.stringify(f));
}

// 2. Localhost exempt.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://localhost:3000/', {}));
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. same-origin → no findings.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': 'same-origin',
  }));
  assert(f.length === 0, 'same-origin clean', JSON.stringify(f));
}

// 4. same-origin-allow-popups → no findings.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': 'same-origin-allow-popups',
  }));
  assert(f.length === 0, 'same-origin-allow-popups clean', JSON.stringify(f));
}

// 5. unsafe-none → explicit warn.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': 'unsafe-none',
  }));
  assert(f.length === 1 && f[0].kind === 'coop.unsafe-none', 'unsafe-none warn', JSON.stringify(f));
}

// 6. invalid value → warn.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': 'whatever',
  }));
  assert(f.length === 1 && f[0].kind === 'coop.invalid', 'invalid value warn', JSON.stringify(f));
}

// 7. Header value case-insensitive.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': 'SAME-ORIGIN',
  }));
  assert(f.length === 0, 'value case-insensitive', JSON.stringify(f));
}

// 8. Header name case-insensitive.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'Cross-Origin-Opener-Policy': 'same-origin',
  }));
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 9. Whitespace tolerated.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': '  same-origin  ',
  }));
  assert(f.length === 0, 'whitespace tolerated', JSON.stringify(f));
}

// 10. same-origin-plus-coep recognised.
{
  const f = detectCoopIssues(buildCoopSnapshot('https://example.com/', {
    'cross-origin-opener-policy': 'same-origin-plus-coep',
  }));
  assert(f.length === 0, 'same-origin-plus-coep recognised', JSON.stringify(f));
}

console.log('\n=== coop.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
