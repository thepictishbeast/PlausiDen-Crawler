/**
 * metaDescription.test.ts — pure-function tests for the
 * meta-description detector (T76).
 */
import {
  detectMetaDescriptionIssues,
  type MetaDescriptionSnapshot,
} from './metaDescription.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (over: Partial<MetaDescriptionSnapshot> = {}): MetaDescriptionSnapshot => ({
  pageUrl: 'http://t/',
  present: true,
  raw: 'A clear, accurate page summary that fits the search-result preview window.',
  ...over,
});

// 1. Clean — present, ~70 chars, no findings.
{
  const f = detectMetaDescriptionIssues(snap());
  assert(f.length === 0, 'clean description — no findings', JSON.stringify(f));
}

// 2. Missing → warn.
{
  const f = detectMetaDescriptionIssues(snap({ present: false, raw: '' }));
  assert(
    f.length === 1 && f[0].kind === 'meta-description.missing' && f[0].severity === 'warn',
    'missing fires warn (only)',
    JSON.stringify(f),
  );
}

// 3. Empty → warn (and short-circuits — no piling on too-short).
{
  const f = detectMetaDescriptionIssues(snap({ raw: '' }));
  assert(
    f.length === 1 && f[0].kind === 'meta-description.empty',
    'empty fires only empty (short-circuit)',
    JSON.stringify(f),
  );
}

// 4. Whitespace-only counts as empty.
{
  const f = detectMetaDescriptionIssues(snap({ raw: '   \t\n  ' }));
  assert(
    f.some((x) => x.kind === 'meta-description.empty'),
    'whitespace-only fires empty',
    JSON.stringify(f),
  );
}

// 5. Too short (49 chars).
{
  const f = detectMetaDescriptionIssues(snap({ raw: 'X'.repeat(49) }));
  assert(
    f.some((x) => x.kind === 'meta-description.too-short'),
    '49-char fires too-short',
    JSON.stringify(f),
  );
}

// 6. Boundary: exactly 50 chars passes too-short.
{
  const f = detectMetaDescriptionIssues(snap({ raw: 'X'.repeat(50) }));
  assert(
    !f.some((x) => x.kind === 'meta-description.too-short'),
    'exact 50-char passes too-short',
    JSON.stringify(f),
  );
}

// 7. Too long (170 chars).
{
  const f = detectMetaDescriptionIssues(snap({ raw: 'X'.repeat(170) }));
  assert(
    f.some((x) => x.kind === 'meta-description.too-long'),
    '170-char fires too-long',
    JSON.stringify(f),
  );
}

// 8. Boundary: exactly 160 chars passes too-long (160 = boundary,
//    threshold is `> 160`).
{
  const f = detectMetaDescriptionIssues(snap({ raw: 'X'.repeat(160) }));
  assert(
    !f.some((x) => x.kind === 'meta-description.too-long'),
    'exact 160-char passes too-long',
    JSON.stringify(f),
  );
}

// 9. Trim semantics: leading/trailing whitespace doesn't bloat
//    length.
{
  const f = detectMetaDescriptionIssues(
    snap({ raw: '   ' + 'X'.repeat(60) + '   ' }),
  );
  assert(f.length === 0, 'trim removes leading/trailing whitespace', JSON.stringify(f));
}

console.log('\n=== metaDescription.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
