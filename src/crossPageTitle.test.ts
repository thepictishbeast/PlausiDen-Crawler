/**
 * crossPageTitle.test.ts — pure-function tests for the
 * aggregates-layer cross-page title duplicate detector (T76).
 */
import {
  newCrossPageTitleAccumulator,
  recordPageTitle,
  detectCrossPageTitleDuplicates,
} from './crossPageTitle.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

// 1. Empty accumulator → no findings.
{
  const f = detectCrossPageTitleDuplicates(newCrossPageTitleAccumulator());
  assert(f.length === 0, 'empty acc — no findings', JSON.stringify(f));
}

// 2. Three pages, all unique titles → no findings.
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', 'Home');
  recordPageTitle(acc, 'https://t/b', 'About');
  recordPageTitle(acc, 'https://t/c', 'Contact');
  const f = detectCrossPageTitleDuplicates(acc);
  assert(f.length === 0, 'unique titles — no findings', JSON.stringify(f));
}

// 3. Two pages share a title → one warn finding.
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', 'My Site');
  recordPageTitle(acc, 'https://t/b', 'My Site');
  const f = detectCrossPageTitleDuplicates(acc);
  assert(
    f.length === 1 &&
      f[0].kind === 'title.cross-page-dup' &&
      f[0].severity === 'warn' &&
      (f[0].evidence.urlCount as number) === 2,
    '2-URL dup fires one warn finding',
    JSON.stringify(f),
  );
}

// 4. Two different dup-groups → two findings (independent).
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', 'Group A');
  recordPageTitle(acc, 'https://t/b', 'Group A');
  recordPageTitle(acc, 'https://t/c', 'Group B');
  recordPageTitle(acc, 'https://t/d', 'Group B');
  recordPageTitle(acc, 'https://t/e', 'Singleton');
  const f = detectCrossPageTitleDuplicates(acc);
  assert(f.length === 2, '2 dup-groups → 2 findings', JSON.stringify(f));
  // Stable iteration: Group A comes first in input order.
  assert(
    f[0].evidence.title === 'Group A',
    'Group A is first finding',
    JSON.stringify(f[0]),
  );
}

// 5. Same title on the SAME url (re-visit) → not a dup.
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', 'My Site');
  recordPageTitle(acc, 'https://t/a', 'My Site'); // same URL revisited
  const f = detectCrossPageTitleDuplicates(acc);
  assert(
    f.length === 0,
    'same-URL revisit not a dup',
    JSON.stringify(f),
  );
}

// 6. Empty/whitespace title is skipped (covered by per-page
//    title.empty finding).
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', '');
  recordPageTitle(acc, 'https://t/b', '   ');
  recordPageTitle(acc, 'https://t/c', '\t\n');
  const f = detectCrossPageTitleDuplicates(acc);
  assert(f.length === 0, 'empty titles skipped', JSON.stringify(f));
}

// 7. Trim semantics: leading/trailing whitespace normalises.
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', '  My Site  ');
  recordPageTitle(acc, 'https://t/b', 'My Site');
  const f = detectCrossPageTitleDuplicates(acc);
  assert(
    f.length === 1 && f[0].evidence.title === 'My Site',
    'trim() normalisation catches whitespace-only difference',
    JSON.stringify(f),
  );
}

// 8. Three+ URLs share — one finding with urlCount = 3.
{
  const acc = newCrossPageTitleAccumulator();
  recordPageTitle(acc, 'https://t/a', 'Dup');
  recordPageTitle(acc, 'https://t/b', 'Dup');
  recordPageTitle(acc, 'https://t/c', 'Dup');
  const f = detectCrossPageTitleDuplicates(acc);
  assert(
    f.length === 1 && (f[0].evidence.urlCount as number) === 3,
    '3 URLs → 1 finding count=3',
    JSON.stringify(f[0].evidence),
  );
}

console.log('\n=== crossPageTitle.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
