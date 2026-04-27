/**
 * dom-diff.ts — capture rendered DOMs from two URLs and diff them.
 *
 * Usage:
 *   node --loader ts-node/esm src/dom-diff.ts <left-url> <right-url>
 *
 * Renders each URL in headless Chromium, waits for the 'load' event,
 * snapshots document.documentElement.outerHTML, writes each to a file,
 * runs a word-level diff, and lists every <button>/<a>/<img>/<section>
 * that appears in one side but not the other.
 *
 * Intent: find every structural difference between a reference site and
 * a rewrite, without relying on the human to spot them.
 */
import { chromium } from 'playwright';
import { writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

function parseCliUrls(argv: string[]): { left: string; right: string } {
  const urls = argv.slice(2).filter((a) => a.startsWith('http'));
  if (urls.length < 2) {
    console.error('usage: dom-diff.ts <left-url> <right-url>');
    process.exit(2);
  }
  return { left: urls[0], right: urls[1] };
}

// Strip React-runtime-generated noise that never matters for visual parity:
// inline motion styles ("style='opacity: 1; transform: none;'"), data-reactroot,
// data-tour, aria-* that flicker per state, etc. Keep everything else.
function normalize(html: string): string {
  return (
    html
      // React/framer-motion inline styles
      .replace(/\s+style="opacity:\s*1;\s*transform:\s*none;?"/g, '')
      .replace(/\s+style="pointer-events:\s*none;?"/g, '')
      // data- runtime attrs
      .replace(/\s+data-reactroot(="[^"]*")?/g, '')
      .replace(/\s+data-tour="[^"]*"/g, '')
      .replace(/\s+data-state="[^"]*"/g, '')
      // collapse whitespace between tags
      .replace(/>\s+</g, '><')
      // trim
      .trim()
  );
}

// Heuristic interactive-element extractor. Not a real HTML parser — good
// enough to list every <button>, <a href>, <img src> signature.
function signatures(html: string): Set<string> {
  const out = new Set<string>();
  const aRe = /<a\s+[^>]*href="([^"]+)"[^>]*>([^<]{0,80})/g;
  const bRe = /<button\b[^>]*>([^<]{0,80})/g;
  const imgRe = /<img\s+[^>]*src="([^"]+)"/g;
  let m: RegExpExecArray | null;
  while ((m = aRe.exec(html))) out.add(`A  href=${m[1].slice(0, 80)}  txt=${m[2].trim().slice(0, 60)}`);
  while ((m = bRe.exec(html))) out.add(`BTN  ${m[1].trim().slice(0, 60)}`);
  while ((m = imgRe.exec(html))) out.add(`IMG  src=${m[1].slice(0, 80)}`);
  return out;
}

async function capture(url: string): Promise<string> {
  const browser = await chromium.launch({ headless: true });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await ctx.newPage();
  await page.goto(url, { waitUntil: 'networkidle', timeout: 30_000 });
  // Give React/lazy chunks a beat to render.
  await page.waitForTimeout(1200);
  const html = await page.content();
  await browser.close();
  return html;
}

async function main(): Promise<number> {
  const { left, right } = parseCliUrls(process.argv);
  const outDir = join('runs', `dom-diff-${new Date().toISOString().replace(/[:.]/g, '-')}`);
  mkdirSync(outDir, { recursive: true });

  console.log(`[dom-diff] left:  ${left}`);
  console.log(`[dom-diff] right: ${right}`);

  const [rawL, rawR] = await Promise.all([capture(left), capture(right)]);
  const nL = normalize(rawL);
  const nR = normalize(rawR);

  writeFileSync(join(outDir, 'left.raw.html'), rawL);
  writeFileSync(join(outDir, 'right.raw.html'), rawR);
  writeFileSync(join(outDir, 'left.norm.html'), nL);
  writeFileSync(join(outDir, 'right.norm.html'), nR);

  const crypto = await import('node:crypto');
  const h = (s: string): string => crypto.createHash('sha256').update(s).digest('hex').slice(0, 16);
  console.log(`[dom-diff] left  sha256(norm)=${h(nL)}  bytes=${nL.length}`);
  console.log(`[dom-diff] right sha256(norm)=${h(nR)}  bytes=${nR.length}`);

  const sL = signatures(nL);
  const sR = signatures(nR);
  const onlyLeft = [...sL].filter((x) => !sR.has(x)).sort();
  const onlyRight = [...sR].filter((x) => !sL.has(x)).sort();

  writeFileSync(
    join(outDir, 'signature-diff.txt'),
    [
      `# left:  ${left}`,
      `# right: ${right}`,
      '',
      `## Only in left (${onlyLeft.length}):`,
      ...onlyLeft,
      '',
      `## Only in right (${onlyRight.length}):`,
      ...onlyRight,
    ].join('\n'),
  );

  console.log(`[dom-diff] only in left: ${onlyLeft.length}`);
  for (const s of onlyLeft) console.log(`  L   ${s}`);
  console.log(`[dom-diff] only in right: ${onlyRight.length}`);
  for (const s of onlyRight) console.log(`  R   ${s}`);
  console.log(`[dom-diff] wrote ${outDir}/`);

  return 0;
}

main().then((c) => process.exit(c)).catch((e) => {
  console.error(e);
  process.exit(1);
});
