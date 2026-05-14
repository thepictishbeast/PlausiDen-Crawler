/**
 * referrerPolicy.test.ts — pure-function tests for the Referrer-
 * Policy detector (T76).
 */
import {
  buildReferrerPolicySnapshot,
  detectReferrerPolicyIssues,
} from './referrerPolicy.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. http page exempt.
{
  const s = buildReferrerPolicySnapshot('http://example.com/', {});
  const f = detectReferrerPolicyIssues(s);
  assert(f.length === 0, 'http exempt', JSON.stringify(f));
}

// 2. localhost exempt.
{
  const s = buildReferrerPolicySnapshot('https://localhost:3000/', {});
  const f = detectReferrerPolicyIssues(s);
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Missing header → warn missing.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {});
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.length === 1 && f[0].kind === 'referrer-policy.missing' && f[0].severity === 'warn',
    'missing → warn',
    JSON.stringify(f),
  );
}

// 4. strict-origin-when-cross-origin → clean.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'strict-origin-when-cross-origin',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(f.length === 0, 'strict-origin-when-cross-origin clean', JSON.stringify(f));
}

// 5. no-referrer → clean.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'no-referrer',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(f.length === 0, 'no-referrer clean', JSON.stringify(f));
}

// 6. unsafe-url → strict permissive.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'unsafe-url',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'referrer-policy.permissive' && x.severity === 'strict'),
    'unsafe-url → strict permissive',
    JSON.stringify(f),
  );
}

// 7. no-referrer-when-downgrade → strict permissive.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'no-referrer-when-downgrade',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'referrer-policy.permissive'),
    'no-referrer-when-downgrade → permissive',
    JSON.stringify(f),
  );
}

// 8. origin-when-cross-origin → strict permissive.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'origin-when-cross-origin',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'referrer-policy.permissive'),
    'origin-when-cross-origin → permissive',
    JSON.stringify(f),
  );
}

// 9. Case-insensitive header NAME.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'Referrer-Policy': 'no-referrer',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 10. Case-insensitive VALUE.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'STRICT-ORIGIN-WHEN-CROSS-ORIGIN',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(f.length === 0, 'value case-insensitive', JSON.stringify(f));
}

// 11. Invalid token → warn invalid.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'gibberish',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'referrer-policy.invalid'),
    'invalid token → warn invalid',
    JSON.stringify(f),
  );
}

// 12. Multi-token: last recognised wins. The spec lets older
//     clients fall back if they don't understand later tokens.
//     "strict-origin-when-cross-origin, unsafe-url" → permissive
//     (because unsafe-url is the last recognised).
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'strict-origin-when-cross-origin, unsafe-url',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'referrer-policy.permissive'),
    'multi-token: last wins → permissive',
    JSON.stringify(f),
  );
}

// 13. Multi-token where last is unknown: walks back to first
//     recognised — safe.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': 'no-referrer, future-token-not-yet-spec',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.length === 0,
    'multi-token: skips unknown, lands on no-referrer',
    JSON.stringify(f),
  );
}

// 14. Empty token list → invalid.
{
  const s = buildReferrerPolicySnapshot('https://example.com/', {
    'referrer-policy': ',  , ,',
  });
  const f = detectReferrerPolicyIssues(s);
  assert(
    f.some((x) => x.kind === 'referrer-policy.invalid'),
    'empty tokens → invalid',
    JSON.stringify(f),
  );
}

console.log('\n=== referrerPolicy.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
