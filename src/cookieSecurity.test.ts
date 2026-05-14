/**
 * cookieSecurity.test.ts — pure-function tests for the
 * Set-Cookie attribute audit (T76).
 */
import {
  buildCookieSecuritySnapshot,
  detectCookieSecurityIssues,
} from './cookieSecurity.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. No cookies → no findings.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {});
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'no cookies — no findings', JSON.stringify(f));
}

// 2. localhost exempt.
{
  const s = buildCookieSecuritySnapshot('https://localhost:3000/', {
    'set-cookie': 'sess=abc',
  });
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. Properly-set https cookie (Secure + SameSite=Strict + HttpOnly) → clean.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'sid=abc; Secure; HttpOnly; SameSite=Strict; Path=/',
  });
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'fully-secured cookie clean', JSON.stringify(f));
}

// 4. https + missing Secure → strict no-secure.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'foo=bar; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  assert(
    f.some((x) => x.kind === 'cookie.no-secure' && x.severity === 'strict'),
    'https without Secure → strict',
    JSON.stringify(f),
  );
}

// 5. http + missing Secure → no no-secure finding (Secure can't apply).
{
  const s = buildCookieSecuritySnapshot('http://example.com/', {
    'set-cookie': 'foo=bar; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  assert(
    !f.some((x) => x.kind === 'cookie.no-secure'),
    'http does not fire no-secure',
    JSON.stringify(f),
  );
}

// 6. Missing SameSite → warn.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'foo=bar; Secure',
  });
  const f = detectCookieSecurityIssues(s);
  assert(
    f.some((x) => x.kind === 'cookie.no-samesite' && x.severity === 'warn'),
    'no SameSite → warn',
    JSON.stringify(f),
  );
}

// 7. SameSite=None without Secure → strict samesite-none-no-secure.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'cross=ok; SameSite=None',
  });
  const f = detectCookieSecurityIssues(s);
  assert(
    f.some((x) => x.kind === 'cookie.samesite-none-no-secure' && x.severity === 'strict'),
    'SameSite=None without Secure → strict',
    JSON.stringify(f),
  );
}

// 8. Session-named cookie without HttpOnly → warn.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'sessid=abc; Secure; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  assert(
    f.some((x) => x.kind === 'cookie.session-no-httponly' && x.severity === 'warn'),
    'session name + no HttpOnly → warn',
    JSON.stringify(f),
  );
}

// 9. Non-session cookie without HttpOnly → no session-httponly finding.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'theme=dark; Secure; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  assert(
    !f.some((x) => x.kind === 'cookie.session-no-httponly'),
    'non-session name does not trip session-httponly',
    JSON.stringify(f),
  );
}

// 10. Multiple Set-Cookie headers (newline-separated, Playwright form).
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'sid=abc; Secure; HttpOnly; SameSite=Strict\nsettings=dark; Secure; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'two clean cookies via newline-split', JSON.stringify(f));
}

// 11. Aggregation: 3 cookies all missing Secure → 1 finding count=3.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie':
      'a=1; SameSite=Lax\nb=2; SameSite=Lax\nc=3; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  const noSec = f.find((x) => x.kind === 'cookie.no-secure');
  assert(
    !!noSec && (noSec.evidence.count as number) === 3,
    'aggregates to count=3',
    JSON.stringify(f),
  );
}

// 12. Case-insensitive header NAME (HTTP/2 lowercases).
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'Set-Cookie': 'foo=bar; Secure; SameSite=Lax',
  });
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 13. Case-insensitive ATTRIBUTE.
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': 'foo=bar; SECURE; HTTPONLY; SAMESITE=STRICT',
  });
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'attribute names case-insensitive', JSON.stringify(f));
}

// 14. Examples capped at 5.
{
  let lines = '';
  for (let i = 0; i < 8; i++) {
    if (i > 0) lines += '\n';
    lines += `c${i}=v; SameSite=Lax`;
  }
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': lines,
  });
  const f = detectCookieSecurityIssues(s);
  const noSec = f.find((x) => x.kind === 'cookie.no-secure');
  const examples = noSec?.evidence.examples as string[];
  assert(examples.length === 5, 'examples capped at 5', JSON.stringify(examples));
  assert(
    (noSec?.evidence.count as number) === 8,
    'count still reflects all 8',
    JSON.stringify(noSec?.evidence),
  );
}

// 15. Malformed Set-Cookie skipped (no `=` before separator).
{
  const s = buildCookieSecuritySnapshot('https://example.com/', {
    'set-cookie': '; Secure; HttpOnly',
  });
  const f = detectCookieSecurityIssues(s);
  assert(f.length === 0, 'malformed skipped silently', JSON.stringify(f));
}

console.log('\n=== cookieSecurity.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
