/**
 * inlineScript.test.ts — inline-script + event-handler +
 * javascript: URI detector tests. T76 cycle 30.
 */
import { detectInlineScriptIssues, type InlineScriptSnapshot } from './inlineScript.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

function snap(over: Partial<InlineScriptSnapshot>): InlineScriptSnapshot {
  return {
    pageUrl: PAGE,
    hasCsp: true,
    cspScriptSrc: '',
    inlineScripts: [],
    eventHandlers: [],
    javascriptUris: [],
    ...over,
  };
}

// T76 cycle 54: hash-pinned inline scripts are clean.
// A script with `sha256: 'X'` AND the CSP script-src directive
// includes `'sha256-X'` should be treated as CSP-covered,
// equivalent to nonce-pinned.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [{ src: 'console.log(1)', hasNonce: false, sha256: 'aaa=' }],
    cspScriptSrc: "'self' 'sha256-aaa='",
  }));
  assert(f.length === 0, 'hash-pinned script clean', JSON.stringify(f));
}

// T76 cycle 54: hash mismatch → still flagged.
// Sha256 present on the script but the CSP directive doesn't
// reference it → cannot credit; raise the warn.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [{ src: 'console.log(1)', hasNonce: false, sha256: 'aaa=' }],
    cspScriptSrc: "'self' 'sha256-different='",
  }));
  assert(
    f.length === 1 && f[0].kind === 'inline-script.present-without-nonce',
    'hash mismatch still flagged',
    JSON.stringify(f),
  );
}

// T76 cycle 54: missing sha256 field → treat as legacy capture.
// Falls back to nonce-only check.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [{ src: 'console.log(1)', hasNonce: false }],
    cspScriptSrc: "'self' 'sha256-aaa='",
  }));
  assert(
    f.length === 1 && f[0].kind === 'inline-script.present-without-nonce',
    'legacy capture without sha256 still flagged',
    JSON.stringify(f),
  );
}

// 1. Empty page → no findings.
{
  const f = detectInlineScriptIssues(snap({}));
  assert(f.length === 0, 'empty page → no findings', JSON.stringify(f));
}

// 2. Inline script without nonce → warn.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [{ src: 'console.log(1)', hasNonce: false }],
  }));
  assert(
    f.length === 1 && f[0].kind === 'inline-script.present-without-nonce',
    'inline script no nonce → warn',
    JSON.stringify(f),
  );
}

// 3. Inline script with nonce → no finding.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [{ src: 'console.log(1)', hasNonce: true }],
  }));
  assert(f.length === 0, 'inline script with nonce clean', JSON.stringify(f));
}

// 4. Multiple inline scripts → aggregated.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [
      { src: 'a()', hasNonce: false },
      { src: 'b()', hasNonce: false },
      { src: 'c()', hasNonce: false },
      { src: 'd()', hasNonce: false },
    ],
  }));
  const fnd = f.find((x) => x.kind === 'inline-script.present-without-nonce');
  assert(fnd !== undefined && (fnd.evidence.count as number) === 4, 'aggregates count=4', JSON.stringify(f));
}

// 5. Examples capped at 5.
{
  const scripts = [];
  for (let i = 0; i < 8; i++) scripts.push({ src: `s${i}()`, hasNonce: false });
  const f = detectInlineScriptIssues(snap({ inlineScripts: scripts }));
  const fnd = f.find((x) => x.kind === 'inline-script.present-without-nonce');
  assert(
    fnd && (fnd.evidence.examples as string[]).length === 5,
    'examples capped at 5',
    JSON.stringify(fnd?.evidence),
  );
  assert(
    fnd && (fnd.evidence.count as number) === 8,
    'count still reflects all 8',
    JSON.stringify(fnd?.evidence),
  );
}

// 6. Event handler attribute → warn.
{
  const f = detectInlineScriptIssues(snap({
    eventHandlers: [{ tag: 'button', attribute: 'onclick', value: 'foo()' }],
  }));
  assert(
    f.length === 1 && f[0].kind === 'inline-script.event-handler-attribute',
    'event handler → warn',
    JSON.stringify(f),
  );
}

