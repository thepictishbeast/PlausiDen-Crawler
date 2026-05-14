/**
 * htmlLang.test.ts — pure-function tests for the <html lang> detector
 * (T76).
 */
import {
  detectHtmlLangIssues,
  type HtmlLangSnapshot,
} from './htmlLang.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const snap = (over: Partial<HtmlLangSnapshot> = {}): HtmlLangSnapshot => ({
  pageUrl: 'http://t/',
  present: true,
  value: 'en',
  ...over,
});

// 1. Clean — `lang="en"`, no findings.
{
  const f = detectHtmlLangIssues(snap());
  assert(f.length === 0, 'lang=en — no findings', JSON.stringify(f));
}

// 2. `lang="en-US"` is valid BCP-47.
{
  const f = detectHtmlLangIssues(snap({ value: 'en-US' }));
  assert(f.length === 0, 'lang=en-US passes', JSON.stringify(f));
}

// 3. `lang="zh-Hans"` (script subtag) is valid.
{
  const f = detectHtmlLangIssues(snap({ value: 'zh-Hans' }));
  assert(f.length === 0, 'lang=zh-Hans passes', JSON.stringify(f));
}

// 4. Missing → strict.
{
  const f = detectHtmlLangIssues(snap({ present: false, value: '' }));
  assert(
    f.length === 1 && f[0].kind === 'lang.missing' && f[0].severity === 'strict',
    'missing lang fires strict',
    JSON.stringify(f),
  );
}

// 5. Empty → strict (and short-circuits — no piling on).
{
  const f = detectHtmlLangIssues(snap({ value: '' }));
  assert(
    f.length === 1 && f[0].kind === 'lang.empty' && f[0].severity === 'strict',
    'empty lang fires strict only',
    JSON.stringify(f),
  );
}

// 6. Whitespace-only counts as empty.
{
  const f = detectHtmlLangIssues(snap({ value: '   ' }));
  assert(
    f.some((x) => x.kind === 'lang.empty'),
    'whitespace-only fires empty',
    JSON.stringify(f),
  );
}

// 7. `lang="en_US"` (underscore) → invalid.
{
  const f = detectHtmlLangIssues(snap({ value: 'en_US' }));
  assert(
    f.some((x) => x.kind === 'lang.invalid'),
    'underscore separator flagged invalid',
    JSON.stringify(f),
  );
}

// 8. `lang="english"` (4 letters in primary) → invalid.
{
  const f = detectHtmlLangIssues(snap({ value: 'english' }));
  assert(
    f.some((x) => x.kind === 'lang.invalid'),
    '4+ letter primary flagged invalid',
    JSON.stringify(f),
  );
}

// 9. `lang="engish"` (typo for 'en'/'english') — 6 letters → invalid.
{
  const f = detectHtmlLangIssues(snap({ value: 'engish' }));
  assert(
    f.some((x) => x.kind === 'lang.invalid'),
    'engish typo (6-letter primary) flagged invalid',
    JSON.stringify(f),
  );
}

// 10. `lang="en "` (trailing space) — invalid.
{
  const f = detectHtmlLangIssues(snap({ value: 'en US' }));
  assert(
    f.some((x) => x.kind === 'lang.invalid'),
    'whitespace inside flagged invalid',
    JSON.stringify(f),
  );
}

// 11. `lang="xx"` (structurally valid 2-letter, not in common set) →
//     warn unknown-primary.
{
  const f = detectHtmlLangIssues(snap({ value: 'xx' }));
  assert(
    f.some((x) => x.kind === 'lang.unknown-primary'),
    'unknown 2-letter primary fires warn',
    JSON.stringify(f),
  );
}

// 12. Region in unknown primary doesn't suppress the warn.
{
  const f = detectHtmlLangIssues(snap({ value: 'xx-YY' }));
  assert(
    f.some((x) => x.kind === 'lang.unknown-primary'),
    'unknown primary even with region fires warn',
    JSON.stringify(f),
  );
}

// 13. `lang="EN"` (uppercase) — primary normalized to lowercase
//     before checking the common set.
{
  const f = detectHtmlLangIssues(snap({ value: 'EN' }));
  assert(f.length === 0, 'uppercase EN normalized to en', JSON.stringify(f));
}

// 14. `lang="en-US"` regardless of region case — passes.
{
  const f = detectHtmlLangIssues(snap({ value: 'en-us' }));
  assert(f.length === 0, 'lowercase en-us passes', JSON.stringify(f));
}

// 15. Empty subtags `lang="en--US"` → invalid.
{
  const f = detectHtmlLangIssues(snap({ value: 'en--US' }));
  assert(
    f.some((x) => x.kind === 'lang.invalid'),
    'empty subtag (doubled hyphen) flagged invalid',
    JSON.stringify(f),
  );
}

console.log('\n=== htmlLang.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
