/**
 * favicon.test.ts — pure-function tests for the favicon detector
 * (T76).
 */
import { detectFaviconIssues, type FaviconSnapshot } from './favicon.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (over: Partial<FaviconSnapshot> = {}): FaviconSnapshot => ({
  pageUrl: 'http://t/',
  iconLinkCount: 1,
  relValues: ['icon'],
  ...over,
});

// 1. Clean — page has at least one icon link.
{
  const f = detectFaviconIssues(snap());
  assert(f.length === 0, 'icon link present — no findings', JSON.stringify(f));
}

// 2. Missing — zero icon links of any rel kind.
{
  const f = detectFaviconIssues(snap({ iconLinkCount: 0, relValues: [] }));
  assert(
    f.length === 1 && f[0].kind === 'favicon.missing-link' && f[0].severity === 'warn',
    'no icon link fires warn',
    JSON.stringify(f),
  );
}

// 3. Multiple icon links — still clean.
{
  const f = detectFaviconIssues(snap({ iconLinkCount: 3, relValues: ['icon', 'apple-touch-icon', 'mask-icon'] }));
  assert(f.length === 0, 'multiple icon links — clean', JSON.stringify(f));
}

// 4. Apple-touch-icon only — clean (counts as an icon link).
{
  const f = detectFaviconIssues(snap({ iconLinkCount: 1, relValues: ['apple-touch-icon'] }));
  assert(f.length === 0, 'apple-touch-icon only — clean', JSON.stringify(f));
}

console.log('\n=== favicon.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
