/**
 * viewportMeta.test.ts — pure-function tests for the viewport-meta
 * detector (T76). The page.evaluate() snapshot capture is exercised
 * by the e2e fixtures.
 */
import {
  detectViewportMetaIssues,
  parseViewportContent,
  type ViewportMetaSnapshot,
} from './viewportMeta.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (over: Partial<ViewportMetaSnapshot> = {}): ViewportMetaSnapshot => ({
  pageUrl: 'http://t/',
  present: true,
  content: 'width=device-width, initial-scale=1',
  ...over,
});

// 1. Clean — present + width=device-width + no zoom-disable.
{
  const f = detectViewportMetaIssues(snap());
  assert(f.length === 0, 'clean viewport — no findings', JSON.stringify(f));
}

// 2. Missing tag → strict.
{
  const f = detectViewportMetaIssues(snap({ present: false, content: '' }));
  assert(
    f.length === 1 && f[0].kind === 'viewport.missing' && f[0].severity === 'strict',
    'missing tag fires strict viewport.missing',
    JSON.stringify(f),
  );
}

// 3. Tag without width=device-width → strict no-device-width.
{
  const f = detectViewportMetaIssues(snap({ content: 'initial-scale=1' }));
  assert(
    f.some((x) => x.kind === 'viewport.no-device-width' && x.severity === 'strict'),
    'no width=device-width fires strict',
    JSON.stringify(f),
  );
}

// 4. user-scalable=no → strict zoom-disabled.
{
  const f = detectViewportMetaIssues(
    snap({ content: 'width=device-width, initial-scale=1, user-scalable=no' }),
  );
  assert(
    f.some((x) => x.kind === 'viewport.zoom-disabled' && x.severity === 'strict'),
    'user-scalable=no fires strict',
    JSON.stringify(f),
  );
}

// 5. user-scalable=0 also disables zoom.
{
  const f = detectViewportMetaIssues(
    snap({ content: 'width=device-width, user-scalable=0' }),
  );
  assert(
    f.some((x) => x.kind === 'viewport.zoom-disabled'),
    'user-scalable=0 also flagged',
    JSON.stringify(f),
  );
}

// 6. maximum-scale=1 → strict zoom-disabled.
{
  const f = detectViewportMetaIssues(
    snap({ content: 'width=device-width, maximum-scale=1' }),
  );
  assert(
    f.some((x) => x.kind === 'viewport.zoom-disabled'),
    'maximum-scale=1 flagged',
    JSON.stringify(f),
  );
}

// 7. maximum-scale=0.9 (less than 1) → still strict.
{
  const f = detectViewportMetaIssues(
    snap({ content: 'width=device-width, maximum-scale=0.9' }),
  );
  assert(
    f.some((x) => x.kind === 'viewport.zoom-disabled'),
    'maximum-scale<1 flagged',
    JSON.stringify(f),
  );
}

// 8. maximum-scale=2 (allows 2× zoom) → no zoom-disable finding.
{
  const f = detectViewportMetaIssues(
    snap({ content: 'width=device-width, maximum-scale=2' }),
  );
  assert(
    !f.some((x) => x.kind === 'viewport.zoom-disabled'),
    'maximum-scale=2 passes',
    JSON.stringify(f),
  );
}

// 9. Combined breakage: missing width AND zoom-disabled.
{
  const f = detectViewportMetaIssues(
    snap({ content: 'initial-scale=1, user-scalable=no' }),
  );
  assert(
    f.some((x) => x.kind === 'viewport.no-device-width') &&
      f.some((x) => x.kind === 'viewport.zoom-disabled'),
    'combined breakage emits both findings',
    JSON.stringify(f),
  );
}

// 10. Whitespace tolerance — trailing comma, lots of spaces.
{
  const f = detectViewportMetaIssues(
    snap({ content: '  width = device-width ,  initial-scale = 1 , ' }),
  );
  assert(f.length === 0, 'whitespace+trailing comma tolerated', JSON.stringify(f));
}

// 11. Case-insensitivity on keys (per spec).
{
  const f = detectViewportMetaIssues(snap({ content: 'WIDTH=device-width' }));
  assert(f.length === 0, 'uppercase WIDTH key recognized', JSON.stringify(f));
}

// 12. parseViewportContent — token without `=` is allowed (empty value).
{
  const parsed = parseViewportContent('foo, width=device-width');
  assert(
    parsed['foo'] === '' && parsed['width'] === 'device-width',
    'parser handles flag-style tokens',
    JSON.stringify(parsed),
  );
}

console.log('\n=== viewportMeta.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
