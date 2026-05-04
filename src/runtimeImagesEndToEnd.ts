/**
 * runtimeImagesEndToEnd.ts — drive Playwright across the
 * fixtures/images/ server and assert each path produces the
 * expected finding kind.
 */
import { chromium } from 'playwright';
import { captureRuntimeImagesSnapshot, detectRuntimeImageIssues } from './runtimeImages.js';

const ROUTES: { path: string; expected: string[] }[] = [
  { path: '/clean/', expected: [] },
  { path: '/broken/', expected: ['images.broken'] },
  { path: '/empty-src/', expected: ['images.empty-src'] },
  { path: '/missing-alt/', expected: ['images.missing-alt-attr'] },
  { path: '/cls-risk/', expected: ['images.cls-risk'] },
  { path: '/combined/', expected: ['images.broken', 'images.missing-alt-attr'] },
];

async function main(): Promise<void> {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();

  const passes: string[] = [];
  const failures: string[] = [];

  for (const route of ROUTES) {
    const url = `http://127.0.0.1:8768${route.path}`;
    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: 8000 });
    } catch (e) {
      // 404 is expected for the broken-img sub-resource; navigation should still complete.
    }
    await page.waitForTimeout(400);
    const snap = await captureRuntimeImagesSnapshot(page);
    const findings = detectRuntimeImageIssues(snap);
    const got = findings.map((f) => f.kind);

    if (route.expected.length === 0) {
      if (findings.length === 0) passes.push(`${route.path}: no findings (expected)`);
      else failures.push(`${route.path}: expected NONE, got ${JSON.stringify(got)} — snap totalImages=${snap.totalImages} broken=${snap.broken.length} missingAlt=${snap.missingAlt.length} cls=${snap.clsRisk.length}`);
    } else {
      const all = route.expected.every((e) => got.includes(e));
      if (all) passes.push(`${route.path}: matched ${JSON.stringify(got)}`);
      else failures.push(`${route.path}: expected all of ${JSON.stringify(route.expected)}, got ${JSON.stringify(got)}`);
    }
  }

  await browser.close();

  console.log('\n=== runtimeImagesEndToEnd ===');
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
