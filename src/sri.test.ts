/**
 * sri.test.ts — Subresource Integrity detector tests. T76.
 */
import {
  detectSriIssues,
  type SriSnapshot,
  type CapturedSriElement,
} from './sri.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/page';
const PAGE_ORIGIN = 'https://example.com';

function elem(over: Partial<CapturedSriElement>): CapturedSriElement {
  return {
    tag: 'script',
    resourceUrl: 'https://cdn.example.org/lib.js',
    resourceOrigin: 'https://cdn.example.org',
    isCrossOrigin: true,
    integrity: null,
    crossorigin: null,
    linkRel: '',
    ...over,
  };
}

function snap(...elements: CapturedSriElement[]): SriSnapshot {
  return { pageUrl: PAGE, pageOrigin: PAGE_ORIGIN, elements };
}

// 1. No elements → no findings.
{
  const f = detectSriIssues(snap());
  assert(f.length === 0, 'empty page → no findings', JSON.stringify(f));
}

// 2. Same-origin script without integrity → no finding.
{
  const f = detectSriIssues(snap(elem({
    resourceUrl: 'https://example.com/local.js',
    resourceOrigin: 'https://example.com',
    isCrossOrigin: false,
  })));
  assert(f.length === 0, 'same-origin script ignored', JSON.stringify(f));
}

// 3. Cross-origin script no integrity → strict.
{
  const f = detectSriIssues(snap(elem({})));
  assert(
    f.length === 1 &&
      f[0].kind === 'sri.script-cross-origin-no-integrity' &&
      f[0].severity === 'strict',
    'cross-origin script no integrity → strict',
    JSON.stringify(f),
  );
}

// 4. Cross-origin stylesheet no integrity → warn.
{
  const f = detectSriIssues(snap(elem({
    tag: 'link',
    linkRel: 'stylesheet',
    resourceUrl: 'https://fonts.googleapis.com/css?family=Roboto',
    resourceOrigin: 'https://fonts.googleapis.com',
  })));
  assert(
    f.length === 1 &&
      f[0].kind === 'sri.style-cross-origin-no-integrity' &&
      f[0].severity === 'warn',
    'cross-origin stylesheet no integrity → warn',
    JSON.stringify(f),
  );
}

// 5. Cross-origin <link> with non-stylesheet rel → no finding.
{
  const f = detectSriIssues(snap(elem({
    tag: 'link',
    linkRel: 'icon',
    resourceUrl: 'https://other.example/favicon.ico',
    resourceOrigin: 'https://other.example',
  })));
  assert(f.length === 0, '<link rel="icon"> not flagged', JSON.stringify(f));
}

// 6. Cross-origin script with valid integrity + crossorigin → no findings.
{
  const f = detectSriIssues(snap(elem({
    integrity: 'sha384-deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef',
    crossorigin: 'anonymous',
  })));
  assert(f.length === 0, 'fully-correct SRI clean', JSON.stringify(f));
}

// 7. Integrity but no crossorigin → warn (silently ignored by browser).
{
  const f = detectSriIssues(snap(elem({
    integrity: 'sha384-deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef',
    crossorigin: null,
  })));
  assert(
    f.some((x) => x.kind === 'sri.script-cross-origin-no-crossorigin'),
    'integrity without crossorigin → warn',
    JSON.stringify(f),
  );
}

// 8. Weak algorithm sha1 → warn.
{
  const f = detectSriIssues(snap(elem({
    integrity: 'sha1-deadbeefdeadbeefdeadbeefdeadbeefdeadbeef',
    crossorigin: 'anonymous',
  })));
  assert(
    f.some((x) => x.kind === 'sri.script-weak-algorithm'),
    'sha1 → weak algorithm warn',
    JSON.stringify(f),
  );
}

