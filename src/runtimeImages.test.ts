/**
 * runtimeImages.test.ts — pure-function tests for detectRuntimeImageIssues.
 */
import { detectRuntimeImageIssues, type RuntimeImagesSnapshot, type ImageOffender } from './runtimeImages.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): RuntimeImagesSnapshot => ({
  pageUrl: 'http://t/',
  viewport: { width: 1280, height: 800 },
  totalImages: 1,
  broken: [],
  emptySrc: [],
  missingAlt: [],
  clsRisk: [],
});

const offender = (overrides: Partial<ImageOffender> = {}): ImageOffender => ({
  selector: 'body > img',
  src: '/x.png',
  alt: 'x',
  naturalWidth: 100,
  naturalHeight: 100,
  complete: true,
  width: 100,
  height: 100,
  hasExplicitDims: true,
  hasAspectRatio: false,
  isVisible: true,
  isDecorative: false,
  ...overrides,
});

// 1. Clean snapshot → no findings.
{
  const f = detectRuntimeImageIssues(baseSnap());
  assert(f.length === 0, 'clean snapshot — no findings', `got ${JSON.stringify(f)}`);
}

// 2. Broken image (404) → strict.
{
  const s = baseSnap();
  s.broken.push(offender({ src: '/404.png', naturalWidth: 0, complete: true }));
  const f = detectRuntimeImageIssues(s);
  assert(f.some((x) => x.kind === 'images.broken' && x.severity === 'strict'), 'broken fires strict', JSON.stringify(f));
}

// 3. Empty src → strict.
{
  const s = baseSnap();
  s.emptySrc.push(offender({ src: '' }));
  const f = detectRuntimeImageIssues(s);
  assert(f.some((x) => x.kind === 'images.empty-src' && x.severity === 'strict'), 'empty-src fires strict', JSON.stringify(f));
}

// 4. Missing alt attribute → strict.
{
  const s = baseSnap();
  s.missingAlt.push(offender({ alt: null }));
  const f = detectRuntimeImageIssues(s);
  assert(f.some((x) => x.kind === 'images.missing-alt-attr' && x.severity === 'strict'), 'missing-alt fires strict', JSON.stringify(f));
}

// 5. CLS risk → warn.
{
  const s = baseSnap();
  s.clsRisk.push(offender({ hasExplicitDims: false, hasAspectRatio: false }));
  const f = detectRuntimeImageIssues(s);
  assert(f.some((x) => x.kind === 'images.cls-risk' && x.severity === 'warn'), 'cls-risk fires warn', JSON.stringify(f));
}

// 6. Mixed snapshot → multiple findings.
{
  const s = baseSnap();
  s.broken.push(offender({ src: '/404.png', naturalWidth: 0 }));
  s.missingAlt.push(offender({ alt: null }));
  s.clsRisk.push(offender({ hasExplicitDims: false, hasAspectRatio: false }));
  const f = detectRuntimeImageIssues(s);
  assert(f.length === 3, 'mixed snapshot emits 3 findings', `got ${f.length}: ${JSON.stringify(f.map((x) => x.kind))}`);
}

console.log('\n=== runtimeImages.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
