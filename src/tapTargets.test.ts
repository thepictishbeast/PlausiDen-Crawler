/**
 * tapTargets.test.ts — pure-function tests for the tap-target
 * detector (T76). Tests detect-side only; the page.evaluate()
 * snapshot capture is exercised by the e2e fixtures.
 */
import {
  detectTapTargetIssues,
  type TapTargetsSnapshot,
  type CapturedTapTarget,
} from './tapTargets.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): TapTargetsSnapshot => ({
  pageUrl: 'http://t/',
  viewportWidth: 375,
  viewportHeight: 667,
  targets: [],
});

const target = (
  overrides: Partial<CapturedTapTarget> = {},
): CapturedTapTarget => ({
  selector: 'body > button',
  tag: 'button',
  role: '',
  width: 44,
  height: 44,
  inline: false,
  accessibleName: 'Sign up',
  ...overrides,
});

// 1. Clean — all targets meet the recommended size, no findings.
{
  const s = baseSnap();
  s.targets.push(target());
  s.targets.push(target({ selector: 'body > a', tag: 'a', width: 80, height: 48 }));
  const f = detectTapTargetIssues(s);
  assert(f.length === 0, 'clean snapshot — no findings', JSON.stringify(f));
}

// 2. Below 24×24 → strict.
{
  const s = baseSnap();
  s.targets.push(target({ width: 16, height: 16, accessibleName: 'X' }));
  const f = detectTapTargetIssues(s);
  assert(
    f.some((x) => x.kind === 'tap.too-small' && x.severity === 'strict'),
    '<24×24 fires strict',
    JSON.stringify(f),
  );
}

// 3. Between 24×24 and 44×44 → warn (AAA recommendation).
{
  const s = baseSnap();
  s.targets.push(target({ width: 32, height: 32 }));
  const f = detectTapTargetIssues(s);
  assert(
    f.some(
      (x) => x.kind === 'tap.below-recommended' && x.severity === 'warn',
    ),
    '24-43px fires warn',
    JSON.stringify(f),
  );
}

// 4. Inline-in-sentence link is exempt regardless of size.
//    (WCAG 2.5.8 explicit exception.)
{
  const s = baseSnap();
  s.targets.push(target({
    selector: 'body > p > a',
    tag: 'a',
    inline: true,
    width: 8,
    height: 12,
    accessibleName: 'click here',
  }));
  const f = detectTapTargetIssues(s);
  assert(
    f.length === 0,
    'inline-in-sentence link gets exception',
    JSON.stringify(f),
  );
}

// 5. 0×0 targets are filtered (likely a layout artefact, not a
//    real tap target). Avoids false positives from invisible overlays.
{
  const s = baseSnap();
  s.targets.push(target({ width: 0, height: 0 }));
  const f = detectTapTargetIssues(s);
  assert(f.length === 0, '0×0 targets are skipped', JSON.stringify(f));
}

// 6. Multiple offenders aggregate into one finding with example list.
{
  const s = baseSnap();
  for (let i = 0; i < 7; i++) {
    s.targets.push(
      target({
        selector: `body > button:nth-of-type(${i + 1})`,
        width: 12,
        height: 12,
      }),
    );
  }
  const f = detectTapTargetIssues(s);
  const strict = f.find((x) => x.kind === 'tap.too-small');
  assert(
    !!strict && (strict.evidence.count as number) === 7,
    '7 small targets aggregate to 1 finding with count=7',
    JSON.stringify(f),
  );
  // Examples capped at 5 to keep findings readable.
  const examples = strict?.evidence.examples as string[];
  assert(
    examples.length === 5,
    'examples capped at 5',
    JSON.stringify(examples),
  );
}

// 7. Uneven dimensions — min(width, height) is what's checked.
//    A 100×10 button is too thin to tap reliably.
{
  const s = baseSnap();
  s.targets.push(target({ width: 100, height: 10 }));
  const f = detectTapTargetIssues(s);
  assert(
    f.some((x) => x.kind === 'tap.too-small'),
    '100×10 (height < 24) flagged as too-small',
    JSON.stringify(f),
  );
}

// 8. Boundary exact 24×24 → not flagged as too-small (>= 24 passes
//    AA), but flagged as below-recommended (< 44).
{
  const s = baseSnap();
  s.targets.push(target({ width: 24, height: 24 }));
  const f = detectTapTargetIssues(s);
  assert(
    !f.some((x) => x.kind === 'tap.too-small'),
    'exact 24×24 not strict',
    JSON.stringify(f),
  );
  assert(
    f.some((x) => x.kind === 'tap.below-recommended'),
    'exact 24×24 still warns (below AAA 44)',
    JSON.stringify(f),
  );
}

// 9. Boundary exact 44×44 → no findings at all (meets both AA + AAA).
{
  const s = baseSnap();
  s.targets.push(target({ width: 44, height: 44 }));
  const f = detectTapTargetIssues(s);
  assert(f.length === 0, 'exact 44×44 passes everything', JSON.stringify(f));
}

console.log('\n=== tapTargets.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
