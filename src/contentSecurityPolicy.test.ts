/**
 * contentSecurityPolicy.test.ts — pure-function tests for the
 * CSP detector (T76).
 */
import {
  buildCspSnapshot,
  detectCspIssues,
} from './contentSecurityPolicy.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const HARDENED_BASE =
  "default-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'; script-src 'self'";

// 1. No header → missing warn.
{
  const s = buildCspSnapshot('https://example.com/', {});
  const f = detectCspIssues(s);
  assert(
    f.length === 1 && f[0].kind === 'csp.missing' && f[0].severity === 'warn',
    'no header → missing warn',
    JSON.stringify(f),
  );
}

// 2. Localhost exempt.
{
  const s = buildCspSnapshot('https://localhost:3000/', {});
  const f = detectCspIssues(s);
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Hardened baseline → no findings.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': HARDENED_BASE,
  });
  const f = detectCspIssues(s);
  assert(f.length === 0, 'hardened baseline clean', JSON.stringify(f));
}

// 4. Garbage → invalid warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': '   ;   ;   ',
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.invalid'),
    'garbage → invalid warn',
    JSON.stringify(f),
  );
}

// 5. script-src 'unsafe-inline' → strict.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': HARDENED_BASE.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'"),
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.script-unsafe-inline' && x.severity === 'strict'),
    'script-src unsafe-inline → strict',
    JSON.stringify(f),
  );
}

// 6. script-src 'unsafe-eval' → strict.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': HARDENED_BASE.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'"),
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.script-unsafe-eval' && x.severity === 'strict'),
    'script-src unsafe-eval → strict',
    JSON.stringify(f),
  );
}

// 7. script-src wildcard '*' → strict.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': HARDENED_BASE.replace("script-src 'self'", 'script-src *'),
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.script-wildcard' && x.severity === 'strict'),
    'script-src wildcard → strict',
    JSON.stringify(f),
  );
}

// 8. script-src 'https:' (scheme-only) → strict (treated as wildcard).
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': HARDENED_BASE.replace("script-src 'self'", "script-src 'self' https:"),
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.script-wildcard'),
    'script-src https: → strict wildcard',
    JSON.stringify(f),
  );
}

// 9. No default-src AND no script-src → no-default-src warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.no-default-src'),
    'no default-src + no script-src → warn',
    JSON.stringify(f),
  );
}

// 10. Has default-src but no script-src → fallback works, no script-related warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'",
  });
  const f = detectCspIssues(s);
  assert(
    !f.some((x) => x.kind === 'csp.no-default-src'),
    'default-src present, no script-src → no warn',
    JSON.stringify(f),
  );
}

// 11. default-src fallback inherits unsafe-inline.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.script-unsafe-inline'),
    'default-src unsafe-inline → fires script-unsafe-inline via fallback',
    JSON.stringify(f),
  );
}

// 12. No object-src → warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'; script-src 'self'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.no-object-src'),
    'no object-src → warn',
    JSON.stringify(f),
  );
}

// 13. No base-uri → warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'; script-src 'self'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.no-base-uri'),
    'no base-uri → warn',
    JSON.stringify(f),
  );
}

// 14. No form-action → warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; require-trusted-types-for 'script'; script-src 'self'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.no-form-action'),
    'no form-action → warn',
    JSON.stringify(f),
  );
}

// 15. No frame-ancestors → warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; require-trusted-types-for 'script'; script-src 'self'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.no-frame-ancestors'),
    'no frame-ancestors → warn',
    JSON.stringify(f),
  );
}

// 16. No trusted-types → warn.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'; script-src 'self'",
  });
  const f = detectCspIssues(s);
  assert(
    f.some((x) => x.kind === 'csp.no-trusted-types'),
    'no trusted-types → warn',
    JSON.stringify(f),
  );
}

// 17. Header name case-insensitive.
{
  const s = buildCspSnapshot('https://example.com/', {
    'Content-Security-Policy': HARDENED_BASE,
  });
  assert(s.raw === HARDENED_BASE, 'header name case-insensitive', JSON.stringify(s));
}

// 18. Trailing semicolon tolerated.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': HARDENED_BASE + ';',
  });
  const f = detectCspIssues(s);
  assert(f.length === 0, 'trailing semicolon tolerated', JSON.stringify(f));
}

// 19. Worst case — missing CSP scores ONE warn (the missing one);
// don't pile every other check on top.
{
  const s = buildCspSnapshot('https://example.com/', {});
  const f = detectCspIssues(s);
  assert(
    f.length === 1,
    'missing header short-circuits — one finding only',
    JSON.stringify(f),
  );
}

// 20. Worst case — declared but with EVERY defect simultaneously.
{
  const s = buildCspSnapshot('https://example.com/', {
    'content-security-policy': "script-src 'unsafe-inline' 'unsafe-eval' *",
  });
  const f = detectCspIssues(s);
  const kinds = new Set(f.map((x) => x.kind));
  const hasAll =
    kinds.has('csp.script-unsafe-inline') &&
    kinds.has('csp.script-unsafe-eval') &&
    kinds.has('csp.script-wildcard') &&
    kinds.has('csp.no-object-src') &&
    kinds.has('csp.no-base-uri') &&
    kinds.has('csp.no-form-action') &&
    kinds.has('csp.no-frame-ancestors') &&
    kinds.has('csp.no-trusted-types');
  assert(hasAll, 'pile-on policy fires every relevant finding', JSON.stringify([...kinds]));
}

console.log('\n=== contentSecurityPolicy.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
