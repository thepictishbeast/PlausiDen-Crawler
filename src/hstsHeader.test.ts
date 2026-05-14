/**
 * hstsHeader.test.ts — pure-function tests for the HSTS detector
 * (T76). Tests the snapshot-builder + detect path. The
 * page.on('response') header capture is exercised by the audit
 * integration.
 */
import { buildHstsSnapshot, detectHstsIssues } from './hstsHeader.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. http page → never fires.
{
  const s = buildHstsSnapshot('http://example.com/', {});
  const f = detectHstsIssues(s);
  assert(f.length === 0, 'http page — no findings', JSON.stringify(f));
}

// 2. localhost https → never fires (HSTS doesn't apply).
{
  const s = buildHstsSnapshot('https://localhost:3000/', {});
  const f = detectHstsIssues(s);
  assert(f.length === 0, 'https localhost — no findings', JSON.stringify(f));
}

// 3. 127.0.0.1 also exempt.
{
  const s = buildHstsSnapshot('https://127.0.0.1/', {});
  const f = detectHstsIssues(s);
  assert(f.length === 0, '127.0.0.1 exempt', JSON.stringify(f));
}

// 4. https page, no HSTS header → strict missing.
{
  const s = buildHstsSnapshot('https://example.com/', {});
  const f = detectHstsIssues(s);
  assert(
    f.length === 1 && f[0].kind === 'hsts.missing' && f[0].severity === 'strict',
    'no HSTS on https → strict missing',
    JSON.stringify(f),
  );
}

// 5. HSTS present + max-age >= 6 months + includeSubDomains → clean.
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'max-age=31536000; includeSubDomains',
  });
  const f = detectHstsIssues(s);
  assert(f.length === 0, 'good HSTS — clean', JSON.stringify(f));
}

// 6. Case-insensitive header name (HTTP/2 lowercases by spec).
{
  const s = buildHstsSnapshot('https://example.com/', {
    'Strict-Transport-Security': 'max-age=31536000; includeSubDomains',
  });
  const f = detectHstsIssues(s);
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 7. max-age too short → warn.
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'max-age=3600; includeSubDomains',
  });
  const f = detectHstsIssues(s);
  assert(
    f.some((x) => x.kind === 'hsts.max-age-too-short' && x.severity === 'warn'),
    '3600s max-age fires too-short warn',
    JSON.stringify(f),
  );
}

// 8. Boundary: max-age = 6 months exactly → no too-short.
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'max-age=15552000; includeSubDomains',
  });
  const f = detectHstsIssues(s);
  assert(
    !f.some((x) => x.kind === 'hsts.max-age-too-short'),
    'exact 6 months passes',
    JSON.stringify(f),
  );
}

// 9. Adequate max-age but no includeSubDomains → warn.
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'max-age=31536000',
  });
  const f = detectHstsIssues(s);
  assert(
    f.some((x) => x.kind === 'hsts.no-subdomains' && x.severity === 'warn'),
    'no includeSubDomains warn fires',
    JSON.stringify(f),
  );
}

// 10. Short max-age suppresses no-subdomains warn (only emit the
//     bigger problem).
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'max-age=3600',
  });
  const f = detectHstsIssues(s);
  assert(
    f.some((x) => x.kind === 'hsts.max-age-too-short') &&
      !f.some((x) => x.kind === 'hsts.no-subdomains'),
    'short max-age fires too-short, suppresses no-subdomains',
    JSON.stringify(f),
  );
}

// 11. Unparseable header — treat as missing.
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'gibberish',
  });
  const f = detectHstsIssues(s);
  assert(
    f.some((x) => x.kind === 'hsts.missing'),
    'unparseable header → missing',
    JSON.stringify(f),
  );
}

// 12. Directive case-insensitive (`MAX-AGE=...; INCLUDESUBDOMAINS`).
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'MAX-AGE=31536000; INCLUDESUBDOMAINS',
  });
  const f = detectHstsIssues(s);
  assert(f.length === 0, 'uppercase directives parsed', JSON.stringify(f));
}

// 13. Quoted max-age value.
{
  const s = buildHstsSnapshot('https://example.com/', {
    'strict-transport-security': 'max-age="31536000"; includeSubDomains',
  });
  const f = detectHstsIssues(s);
  assert(f.length === 0, 'quoted max-age value parsed', JSON.stringify(f));
}

// 14. .localhost subdomain (e.g. dev.localhost) — exempt.
{
  const s = buildHstsSnapshot('https://dev.localhost/', {});
  const f = detectHstsIssues(s);
  assert(f.length === 0, '.localhost subdomain exempt', JSON.stringify(f));
}

console.log('\n=== hstsHeader.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
