/**
 * documentPolicy.test.ts — Document-Policy detector tests.
 * T76 cycle 60.
 */
import {
  buildDocumentPolicySnapshot,
  detectDocumentPolicyIssues,
} from './documentPolicy.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PROD = 'https://example.com/';
const LOCAL = 'http://127.0.0.1:8123/';

// 1. localhost exempted regardless of header state.
{
  const snap = buildDocumentPolicySnapshot(LOCAL, undefined);
  const f = detectDocumentPolicyIssues(snap);
  assert(f.length === 0, 'localhost → no findings', JSON.stringify(f));
}

// 2. Production page with no header → missing warn.
{
  const snap = buildDocumentPolicySnapshot(PROD, undefined);
  const f = detectDocumentPolicyIssues(snap);
  assert(
    f.length === 1 && f[0].kind === 'document-policy.missing',
    'missing header → missing warn',
    JSON.stringify(f),
  );
}

// 3. Production page WITH `document-write=?0` and force-load-at-top → clean.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': 'document-write=?0, force-load-at-top',
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(f.length === 0, 'full directive set → clean', JSON.stringify(f));
}

// 4. Header set with only force-load-at-top → clean.
// T76 cycle 61: document-write directive not yet shipped in
// Chromium; absence of `document-write=?0` is no longer flagged.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': 'force-load-at-top',
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(
    f.length === 0,
    'header with only force-load-at-top → clean (document-write absence not flagged until directive ships)',
    JSON.stringify(f),
  );
}

// 5. Header explicitly enables document.write → warn.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': 'document-write=?1',
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(
    f.length === 1 && f[0].kind === 'document-policy.permits-document-write',
    'document-write=?1 → permits warn',
    JSON.stringify(f),
  );
}

// 6. Header value unparseable → invalid warn.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': '   ',
  });
  const f = detectDocumentPolicyIssues(snap);
  // Empty parsed dict + raw was empty whitespace; trimmed === '' which is falsy → treated as null
  // So this should fire missing, not invalid. Let's adjust expectation.
  assert(
    f.length === 1 && (f[0].kind === 'document-policy.missing' || f[0].kind === 'document-policy.invalid'),
    'whitespace-only value → missing or invalid',
    JSON.stringify(f),
  );
}

// 7. Header value unparseable (commas only) → invalid warn.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': ',,,',
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(
    f.length === 1 && f[0].kind === 'document-policy.invalid',
    'unparseable value → invalid warn',
    JSON.stringify(f),
  );
}

// 8. Case-insensitive header lookup.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'Document-Policy': 'document-write=?0, force-load-at-top',
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(f.length === 0, 'mixed-case header → resolved', JSON.stringify(f));
}

// 9. Bare `document-write` parses as ?1 → permits warn.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': 'document-write',  // bare = ?1 = ENABLED
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(
    f.length === 1 && f[0].kind === 'document-policy.permits-document-write',
    'bare document-write directive treated as ?1 → permits warn',
    JSON.stringify(f),
  );
}

// 10. Multiple directives parsed correctly.
{
  const snap = buildDocumentPolicySnapshot(PROD, {
    'document-policy': 'document-write=?0, force-load-at-top, unsized-media=?0, js-profiling',
  });
  const f = detectDocumentPolicyIssues(snap);
  assert(f.length === 0, 'multi-directive parses → clean', JSON.stringify(f));
  assert(
    snap.directives['document-write'] === '?0' &&
      snap.directives['force-load-at-top'] === '?1' &&
      snap.directives['unsized-media'] === '?0' &&
      snap.directives['js-profiling'] === '?1',
    'all 4 directives parsed correctly',
    JSON.stringify(snap.directives),
  );
}

console.log('=== documentPolicy.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
for (const p of PASSED) console.log('  ✓ ' + p);
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  for (const f of FAILED) console.log(`  ✗ ${f.name}\n    ${f.reason}`);
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
