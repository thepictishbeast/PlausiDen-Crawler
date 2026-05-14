/**
 * fontLoading.test.ts — pure-function tests for the @font-face
 * font-display detector (T76).
 */
import {
  detectFontLoadingIssues,
  type FontLoadingSnapshot,
  type CapturedFontFace,
} from './fontLoading.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (
  faces: CapturedFontFace[] = [],
): FontLoadingSnapshot => ({
  pageUrl: 'https://example.com/',
  inaccessibleSheetCount: 0,
  faces,
});

const face = (over: Partial<CapturedFontFace> = {}): CapturedFontFace => ({
  family: 'Inter',
  fontDisplay: 'swap',
  sheetHref: 'https://example.com/styles.css',
  ...over,
});

// 1. No faces at all → no findings.
{
  const f = detectFontLoadingIssues(baseSnap());
  assert(f.length === 0, 'no faces → no findings', JSON.stringify(f));
}

// 2. Face with font-display: swap → clean.
{
  const f = detectFontLoadingIssues(baseSnap([face()]));
  assert(f.length === 0, 'swap is clean', JSON.stringify(f));
}

// 3. Face with font-display: fallback → clean.
{
  const f = detectFontLoadingIssues(baseSnap([face({ fontDisplay: 'fallback' })]));
  assert(f.length === 0, 'fallback is clean', JSON.stringify(f));
}

// 4. Face with font-display: optional → clean.
{
  const f = detectFontLoadingIssues(baseSnap([face({ fontDisplay: 'optional' })]));
  assert(f.length === 0, 'optional is clean', JSON.stringify(f));
}

// 5. Face with NO font-display → warn no-display.
{
  const f = detectFontLoadingIssues(baseSnap([face({ fontDisplay: '' })]));
  assert(
    f.some((x) => x.kind === 'font-loading.no-display' && x.severity === 'warn'),
    'no font-display → warn',
    JSON.stringify(f),
  );
}

// 6. Face with font-display: block → warn display-block.
{
  const f = detectFontLoadingIssues(baseSnap([face({ fontDisplay: 'block' })]));
  assert(
    f.some((x) => x.kind === 'font-loading.display-block' && x.severity === 'warn'),
    'block → warn display-block',
    JSON.stringify(f),
  );
}

// 7. Face with font-display: auto → warn display-block (auto resolves to block).
{
  const f = detectFontLoadingIssues(baseSnap([face({ fontDisplay: 'auto' })]));
  assert(
    f.some((x) => x.kind === 'font-loading.display-block'),
    'auto → display-block warn',
    JSON.stringify(f),
  );
}

// 8. Unknown value treated as no-display.
{
  const f = detectFontLoadingIssues(baseSnap([face({ fontDisplay: 'gibberish' })]));
  assert(
    f.some((x) => x.kind === 'font-loading.no-display'),
    'unknown value → no-display warn',
    JSON.stringify(f),
  );
}

// 9. Multiple no-display faces aggregate to one finding.
{
  const f = detectFontLoadingIssues(baseSnap([
    face({ family: 'Inter', fontDisplay: '' }),
    face({ family: 'Mono', fontDisplay: '' }),
    face({ family: 'Display', fontDisplay: '' }),
  ]));
  const noDisp = f.find((x) => x.kind === 'font-loading.no-display');
  assert(
    !!noDisp && (noDisp.evidence.count as number) === 3,
    '3 no-display faces → 1 finding count=3',
    JSON.stringify(noDisp),
  );
}

// 10. Mixed: 2 no-display + 1 block → 2 findings (independent).
{
  const f = detectFontLoadingIssues(baseSnap([
    face({ fontDisplay: '' }),
    face({ fontDisplay: '' }),
    face({ fontDisplay: 'block' }),
  ]));
  assert(f.length === 2, 'mixed → 2 findings', JSON.stringify(f.map((x) => x.kind)));
}

// 11. Examples capped at 5.
{
  const faces = [];
  for (let i = 0; i < 8; i++) {
    faces.push(face({ family: `Font${i}`, fontDisplay: '' }));
  }
  const f = detectFontLoadingIssues(baseSnap(faces));
  const noDisp = f[0];
  const examples = noDisp.evidence.examples as string[];
  assert(examples.length === 5, 'examples capped at 5', JSON.stringify(examples));
  assert(
    (noDisp.evidence.count as number) === 8,
    'count still 8',
    JSON.stringify(noDisp.evidence),
  );
}

// 12. Mixed clean + bad: only the bad ones contribute to findings.
{
  const f = detectFontLoadingIssues(baseSnap([
    face({ family: 'Inter', fontDisplay: 'swap' }),     // clean
    face({ family: 'Mono', fontDisplay: '' }),          // bad
    face({ family: 'Display', fontDisplay: 'optional' }), // clean
  ]));
  assert(
    f.length === 1 && (f[0].evidence.count as number) === 1,
    '1 of 3 bad → 1 finding count=1',
    JSON.stringify(f),
  );
}

// 13. Inaccessible sheet count carried through evidence (so audit
//     reader knows to manually check the cross-origin font sources).
{
  const snap = baseSnap([face({ fontDisplay: '' })]);
  snap.inaccessibleSheetCount = 3;
  const f = detectFontLoadingIssues(snap);
  assert(
    (f[0].evidence.inaccessibleSheetCount as number) === 3,
    'inaccessible count surfaces in evidence',
    JSON.stringify(f[0].evidence),
  );
}

console.log('\n=== fontLoading.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
