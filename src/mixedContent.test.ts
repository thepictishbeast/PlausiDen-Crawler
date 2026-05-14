/**
 * mixedContent.test.ts — pure-function tests for the mixed-content
 * detector (T76).
 */
import {
  detectMixedContentIssues,
  type MixedContentSnapshot,
  type CapturedMixedAsset,
} from './mixedContent.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): MixedContentSnapshot => ({
  pageUrl: 'https://example.com/',
  pageIsHttps: true,
  assets: [],
});

const asset = (
  over: Partial<CapturedMixedAsset> = {},
): CapturedMixedAsset => ({
  selector: 'body > img',
  tag: 'img',
  url: 'http://other.example/x.png',
  attribute: 'src',
  classification: 'passive',
  ...over,
});

// 1. http page → never fires (out of scope).
{
  const s: MixedContentSnapshot = {
    pageUrl: 'http://example.com/',
    pageIsHttps: false,
    assets: [asset({ classification: 'active', tag: 'script' })],
  };
  const f = detectMixedContentIssues(s);
  assert(f.length === 0, 'http page never fires', JSON.stringify(f));
}

// 2. https page with no http assets → no findings.
{
  const f = detectMixedContentIssues(baseSnap());
  assert(f.length === 0, 'clean https page — no findings', JSON.stringify(f));
}

// 3. Active mixed (script over http) → strict.
{
  const s = baseSnap();
  s.assets.push(asset({ classification: 'active', tag: 'script', selector: 'body > script' }));
  const f = detectMixedContentIssues(s);
  assert(
    f.some((x) => x.kind === 'mixed-content.active' && x.severity === 'strict'),
    'active mixed → strict',
    JSON.stringify(f),
  );
}

// 4. Passive mixed (img over http) → warn.
{
  const s = baseSnap();
  s.assets.push(asset());
  const f = detectMixedContentIssues(s);
  assert(
    f.some((x) => x.kind === 'mixed-content.passive' && x.severity === 'warn'),
    'passive mixed → warn',
    JSON.stringify(f),
  );
}

// 5. Mixed form action → strict.
{
  const s = baseSnap();
  s.assets.push(asset({
    classification: 'form',
    tag: 'form',
    attribute: 'action',
    url: 'http://login.example/submit',
    selector: 'body > form',
  }));
  const f = detectMixedContentIssues(s);
  assert(
    f.some((x) => x.kind === 'mixed-content.form-action' && x.severity === 'strict'),
    'mixed form action → strict',
    JSON.stringify(f),
  );
}

// 6. Mixed of all three → 3 findings (independent).
{
  const s = baseSnap();
  s.assets.push(asset({ classification: 'active', tag: 'script' }));
  s.assets.push(asset({ classification: 'passive', tag: 'img' }));
  s.assets.push(asset({ classification: 'form', tag: 'form', attribute: 'action' }));
  const f = detectMixedContentIssues(s);
  assert(f.length === 3, 'all three classes emit 3 findings', JSON.stringify(f.map((x) => x.kind)));
}

// 7. Aggregation: multiple actives → one finding count = N.
{
  const s = baseSnap();
  for (let i = 0; i < 4; i++) {
    s.assets.push(asset({
      classification: 'active',
      tag: 'script',
      selector: `body > script:nth-of-type(${i + 1})`,
      url: `http://cdn.example/script${i}.js`,
    }));
  }
  const f = detectMixedContentIssues(s);
  const active = f.find((x) => x.kind === 'mixed-content.active');
  assert(
    !!active && (active.evidence.count as number) === 4,
    '4 actives → count=4',
    JSON.stringify(active),
  );
}

// 8. Examples capped at 5.
{
  const s = baseSnap();
  for (let i = 0; i < 9; i++) {
    s.assets.push(asset({
      classification: 'passive',
      tag: 'img',
      selector: `body > img:nth-of-type(${i + 1})`,
      url: `http://cdn.example/img${i}.png`,
    }));
  }
  const f = detectMixedContentIssues(s);
  const passive = f.find((x) => x.kind === 'mixed-content.passive');
  const examples = passive?.evidence.examples as string[];
  assert(examples.length === 5, 'examples capped at 5', JSON.stringify(examples));
  assert(
    (passive?.evidence.count as number) === 9,
    'count still reflects all 9',
    JSON.stringify(passive),
  );
}

console.log('\n=== mixedContent.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
