/**
 * runtimeContrast.test.ts — pure-function tests for
 * detectRuntimeContrastIssues. Hand-crafted snapshots; no browser.
 */
import { detectRuntimeContrastIssues, type RuntimeContrastSnapshot } from './runtimeContrast.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) => (c ? PASSED.push(name) : FAILED.push({ name, reason }));

const baseSnap = (): RuntimeContrastSnapshot => ({
  pageUrl: 'http://t/',
  viewport: { width: 1280, height: 800 },
  textNodesScanned: 50,
  totalContrastPairs: 50,
  failingOffenders: [],
});

// 1. Clean snapshot → no findings.
{
  const f = detectRuntimeContrastIssues(baseSnap());
  assert(f.length === 0, 'clean snapshot — no findings', `got ${JSON.stringify(f)}`);
}

// 2. One body-text offender → strict.
{
  const s = baseSnap();
  s.failingOffenders.push({ selector: 'body > p', fg: 'rgb(120,120,120)', bg: 'rgb(255,255,255)', ratio: 3.2, required: 4.5, fontSizePx: 14, isLarge: false, text: 'low contrast text' });
  const f = detectRuntimeContrastIssues(s);
  assert(f.some((x) => x.kind === 'contrast.body-text-below-aa' && x.severity === 'strict'), 'body-text-below-aa fires strict', JSON.stringify(f));
}

// 3. One large-text offender → warn.
{
  const s = baseSnap();
  s.failingOffenders.push({ selector: 'body > h1', fg: 'rgb(180,180,180)', bg: 'rgb(255,255,255)', ratio: 2.5, required: 3, fontSizePx: 32, isLarge: true, text: 'big light text' });
  const f = detectRuntimeContrastIssues(s);
  assert(f.some((x) => x.kind === 'contrast.large-text-below-aa' && x.severity === 'warn'), 'large-text-below-aa fires warn', JSON.stringify(f));
}

// 4. Mixed — both findings emitted.
{
  const s = baseSnap();
  s.failingOffenders.push({ selector: 'body > p', fg: 'rgb(120,120,120)', bg: 'rgb(255,255,255)', ratio: 3.2, required: 4.5, fontSizePx: 14, isLarge: false, text: 'a' });
  s.failingOffenders.push({ selector: 'body > h2', fg: 'rgb(180,180,180)', bg: 'rgb(255,255,255)', ratio: 2.5, required: 3, fontSizePx: 28, isLarge: true, text: 'b' });
  const f = detectRuntimeContrastIssues(s);
  assert(f.length === 2, 'mixed snapshot emits both findings', JSON.stringify(f.map((x) => x.kind)));
}

console.log('\n=== runtimeContrast.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
