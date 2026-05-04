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

// 5. Small tap targets on mobile → strict.
{
  const snap = cleanSnap({ width: 375, height: 812 });
  snap.smallTapTargets = [
    { selector: 'body > nav > a', left: 10, top: 10, width: 24, height: 24, right: 34, text: 'X' },
  ];
  const findings = detectUIOverflowIssues(snap);
  const tap = findings.find((f) => f.kind === 'overflow.tap-target-too-small');
  assert(!!tap, 'tap-target-too-small triggers on mobile', 'no finding emitted');
  assert(tap?.severity === 'strict', 'tap-target severity is strict on mobile viewport', `got ${tap?.severity}`);
}

// 6. Small tap targets on desktop → warn.
{
  const snap = cleanSnap({ width: 1280, height: 800 });
  snap.smallTapTargets = [
    { selector: 'body > nav > button.icon', left: 10, top: 10, width: 28, height: 28, right: 38, text: '' },
  ];
  const findings = detectUIOverflowIssues(snap);
  const tap = findings.find((f) => f.kind === 'overflow.tap-target-too-small');
  assert(!!tap, 'tap-target-too-small triggers on desktop', 'no finding emitted');
  assert(tap?.severity === 'warn', 'tap-target severity is warn on desktop viewport', `got ${tap?.severity}`);
}

// 7. Combined breakage produces multiple findings.
{
  const snap = cleanSnap();
  snap.documentScrollWidth = 500;
  snap.pageHasHorizontalScroll = true;
  snap.bleedingElements = [{ selector: 'body > img', left: 0, top: 100, width: 500, height: 200, right: 500, text: '' }];
  snap.smallTapTargets = [{ selector: 'body > a', left: 0, top: 0, width: 20, height: 20, right: 20, text: 'x' }];
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
