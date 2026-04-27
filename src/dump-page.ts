/**
 * dump-page.ts — render a URL in headless Chromium and dump its <main> DOM.
 *
 * Usage:
 *   node --loader ts-node/esm src/dump-page.ts <url> <outfile>
 *
 * Runs React, waits networkidle + 1.5s, writes the innerHTML of the outermost
 * <main> tag to disk. Scope is just the page content (not chrome), since nav
 * and footer are handled by the Rust layout.
 */
import { chromium } from 'playwright';
import { writeFileSync } from 'node:fs';

async function main(): Promise<number> {
  const url = process.argv[2];
  const out = process.argv[3];
  if (!url || !out) {
    console.error('usage: dump-page.ts <url> <outfile>');
    return 2;
  }
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newContext({ viewport: { width: 1440, height: 900 } }).then((c) => c.newPage());
  await page.goto(url, { waitUntil: 'networkidle', timeout: 30_000 });
  await page.waitForTimeout(1500);
  // Capture <main> innerHTML to skip the nav/footer chrome.
  const mainHtml: string = await page.evaluate(() => {
    const el = document.querySelector('main');
    return el ? el.innerHTML : document.body.innerHTML;
  });
  writeFileSync(out, mainHtml);
  console.log(`[dump-page] ${url} → ${out}  (${mainHtml.length} bytes)`);
  await browser.close();
  return 0;
}

main().then((c) => process.exit(c)).catch((e) => {
  console.error(e);
  process.exit(1);
});