// 9. Malformed integrity (no dash) → warn.
{
  const f = detectSriIssues(snap(elem({
    integrity: 'just-a-bunch-of-text-no-real-hash',
    crossorigin: 'anonymous',
  })));
  assert(
    f.some((x) => x.kind === 'sri.script-invalid-integrity-format'),
    'malformed integrity → invalid-format warn',
    JSON.stringify(f),
  );
}

// 10. Multiple algorithms (sha384 + sha512) → no findings (browser uses strongest).
{
  const f = detectSriIssues(snap(elem({
    integrity: 'sha384-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa sha512-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    crossorigin: 'anonymous',
  })));
  assert(f.length === 0, 'multiple-algo integrity OK', JSON.stringify(f));
}

// 11. Aggregation: 4 cross-origin scripts no integrity → 1 finding count=4.
{
  const f = detectSriIssues(snap(
    elem({ resourceUrl: 'https://cdn.example.org/a.js' }),
    elem({ resourceUrl: 'https://cdn.example.org/b.js' }),
    elem({ resourceUrl: 'https://cdn.example.org/c.js' }),
    elem({ resourceUrl: 'https://cdn.example.org/d.js' }),
  ));
  const fnd = f.find((x) => x.kind === 'sri.script-cross-origin-no-integrity');
  assert(fnd !== undefined && (fnd.evidence.count as number) === 4, 'aggregates count=4', JSON.stringify(f));
}

// 12. Examples capped at 5.
{
  const elems: CapturedSriElement[] = [];
  for (let i = 0; i < 8; i++) {
    elems.push(elem({ resourceUrl: `https://cdn.example.org/${i}.js` }));
  }
  const f = detectSriIssues(snap(...elems));
  const fnd = f.find((x) => x.kind === 'sri.script-cross-origin-no-integrity');
  assert(
    fnd !== undefined && (fnd.evidence.examples as string[]).length === 5,
    'examples capped at 5',
    JSON.stringify(fnd),
  );
  assert(fnd && (fnd.evidence.count as number) === 8, 'count still reflects all 8', JSON.stringify(fnd));
}

// 13. Mixed: cross-origin script + cross-origin style + same-origin → 2 findings.
{
  const f = detectSriIssues(snap(
    elem({ tag: 'script', resourceUrl: 'https://cdn.example.org/x.js' }),
    elem({ tag: 'link', linkRel: 'stylesheet', resourceUrl: 'https://cdn.example.org/x.css' }),
    elem({
      tag: 'script',
      resourceUrl: 'https://example.com/local.js',
      resourceOrigin: 'https://example.com',
      isCrossOrigin: false,
    }),
  ));
  assert(f.length === 2, 'mixed → 2 findings', JSON.stringify(f.map((x) => x.kind)));
  assert(
    f.some((x) => x.kind === 'sri.script-cross-origin-no-integrity'),
    'mixed: script flagged',
    JSON.stringify(f),
  );
  assert(
    f.some((x) => x.kind === 'sri.style-cross-origin-no-integrity'),
    'mixed: style flagged',
    JSON.stringify(f),
  );
}

// 14. crossorigin set to use-credentials counts as set → no warn for missing.
{
  const f = detectSriIssues(snap(elem({
    integrity: 'sha384-deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef',
    crossorigin: 'use-credentials',
  })));
  assert(
    !f.some((x) => x.kind === 'sri.script-cross-origin-no-crossorigin'),
    'use-credentials counts as crossorigin set',
    JSON.stringify(f),
  );
}

// 15. Empty crossorigin attribute (`crossorigin=""`) is valid per HTML spec
// (defaults to 'anonymous'). Treat as set.
{
  const f = detectSriIssues(snap(elem({
    integrity: 'sha384-deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef',
    crossorigin: '',
  })));
  assert(
    !f.some((x) => x.kind === 'sri.script-cross-origin-no-crossorigin'),
    'empty crossorigin attribute counts as anonymous',
    JSON.stringify(f),
  );
}

console.log('\n=== sri.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