// 7. Multiple event handlers aggregated.
{
  const f = detectInlineScriptIssues(snap({
    eventHandlers: [
      { tag: 'button', attribute: 'onclick', value: 'a()' },
      { tag: 'a', attribute: 'onmouseover', value: 'b()' },
      { tag: 'body', attribute: 'onload', value: 'c()' },
    ],
  }));
  const fnd = f.find((x) => x.kind === 'inline-script.event-handler-attribute');
  assert(fnd && (fnd.evidence.count as number) === 3, 'event handlers aggregated', JSON.stringify(f));
}

// 8. javascript: URI → warn.
{
  const f = detectInlineScriptIssues(snap({
    javascriptUris: [{ tag: 'a', uri: 'javascript:void(0)' }],
  }));
  assert(
    f.length === 1 && f[0].kind === 'inline-script.javascript-uri',
    'javascript: URI → warn',
    JSON.stringify(f),
  );
}

// 9. All three present → 3 findings (plus no-csp-but-inline if no CSP).
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [{ src: 'a()', hasNonce: false }],
    eventHandlers: [{ tag: 'button', attribute: 'onclick', value: 'b()' }],
    javascriptUris: [{ tag: 'a', uri: 'javascript:c()' }],
  }));
  // hasCsp default = true in helper, so no composite.
  assert(f.length === 3, 'all three → 3 findings', JSON.stringify(f.map((x) => x.kind)));
  assert(
    f.some((x) => x.kind === 'inline-script.present-without-nonce') &&
      f.some((x) => x.kind === 'inline-script.event-handler-attribute') &&
      f.some((x) => x.kind === 'inline-script.javascript-uri'),
    'all three kinds present',
    JSON.stringify(f.map((x) => x.kind)),
  );
}

// 10. Inline script + no CSP → composite finding.
{
  const f = detectInlineScriptIssues(snap({
    hasCsp: false,
    inlineScripts: [{ src: 'a()', hasNonce: false }],
  }));
  assert(
    f.some((x) => x.kind === 'inline-script.no-csp-but-inline'),
    'no CSP + inline → composite warn',
    JSON.stringify(f),
  );
}

// 11. Event handler + no CSP → composite still fires.
{
  const f = detectInlineScriptIssues(snap({
    hasCsp: false,
    eventHandlers: [{ tag: 'button', attribute: 'onclick', value: 'a()' }],
  }));
  assert(
    f.some((x) => x.kind === 'inline-script.no-csp-but-inline'),
    'no CSP + event handler → composite warn',
    JSON.stringify(f),
  );
}

// 12. No CSP but no inline either → no composite finding.
{
  const f = detectInlineScriptIssues(snap({
    hasCsp: false,
    inlineScripts: [],
    eventHandlers: [],
    javascriptUris: [],
  }));
  assert(f.length === 0, 'no CSP + no inline → no finding', JSON.stringify(f));
}

// 13. Inline script with nonce + CSP → no findings.
{
  const f = detectInlineScriptIssues(snap({
    hasCsp: true,
    inlineScripts: [{ src: 'a()', hasNonce: true }],
  }));
  assert(f.length === 0, 'fully-nonced inline script clean', JSON.stringify(f));
}

// 14. Mixed nonce'd + non-nonce'd inline scripts → only non-nonce flagged.
{
  const f = detectInlineScriptIssues(snap({
    inlineScripts: [
      { src: 'a()', hasNonce: true },
      { src: 'b()', hasNonce: false },
      { src: 'c()', hasNonce: false },
    ],
  }));
  const fnd = f.find((x) => x.kind === 'inline-script.present-without-nonce');
  assert(fnd && (fnd.evidence.count as number) === 2, 'mixed → only non-nonce flagged', JSON.stringify(f));
}

console.log('\n=== inlineScript.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
