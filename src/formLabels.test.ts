/**
 * formLabels.test.ts — pure-function tests for the form-label
 * detector (T76). The page.evaluate() snapshot capture is exercised
 * by the e2e fixtures.
 */
import {
  detectFormLabelIssues,
  type FormLabelsSnapshot,
  type CapturedFormControl,
} from './formLabels.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const baseSnap = (): FormLabelsSnapshot => ({
  pageUrl: 'http://t/',
  controls: [],
});

const ctrl = (
  overrides: Partial<CapturedFormControl> = {},
): CapturedFormControl => ({
  selector: 'body > form > input',
  tag: 'input',
  type: 'text',
  accessibleName: 'Email',
  nameSource: 'label-for',
  placeholder: '',
  required: false,
  requiredIndicated: false,
  ...overrides,
});

// 1. Clean — properly labelled input, no findings.
{
  const s = baseSnap();
  s.controls.push(ctrl());
  const f = detectFormLabelIssues(s);
  assert(f.length === 0, 'clean labelled input — no findings', JSON.stringify(f));
}

// 2. nameSource='none' → strict no-label.
{
  const s = baseSnap();
  s.controls.push(ctrl({ accessibleName: '', nameSource: 'none' }));
  const f = detectFormLabelIssues(s);
  assert(
    f.some((x) => x.kind === 'form.no-label' && x.severity === 'strict'),
    'no-name fires strict',
    JSON.stringify(f),
  );
}

// 3. Empty accessibleName even with non-none source → still strict
//    (defensive: catches whitespace-only labels).
{
  const s = baseSnap();
  s.controls.push(ctrl({ accessibleName: '', nameSource: 'aria-label' }));
  const f = detectFormLabelIssues(s);
  assert(
    f.some((x) => x.kind === 'form.no-label'),
    'empty name even with source fires no-label',
    JSON.stringify(f),
  );
}

// 4. Placeholder-as-only-name → warn.
{
  const s = baseSnap();
  s.controls.push(
    ctrl({
      accessibleName: 'Your email',
      nameSource: 'placeholder',
      placeholder: 'Your email',
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    f.some((x) => x.kind === 'form.placeholder-only-label' && x.severity === 'warn'),
    'placeholder-only fires warn',
    JSON.stringify(f),
  );
  assert(
    !f.some((x) => x.kind === 'form.no-label'),
    'placeholder-only does NOT also fire no-label (would double-count)',
    JSON.stringify(f),
  );
}

// 5. Required without visible indicator → warn.
{
  const s = baseSnap();
  s.controls.push(
    ctrl({
      accessibleName: 'Email',
      required: true,
      requiredIndicated: false,
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    f.some(
      (x) => x.kind === 'form.required-no-indicator' && x.severity === 'warn',
    ),
    'required-no-indicator fires warn',
    JSON.stringify(f),
  );
}

// 6. Required WITH '*' indicator → no required finding.
{
  const s = baseSnap();
  s.controls.push(
    ctrl({
      accessibleName: 'Email *',
      required: true,
      requiredIndicated: true,
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    !f.some((x) => x.kind === 'form.required-no-indicator'),
    "required with '*' indicator passes",
    JSON.stringify(f),
  );
}

// 7. Required WITH the word 'required' in label → no finding.
{
  const s = baseSnap();
  s.controls.push(
    ctrl({
      accessibleName: 'Email (required)',
      required: true,
      requiredIndicated: true,
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    !f.some((x) => x.kind === 'form.required-no-indicator'),
    "required with 'required' in label passes",
    JSON.stringify(f),
  );
}

// 8. Mixed snapshot: one of each → 3 findings.
{
  const s = baseSnap();
  s.controls.push(ctrl({ accessibleName: '', nameSource: 'none' }));
  s.controls.push(
    ctrl({
      selector: 'body > form > input:nth-of-type(2)',
      nameSource: 'placeholder',
      accessibleName: 'X',
      placeholder: 'X',
    }),
  );
  s.controls.push(
    ctrl({
      selector: 'body > form > input:nth-of-type(3)',
      required: true,
      requiredIndicated: false,
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    f.length === 3,
    'mixed snapshot emits 3 findings',
    JSON.stringify(f),
  );
}

// 9. Hidden input would never reach here; defence-in-depth check —
//    if it did, it'd be skipped.
{
  const s = baseSnap();
  s.controls.push(
    ctrl({
      tag: 'input',
      type: 'hidden',
      accessibleName: '',
      nameSource: 'none',
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    f.length === 0,
    'hidden input slipping through is filtered',
    JSON.stringify(f),
  );
}

// 10. Submit button should never need a label fired against it.
{
  const s = baseSnap();
  s.controls.push(
    ctrl({
      tag: 'input',
      type: 'submit',
      accessibleName: '',
      nameSource: 'none',
    }),
  );
  const f = detectFormLabelIssues(s);
  assert(
    f.length === 0,
    'submit button is exempt from label requirement',
    JSON.stringify(f),
  );
}

// 11. Multiple no-label inputs → 1 finding with count.
{
  const s = baseSnap();
  for (let i = 0; i < 4; i++) {
    s.controls.push(
      ctrl({
        selector: `body > form > input:nth-of-type(${i + 1})`,
        accessibleName: '',
        nameSource: 'none',
      }),
    );
  }
  const f = detectFormLabelIssues(s);
  const noLabel = f.find((x) => x.kind === 'form.no-label');
  assert(
    !!noLabel && (noLabel.evidence.count as number) === 4,
    '4 no-label inputs aggregate to 1 finding with count=4',
    JSON.stringify(f),
  );
}

// 12. Examples list capped at 5 even with 10 offenders.
{
  const s = baseSnap();
  for (let i = 0; i < 10; i++) {
    s.controls.push(
      ctrl({
        selector: `body > form > input:nth-of-type(${i + 1})`,
        accessibleName: '',
        nameSource: 'none',
      }),
    );
  }
  const f = detectFormLabelIssues(s);
  const noLabel = f.find((x) => x.kind === 'form.no-label');
  const examples = noLabel?.evidence.examples as string[];
  assert(
    examples.length === 5,
    'examples capped at 5',
    JSON.stringify(examples),
  );
  assert(
    (noLabel?.evidence.count as number) === 10,
    'count still reflects all 10',
    JSON.stringify(noLabel),
  );
}

console.log('\n=== formLabels.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
