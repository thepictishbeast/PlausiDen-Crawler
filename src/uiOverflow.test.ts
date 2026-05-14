/**
 * uiOverflow.test.ts — pure-function tests for detectUIOverflowIssues.
 *
 * Run with: npx tsx src/uiOverflow.test.ts
 */
import { detectUIOverflowIssues, type UIOverflowSnapshot } from './uiOverflow.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];

function assert(cond: boolean, name: string, reason: string): void {
  if (cond) PASSED.push(name);
  else FAILED.push({ name, reason });
}

const cleanSnap = (vp = { width: 375, height: 812 }): UIOverflowSnapshot => ({
  pageUrl: 'http://t/clean',
  viewport: vp,
  documentScrollWidth: vp.width,
  documentClientWidth: vp.width,
  pageHasHorizontalScroll: false,
  bleedingElements: [],
  textClippedElements: [],
  smallTapTargets: [],
});

// 1. Clean page → no findings.
{
  const findings = detectUIOverflowIssues(cleanSnap());
  assert(findings.length === 0, 'clean snapshot produces no findings', `got ${JSON.stringify(findings)}`);
}

// 2. Page-level horizontal scroll.
{
  const snap = cleanSnap();
  snap.documentScrollWidth = 500;
  snap.pageHasHorizontalScroll = true;
  snap.bleedingElements = [
    { selector: 'body > div > img', left: 0, top: 100, width: 500, height: 200, right: 500, text: '' },
  ];
  const findings = detectUIOverflowIssues(snap);
  assert(
    findings.some((f) => f.kind === 'overflow.page-horizontal-scroll'),
    'page-horizontal-scroll triggers',
    `got ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// 3. Element bleeds viewport.
{
  const snap = cleanSnap();
  snap.bleedingElements = [
    { selector: 'body > div > p', left: 0, top: 100, width: 600, height: 20, right: 600, text: 'long unbroken url...' },
  ];
  const findings = detectUIOverflowIssues(snap);
  assert(
    findings.some((f) => f.kind === 'overflow.element-bleeds-viewport'),
    'element-bleeds-viewport triggers',
    `got ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// 4. Text clipped without scroll affordance.
{
  const snap = cleanSnap();
  snap.textClippedElements = [
    { selector: 'body > div > pre', left: 0, top: 100, width: 300, height: 50, right: 300, text: 'monospace dump...' },
  ];
  const findings = detectUIOverflowIssues(snap);
  assert(
    findings.some((f) => f.kind === 'overflow.text-clipped'),
    'text-clipped triggers',
    `got ${JSON.stringify(findings.map((f) => f.kind))}`,
  );
}

// T76 2026-05-14: tap-target tests removed — that detection moved
// to src/tapTargets.ts (WCAG 2.5.8 + 2.5.5, two severity tiers,
// inline-in-sentence exception). See tapTargets.test.ts for
// canonical tap-target test coverage.

// 5. Combined breakage (overflow + bleed + clip) produces multiple
//    findings. (Was test #7; renumbered after tap-target tests
//    were removed.)
{
  const snap = cleanSnap();
  snap.documentScrollWidth = 500;
  snap.pageHasHorizontalScroll = true;
  snap.bleedingElements = [{ selector: 'body > img', left: 0, top: 100, width: 500, height: 200, right: 500, text: '' }];
  snap.textClippedElements = [{ selector: 'body > p', left: 0, top: 200, width: 200, height: 30, right: 200, text: 'truncated' }];
  const findings = detectUIOverflowIssues(snap);
  assert(findings.length >= 3, 'combined snapshot produces 3+ findings', `got ${findings.length}`);
}

console.log(`\n=== uiOverflow.test.ts ===`);
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}\n    ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
