/**
 * docTitle.test.ts — pure-function tests for the document title
 * quality detector (T76).
 */
import {
  detectDocTitleIssues,
  type DocTitleSnapshot,
} from './docTitle.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (over: Partial<DocTitleSnapshot> = {}): DocTitleSnapshot => ({
  pageUrl: 'http://t/',
  present: true,
  raw: 'Acme — Pricing for Enterprise Plans',
  ...over,
});

// 1. Clean — descriptive title, no findings.
{
  const f = detectDocTitleIssues(snap());
  assert(f.length === 0, 'clean title — no findings', JSON.stringify(f));
}

// 2. Missing → strict.
{
  const f = detectDocTitleIssues(snap({ present: false, raw: '' }));
  assert(
    f.length === 1 && f[0].kind === 'title.missing' && f[0].severity === 'strict',
    'missing title fires strict',
    JSON.stringify(f),
  );
}

// 3. Empty → strict (and DOES NOT also fire generic/short — short-
//    circuit). Avoids piling 3 warns on top of 1 strict for the
//    same root cause.
{
  const f = detectDocTitleIssues(snap({ raw: '' }));
  assert(
    f.length === 1 && f[0].kind === 'title.empty' && f[0].severity === 'strict',
    'empty title fires strict and only strict',
    JSON.stringify(f),
  );
}

// 4. Whitespace-only → empty (trimmed).
{
  const f = detectDocTitleIssues(snap({ raw: '   \t\n  ' }));
  assert(
    f.some((x) => x.kind === 'title.empty'),
    'whitespace-only fires empty',
    JSON.stringify(f),
  );
}

// 5. Generic ("Untitled") → warn.
{
  const f = detectDocTitleIssues(snap({ raw: 'Untitled' }));
  assert(
    f.some((x) => x.kind === 'title.generic' && x.severity === 'warn'),
    'Untitled fires generic warn',
    JSON.stringify(f),
  );
}

// 6. Generic case-insensitive ("DOCUMENT").
{
  const f = detectDocTitleIssues(snap({ raw: 'DOCUMENT' }));
  assert(
    f.some((x) => x.kind === 'title.generic'),
    'DOCUMENT (uppercase) caught',
    JSON.stringify(f),
  );
}

// 7. "Document - Acme" is NOT generic (whole-string match).
//    Avoids false positives on legitimate titles that happen to
//    contain a generic word.
{
  const f = detectDocTitleIssues(snap({ raw: 'Document - Acme' }));
  assert(
    !f.some((x) => x.kind === 'title.generic'),
    'composite title containing generic word is not generic',
    JSON.stringify(f),
  );
}

// 8. Too short ("OK").
{
  const f = detectDocTitleIssues(snap({ raw: 'OK' }));
  assert(
    f.some((x) => x.kind === 'title.too-short'),
    '2-char title fires too-short',
    JSON.stringify(f),
  );
}

// 9. Boundary 3-char title — passes too-short.
{
  const f = detectDocTitleIssues(snap({ raw: 'FAQ' }));
  assert(
    !f.some((x) => x.kind === 'title.too-short'),
    '3-char title is acceptable',
    JSON.stringify(f),
  );
}

// 10. Too long (>= 70 chars).
{
  const long = 'A very long page title that goes way past the search-result truncation point of about sixty-five';
  const f = detectDocTitleIssues(snap({ raw: long }));
  assert(
    f.some((x) => x.kind === 'title.too-long'),
    '>= 70-char title fires too-long',
    JSON.stringify(f),
  );
}

// 11. Boundary 69-char title — passes too-long.
{
  const sixtyNine = 'A title that is exactly sixty-nine characters long for boundary'; // 64 chars; bump:
  // Make it exactly 69 chars to test the boundary.
  const exact69 = 'X'.repeat(69);
  const f = detectDocTitleIssues(snap({ raw: exact69 }));
  assert(
    !f.some((x) => x.kind === 'title.too-long'),
    'exact 69-char title passes too-long',
    JSON.stringify(f),
  );
  // Reference the unused var to satisfy strict mode.
  void sixtyNine;
}

// 12. Generic "home" lowercase → warn (suspicious for non-homepages).
{
  const f = detectDocTitleIssues(snap({ raw: 'home' }));
  assert(
    f.some((x) => x.kind === 'title.generic'),
    'lowercase "home" caught',
    JSON.stringify(f),
  );
}

console.log('\n=== docTitle.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
