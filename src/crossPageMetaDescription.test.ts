/**
 * crossPageMetaDescription.test.ts — pure-function tests for the
 * second aggregates-layer detector (T76).
 */
import {
  newCrossPageMetaDescriptionAccumulator,
  recordPageMetaDescription,
  detectCrossPageMetaDescriptionDuplicates,
} from './crossPageMetaDescription.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. Empty accumulator → no findings.
{
  const f = detectCrossPageMetaDescriptionDuplicates(
    newCrossPageMetaDescriptionAccumulator(),
  );
  assert(f.length === 0, 'empty acc — no findings', JSON.stringify(f));
}

// 2. Three pages, all unique descriptions → no findings.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', 'Page A summary');
  recordPageMetaDescription(acc, 'https://t/b', 'Page B summary');
  recordPageMetaDescription(acc, 'https://t/c', 'Page C summary');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(f.length === 0, 'unique descriptions — no findings', JSON.stringify(f));
}

// 3. Two pages share → one warn.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', 'My Site is great');
  recordPageMetaDescription(acc, 'https://t/b', 'My Site is great');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(
    f.length === 1 &&
      f[0].kind === 'meta-description.cross-page-dup' &&
      f[0].severity === 'warn' &&
      (f[0].evidence.urlCount as number) === 2,
    '2-URL dup fires one warn finding',
    JSON.stringify(f),
  );
}

// 4. Multiple dup-groups → multiple findings.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', 'Group A summary');
  recordPageMetaDescription(acc, 'https://t/b', 'Group A summary');
  recordPageMetaDescription(acc, 'https://t/c', 'Group B summary');
  recordPageMetaDescription(acc, 'https://t/d', 'Group B summary');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(f.length === 2, '2 groups → 2 findings', JSON.stringify(f));
}

// 5. Same URL revisit → not a dup.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', 'desc');
  recordPageMetaDescription(acc, 'https://t/a', 'desc');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(f.length === 0, 'same-URL revisit not a dup', JSON.stringify(f));
}

// 6. Empty / whitespace skipped.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', '');
  recordPageMetaDescription(acc, 'https://t/b', '   ');
  recordPageMetaDescription(acc, 'https://t/c', '\t\n');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(f.length === 0, 'empty descriptions skipped', JSON.stringify(f));
}

// 7. Trim normalisation.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', '  desc  ');
  recordPageMetaDescription(acc, 'https://t/b', 'desc');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(
    f.length === 1 && f[0].evidence.description === 'desc',
    'whitespace trims to match',
    JSON.stringify(f),
  );
}

// 8. Long description gets truncated in the detail with ellipsis.
{
  const long =
    'This is a very long meta description that exceeds the eighty character preview window we use in the detail';
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', long);
  recordPageMetaDescription(acc, 'https://t/b', long);
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(
    f[0].detail.includes('…'),
    'long descriptions get ellipsis in detail',
    f[0].detail,
  );
  assert(
    f[0].evidence.description === long,
    'evidence carries full untruncated description',
    JSON.stringify(f[0].evidence),
  );
}

// 9. 4+ URLs share — count = 4.
{
  const acc = newCrossPageMetaDescriptionAccumulator();
  recordPageMetaDescription(acc, 'https://t/a', 'shared');
  recordPageMetaDescription(acc, 'https://t/b', 'shared');
  recordPageMetaDescription(acc, 'https://t/c', 'shared');
  recordPageMetaDescription(acc, 'https://t/d', 'shared');
  const f = detectCrossPageMetaDescriptionDuplicates(acc);
  assert(
    (f[0].evidence.urlCount as number) === 4,
    '4 URLs → count=4',
    JSON.stringify(f[0].evidence),
  );
}

console.log('\n=== crossPageMetaDescription.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
