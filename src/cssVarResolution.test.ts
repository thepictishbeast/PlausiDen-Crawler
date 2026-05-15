/**
 * cssVarResolution.test.ts — unit tests for the var() resolution
 * detector (T76 axis 52).
 */
import {
  detectCssVarIssues,
  type CapturedCssVarSnapshot,
} from './cssVarResolution.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

function snap(over: Partial<CapturedCssVarSnapshot>): CapturedCssVarSnapshot {
  return {
    pageUrl: 'https://example.com/',
    defined: [],
    referenced: [],
    unparseable: 0,
    refCount: {},
    ...over,
  };
}

// 1. No vars → no findings.
{
  const f = detectCssVarIssues(snap({}));
  assert(f.length === 0, 'empty page → no findings', JSON.stringify(f));
}

// 2. Defined + referenced + matched → no findings.
{
  const f = detectCssVarIssues(snap({
    defined: ['--loom-space-4', '--loom-color-ink'],
    referenced: ['--loom-space-4', '--loom-color-ink'],
    refCount: { '--loom-space-4': 12, '--loom-color-ink': 3 },
  }));
  assert(f.length === 0, 'all matched → no findings', JSON.stringify(f));
}

// 3. Reference without definition → strict.
{
  const f = detectCssVarIssues(snap({
    defined: ['--loom-color-ink'],
    referenced: ['--loom-color-ink', '--loom-space-6'],
    refCount: { '--loom-color-ink': 1, '--loom-space-6': 7 },
  }));
  const strict = f.find((x) => x.kind === 'css-var.undefined-reference');
  assert(
    strict !== undefined && strict.severity === 'strict',
    'undefined reference → strict',
    JSON.stringify(f),
  );
  assert(
    (strict?.evidence.examples as string[])?.includes('--loom-space-6') ?? false,
    'evidence names the undefined token',
    JSON.stringify(f),
  );
}

// 4. Definition without reference → warn (dead def).
{
  const f = detectCssVarIssues(snap({
    defined: ['--loom-space-4', '--loom-unused-token'],
    referenced: ['--loom-space-4'],
    refCount: { '--loom-space-4': 1 },
  }));
  const warn = f.find((x) => x.kind === 'css-var.dead-definition');
  assert(
    warn !== undefined && warn.severity === 'warn',
    'dead definition → warn',
    JSON.stringify(f),
  );
}

// 5. Both undefined-ref AND dead-def in same snapshot → both findings.
{
  const f = detectCssVarIssues(snap({
    defined: ['--loom-color-ink', '--unused'],
    referenced: ['--loom-color-ink', '--undefined-ref'],
    refCount: { '--loom-color-ink': 2, '--undefined-ref': 1 },
  }));
  assert(
    f.length === 2 &&
      f.some((x) => x.kind === 'css-var.undefined-reference') &&
      f.some((x) => x.kind === 'css-var.dead-definition'),
    'both diagnoses independent',
    JSON.stringify(f),
  );
}

// 6. Unparseable stylesheet → warn.
{
  const f = detectCssVarIssues(snap({
    defined: ['--x'],
    referenced: ['--x'],
    refCount: { '--x': 1 },
    unparseable: 2,
  }));
  const warn = f.find((x) => x.kind === 'css-var.unparseable-stylesheet');
  assert(
    warn !== undefined && (warn.evidence.count as number) === 2,
    'unparseable stylesheets → warn with count',
    JSON.stringify(f),
  );
}

// 7. Many undefined refs → top-8 sorted by frequency in detail.
{
  const referenced: string[] = [];
  const refCount: Record<string, number> = {};
  for (let i = 0; i < 12; i++) {
    const name = `--missing-${i.toString().padStart(2, '0')}`;
    referenced.push(name);
    refCount[name] = 12 - i; // first has highest count
  }
  const f = detectCssVarIssues(snap({ defined: [], referenced, refCount }));
  const strict = f.find((x) => x.kind === 'css-var.undefined-reference');
  assert(
    strict !== undefined && strict.detail.includes('--missing-00 (×12)'),
    'detail surfaces highest-frequency undefined token',
    strict?.detail || 'no strict',
  );
  assert(
    (strict?.evidence.examples as string[])?.length === 12,
    'evidence carries up to 12 examples',
    JSON.stringify(f),
  );
}

// 8. Reproduces the cycle-95c bug class: --loom-space-N defined
// in a separately-shipped stylesheet but page never linked it.
// Snapshot shows references but no defs.
{
  const f = detectCssVarIssues(snap({
    defined: ['--loom-bg', '--loom-fg'], // only critical-CSS tokens
    referenced: [
      '--loom-bg', '--loom-fg',
      '--loom-space-2', '--loom-space-3', '--loom-space-4',
      '--loom-space-5', '--loom-space-6', '--loom-space-8',
      '--loom-font-base', '--loom-font-sm', '--loom-font-lg',
    ],
    refCount: {
      '--loom-bg': 4, '--loom-fg': 8,
      '--loom-space-2': 11, '--loom-space-3': 14, '--loom-space-4': 22,
      '--loom-space-5': 9, '--loom-space-6': 6, '--loom-space-8': 4,
      '--loom-font-base': 3, '--loom-font-sm': 7, '--loom-font-lg': 2,
    },
  }));
  const strict = f.find((x) => x.kind === 'css-var.undefined-reference');
  assert(
    strict !== undefined && (strict.evidence.count as number) === 9,
    'cycle 95c reproducer: 9 undefined --loom-* tokens flagged',
    JSON.stringify(f),
  );
  assert(
    strict?.detail.includes('--loom-space-4 (×22)') ?? false,
    'most-referenced undefined token (--loom-space-4 ×22) named first',
    strict?.detail || 'no strict',
  );
}

console.log('\n=== cssVarResolution.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
