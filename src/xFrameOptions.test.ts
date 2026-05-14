/**
 * xFrameOptions.test.ts — pure-function tests for the
 * clickjacking-defence detector (T76).
 */
import {
  buildXFrameOptionsSnapshot,
  detectXFrameOptionsIssues,
} from './xFrameOptions.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. http page → never fires.
{
  const s = buildXFrameOptionsSnapshot('http://example.com/', {});
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'http page exempt', JSON.stringify(f));
}

// 2. localhost https → never fires.
{
  const s = buildXFrameOptionsSnapshot('https://localhost:3000/', {});
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. https page with NEITHER header → strict missing.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {});
  const f = detectXFrameOptionsIssues(s);
  assert(
    f.length === 1 && f[0].kind === 'frame-options.missing' && f[0].severity === 'strict',
    'no protection → strict missing',
    JSON.stringify(f),
  );
}

// 4. XFO=DENY → clean.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'x-frame-options': 'DENY',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'XFO=DENY is clean', JSON.stringify(f));
}

// 5. XFO=SAMEORIGIN (case-insensitive) → clean.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'x-frame-options': 'sameorigin',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'sameorigin (lowercase) clean', JSON.stringify(f));
}

// 6. XFO=ALLOW-FROM https://embed.example → clean.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'x-frame-options': 'ALLOW-FROM https://embed.example',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'allow-from <uri> clean', JSON.stringify(f));
}

// 7. XFO=GARBAGE → invalid warn.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'x-frame-options': 'GARBAGE',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(
    f.some((x) => x.kind === 'frame-options.invalid' && x.severity === 'warn'),
    'invalid XFO value → warn',
    JSON.stringify(f),
  );
}

// 8. CSP frame-ancestors 'self' → clean (no XFO needed).
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'content-security-policy': "default-src 'self'; frame-ancestors 'self'",
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, "frame-ancestors 'self' clean", JSON.stringify(f));
}

// 9. CSP frame-ancestors 'none' → clean.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'content-security-policy': "frame-ancestors 'none'",
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, "frame-ancestors 'none' clean", JSON.stringify(f));
}

// 10. CSP frame-ancestors * → warn allowall.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'content-security-policy': 'frame-ancestors *',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(
    f.some((x) => x.kind === 'frame-options.allowall' && x.severity === 'warn'),
    'allowall fires warn',
    JSON.stringify(f),
  );
}

// 11. CSP without frame-ancestors + no XFO → still missing.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'content-security-policy': "default-src 'self'",
  });
  const f = detectXFrameOptionsIssues(s);
  assert(
    f.some((x) => x.kind === 'frame-options.missing'),
    'CSP without frame-ancestors + no XFO → missing',
    JSON.stringify(f),
  );
}

// 12. CSP frame-ancestors supersedes XFO — even broken XFO is
//     fine if CSP is good.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'content-security-policy': "frame-ancestors 'self'",
    'x-frame-options': 'GARBAGE',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(
    f.length === 0,
    'good CSP supersedes invalid XFO',
    JSON.stringify(f),
  );
}

// 13. Header name case-insensitive.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'X-Frame-Options': 'DENY',
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 14. CSP with multiple directives, frame-ancestors among them.
{
  const s = buildXFrameOptionsSnapshot('https://example.com/', {
    'content-security-policy':
      "default-src 'self'; script-src 'self'; frame-ancestors 'self'; img-src 'self' data:",
  });
  const f = detectXFrameOptionsIssues(s);
  assert(f.length === 0, 'multi-directive CSP clean', JSON.stringify(f));
}

console.log('\n=== xFrameOptions.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
