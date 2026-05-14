/**
 * linkUnderline.test.ts — pure-function tests for the
 * link-underline detector (T76). The page.evaluate() snapshot
 * capture (which does the running-text + visual-cue analysis)
 * is exercised end-to-end via the t76-detector-fixtures journey.
 */
import {
  detectLinkUnderlineIssues,
  type LinkUnderlineSnapshot,
  type CapturedColorOnlyLink,
} from './linkUnderline.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (
  candidates: CapturedColorOnlyLink[] = [],
): LinkUnderlineSnapshot => ({
  pageUrl: 'http://t/',
  candidates,
});

const cand = (over: Partial<CapturedColorOnlyLink> = {}): CapturedColorOnlyLink => ({
  selector: 'body > p > a',
  text: 'click here',
  textDecoration: 'none',
  fontWeight: '400',
  parentFontWeight: '400',
  href: '/x',
  ...over,
});

// 1. No candidates → no findings.
{
  const f = detectLinkUnderlineIssues(baseSnap());
  assert(f.length === 0, 'empty candidates — no findings', JSON.stringify(f));
}

// 2. One candidate → one warn finding with count=1.
{
  const f = detectLinkUnderlineIssues(baseSnap([cand()]));
  assert(
    f.length === 1 &&
      f[0].kind === 'link.color-only-distinction' &&
      f[0].severity === 'warn',
    'one candidate fires warn',
    JSON.stringify(f),
  );
  assert(
    (f[0].evidence.count as number) === 1,
    'count is 1',
    JSON.stringify(f[0].evidence),
  );
}

// 3. Multiple candidates aggregate to one finding with correct count.
{
  const f = detectLinkUnderlineIssues(baseSnap([
    cand({ selector: 'a:nth-of-type(1)' }),
    cand({ selector: 'a:nth-of-type(2)' }),
    cand({ selector: 'a:nth-of-type(3)' }),
  ]));
  assert(
    f.length === 1 && (f[0].evidence.count as number) === 3,
    '3 candidates → 1 finding count=3',
    JSON.stringify(f),
  );
}

// 4. Examples capped at 5.
{
  const candidates: CapturedColorOnlyLink[] = [];
  for (let i = 0; i < 8; i++) {
    candidates.push(cand({ selector: `a:nth-of-type(${i + 1})`, text: `link${i}` }));
  }
  const f = detectLinkUnderlineIssues(baseSnap(candidates));
  const examples = f[0].evidence.examples as string[];
  assert(examples.length === 5, 'examples capped at 5', JSON.stringify(examples));
  assert(
    (f[0].evidence.count as number) === 8,
    'count still reflects all 8',
    JSON.stringify(f[0].evidence),
  );
}

// 5. No-text link still produces a useful example string.
{
  const f = detectLinkUnderlineIssues(baseSnap([
    cand({ text: '', href: '/icon-only' }),
  ]));
  const examples = f[0].evidence.examples as string[];
  assert(
    examples[0].includes('(no text)'),
    'no-text candidate uses placeholder',
    JSON.stringify(examples),
  );
}

console.log('\n=== linkUnderline.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
