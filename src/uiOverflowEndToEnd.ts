/**
 * uiOverflowEndToEnd.ts — drive Playwright over the ui-overflow
 * fixture server and verify each path produces the expected
 * finding kind. T28 verification.
 *
 * Usage:
 *   npx tsx src/uiOverflowEndToEnd.ts
 *
 * Assumes server running on http://127.0.0.1:8767.
 */
import { chromium } from 'playwright';
import { captureUIOverflowSnapshot, detectUIOverflowIssues } from './uiOverflow.js';

const ROUTES: { path: string; expected: string[] }[] = [
  { path: '/clean/', expected: [] },
  { path: '/page-h-scroll/', expected: ['overflow.page-horizontal-scroll'] },
  { path: '/element-bleed/', expected: ['overflow.element-bleeds-viewport'] },
  { path: '/text-clipped/', expected: ['overflow.text-clipped'] },
  { path: '/small-tap-targets/', expected: ['overflow.tap-target-too-small'] },
  { path: '/combined/', expected: ['overflow.page-horizontal-scroll', 'overflow.element-bleeds-viewport', 'overflow.tap-target-too-small'] },
];

async function main(): Promise<void> {
  // Mobile viewport so tap-target severity is strict in fixture run.
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 375, height: 812 } });
  const page = await ctx.newPage();

  const passes: string[] = [];
  const failures: string[] = [];

  for (const route of ROUTES) {
    const url = `http://127.0.0.1:8767${route.path}`;
    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: 8000 });
    } catch (e) {
      failures.push(`${route.path}: navigation threw ${(e as Error).message}`);
      continue;
    }
    await page.waitForTimeout(200);
    const snap = await captureUIOverflowSnapshot(page);
    const findings = detectUIOverflowIssues(snap);
    const got = findings.map((f) => f.kind);

    if (route.expected.length === 0) {
      if (findings.length === 0) {
        passes.push(`${route.path}: no findings (as expected)`);
      } else {
        failures.push(
          `${route.path}: expected NO findings but got ${JSON.stringify(got)} — snap ${JSON.stringify({ doc: { sw: snap.documentScrollWidth, cw: snap.documentClientWidth }, bleed: snap.bleedingElements.length, clip: snap.textClippedElements.length, tap: snap.smallTapTargets.length })}`,
        );
      }
    } else {
      const all = route.expected.every((e) => got.includes(e));
      if (all) {
        passes.push(`${route.path}: all expected findings present (${JSON.stringify(got)})`);
      } else {
        failures.push(
          `${route.path}: expected all of ${JSON.stringify(route.expected)} but got ${JSON.stringify(got)} — snap ${JSON.stringify({ doc: { sw: snap.documentScrollWidth, cw: snap.documentClientWidth }, bleed: snap.bleedingElements.length, clip: snap.textClippedElements.length, tap: snap.smallTapTargets.length })}`,
        );
      }
    }
  }

  await browser.close();

  console.log('\n=== uiOverflowEndToEnd ===');
  console.log(`PASSED ${passes.length}:`);
  passes.forEach((p) => console.log(`  ✓ ${p}`));
  if (failures.length > 0) {
    console.log(`FAILED ${failures.length}:`);
    failures.forEach((f) => console.log(`  ✗ ${f}`));
    process.exit(1);
  }
  console.log(`All ${passes.length} fixtures matched.`);
}

main().catch((e) => {
  console.error(e);
  process.exit(2);
});
