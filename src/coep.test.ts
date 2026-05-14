/**
 * coep.test.ts — Cross-Origin-Embedder-Policy detector tests. T76.
 */
import { buildCoepSnapshot, detectCoepIssues } from './coep.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. No header → missing.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {}));
  assert(f.length === 1 && f[0].kind === 'coep.missing', 'no header → missing', JSON.stringify(f));
}

// 2. Localhost exempt.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://localhost:3000/', {}));
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. require-corp → no findings.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'cross-origin-embedder-policy': 'require-corp',
  }));
  assert(f.length === 0, 'require-corp clean', JSON.stringify(f));
}

// 4. credentialless → no findings.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'cross-origin-embedder-policy': 'credentialless',
  }));
  assert(f.length === 0, 'credentialless clean', JSON.stringify(f));
}

// 5. unsafe-none → explicit warn.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'cross-origin-embedder-policy': 'unsafe-none',
  }));
  assert(f.length === 1 && f[0].kind === 'coep.unsafe-none', 'unsafe-none warn', JSON.stringify(f));
}

// 6. invalid value → warn.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'cross-origin-embedder-policy': 'wat',
  }));
  assert(f.length === 1 && f[0].kind === 'coep.invalid', 'invalid value warn', JSON.stringify(f));
}

// 7. Value case-insensitive.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'cross-origin-embedder-policy': 'REQUIRE-CORP',
  }));
  assert(f.length === 0, 'value case-insensitive', JSON.stringify(f));
}

// 8. Header name case-insensitive.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'Cross-Origin-Embedder-Policy': 'require-corp',
  }));
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 9. Whitespace tolerated.
{
  const f = detectCoepIssues(buildCoepSnapshot('https://example.com/', {
    'cross-origin-embedder-policy': '  require-corp  ',
  }));
  assert(f.length === 0, 'whitespace tolerated', JSON.stringify(f));
}

console.log('\n=== coep.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
