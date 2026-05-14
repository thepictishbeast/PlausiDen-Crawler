/**
 * skipLink.test.ts — pure-function tests for the skip-link detector
 * (T76).
 */
import {
  detectSkipLinkIssues,
  type SkipLinkSnapshot,
} from './skipLink.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (over: Partial<SkipLinkSnapshot> = {}): SkipLinkSnapshot => ({
  pageUrl: 'http://t/',
  found: true,
  href: '#main',
  text: 'Skip to main content',
  targetExists: true,
  firstFocusable: true,
  permanentlyHidden: false,
  ...over,
});

// 1. Clean — proper skip link, first focusable, target exists, not hidden.
{
  const f = detectSkipLinkIssues(snap());
  assert(f.length === 0, 'clean skip link — no findings', JSON.stringify(f));
}

// 2. Missing skip link → warn (not strict — landmark nav partially compensates).
{
  const f = detectSkipLinkIssues(snap({ found: false, href: '', text: '', targetExists: false, firstFocusable: false, permanentlyHidden: false }));
  assert(
    f.length === 1 && f[0].kind === 'skip.missing' && f[0].severity === 'warn',
    'missing fires warn (not strict)',
    JSON.stringify(f),
  );
}

// 3. Broken target → strict.
{
  const f = detectSkipLinkIssues(snap({ targetExists: false }));
  assert(
    f.some((x) => x.kind === 'skip.broken-target' && x.severity === 'strict'),
    'broken target fires strict',
    JSON.stringify(f),
  );
}

// 4. Not first focusable → warn.
{
  const f = detectSkipLinkIssues(snap({ firstFocusable: false }));
  assert(
    f.some((x) => x.kind === 'skip.not-first-focusable' && x.severity === 'warn'),
    'not-first-focusable fires warn',
    JSON.stringify(f),
  );
}

// 5. Permanently hidden → strict.
{
  const f = detectSkipLinkIssues(snap({ permanentlyHidden: true }));
  assert(
    f.some((x) => x.kind === 'skip.permanently-hidden' && x.severity === 'strict'),
    'permanently-hidden fires strict',
    JSON.stringify(f),
  );
}

// 6. Multiple defects: broken target + not first focusable.
{
  const f = detectSkipLinkIssues(
    snap({ targetExists: false, firstFocusable: false }),
  );
  assert(
    f.some((x) => x.kind === 'skip.broken-target') &&
      f.some((x) => x.kind === 'skip.not-first-focusable'),
    'multiple defects emit multiple findings',
    JSON.stringify(f),
  );
}

// 7. Missing skip link short-circuits (no broken-target / not-first
//    findings emitted alongside the missing finding — would be
//    nonsense).
{
  const f = detectSkipLinkIssues(snap({ found: false, href: '', text: '', targetExists: false, firstFocusable: false, permanentlyHidden: false }));
  assert(
    f.length === 1,
    'missing short-circuits — only 1 finding',
    JSON.stringify(f),
  );
}

// 8. Permanently-hidden + broken target → both fire (independent
//    failure modes).
{
  const f = detectSkipLinkIssues(
    snap({ permanentlyHidden: true, targetExists: false }),
  );
  assert(
    f.some((x) => x.kind === 'skip.permanently-hidden') &&
      f.some((x) => x.kind === 'skip.broken-target'),
    'hidden + broken both fire',
    JSON.stringify(f),
  );
}

console.log('\n=== skipLink.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
