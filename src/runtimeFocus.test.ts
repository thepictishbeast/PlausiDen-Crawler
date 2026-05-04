/**
 * runtimeFocus.test.ts — pure-function tests.
 */
import { detectRuntimeFocusIssues, type RuntimeFocusSnapshot, type FocusOffender } from './runtimeFocus.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): RuntimeFocusSnapshot => ({
  pageUrl: 'http://t/',
  viewport: { width: 1280, height: 800 },
  totalInteractive: 5,
  totalChecked: 5,
  invisibleFocus: [],
});

const offender = (overrides: Partial<FocusOffender> = {}): FocusOffender => ({
  selector: 'body > button',
  tag: 'button',
  text: 'X',
  beforeOutline: '0px none rgb(0,0,0)',
  afterOutline: '0px none rgb(0,0,0)',
  beforeBoxShadow: 'none',
  afterBoxShadow: 'none',
  beforeBorderTop: 'rgb(0,0,0) 0px',
  afterBorderTop: 'rgb(0,0,0) 0px',
  ...overrides,
});

// 1. Clean — no findings
{
  const f = detectRuntimeFocusIssues(baseSnap());
  assert(f.length === 0, 'clean snapshot — no findings', JSON.stringify(f));
}

// 2. One invisible-focus offender → strict
{
  const s = baseSnap();
  s.invisibleFocus.push(offender());
  const f = detectRuntimeFocusIssues(s);
  assert(
    f.some((x) => x.kind === 'focus.invisible-indicator' && x.severity === 'strict'),
    'invisible-indicator fires strict',
    JSON.stringify(f),
  );
}

// 3. Multiple offenders — single finding with offenderCount
{
  const s = baseSnap();
  for (let i = 0; i < 5; i++) s.invisibleFocus.push(offender({ selector: `body > button:nth-of-type(${i + 1})` }));
  const f = detectRuntimeFocusIssues(s);
  assert(f.length === 1 && (f[0].evidence.offenderCount === 5), 'multiple offenders aggregate to 1 finding', JSON.stringify(f));
}

console.log('\n=== runtimeFocus.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
