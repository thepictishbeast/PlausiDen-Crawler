/**
 * trustedTypesRuntime.test.ts — Trusted Types detector tests.
 * T76 cycle 57.
 *
 * Tests detectTrustedTypesIssues() directly with hand-constructed
 * snapshots. The browser-side probe is tested indirectly via the
 * Loom edit-serve dogfood audit (the only production target with
 * known sink-call patterns + a hash-pinned CSP today).
 */
import {
  detectTrustedTypesIssues,
  type TrustedTypesSnapshot,
} from './trustedTypesRuntime.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

function snap(over: Partial<TrustedTypesSnapshot>): TrustedTypesSnapshot {
  return {
    pageUrl: PAGE,
    sinks: [],
    hasRequireDirective: false,
    trustedTypesDirective: '',
    hasScripts: false,
    ...over,
  };
}

// 1. Empty page (no scripts, no sinks) → no findings.
{
  const f = detectTrustedTypesIssues(snap({}));
  assert(f.length === 0, 'empty page → no findings', JSON.stringify(f));
}

// 2. Page with scripts but no require-trusted-types-for → warn.
{
  const f = detectTrustedTypesIssues(snap({ hasScripts: true }));
  assert(
    f.length === 1 && f[0].kind === 'tt.directive-missing',
    'scripts without TT directive → directive-missing warn',
    JSON.stringify(f),
  );
}

// 3. Page with scripts AND require-trusted-types-for AND
//    allowlist → clean. (Without the allowlist, an info-level
//    tt.policy-undeclared still fires.)
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    hasRequireDirective: true,
    trustedTypesDirective: 'loom-editor',
  }));
  assert(f.length === 0, 'scripts WITH TT directive + allowlist → clean',
    JSON.stringify(f));
}

// 4. Sink called with plain string, no TT directive → warn.
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    sinks: [{ kind: 'innerHTML', preview: '<b>x</b>', trusted: false, t: 100 }],
  }));
  // Two findings: tt.unprotected-sink AND tt.directive-missing
  // (both fire because the directive is missing AND a sink was
  // called untrusted under that missing directive).
  assert(
    f.length === 2 &&
      f.some((x) => x.kind === 'tt.unprotected-sink') &&
      f.some((x) => x.kind === 'tt.directive-missing'),
    'untrusted sink without TT directive → both warns',
    JSON.stringify(f),
  );
}

// 5. Sink called with Trusted value → no unprotected-sink finding.
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    sinks: [{ kind: 'innerHTML', preview: '<b>x</b>', trusted: true, t: 100 }],
  }));
  // Only directive-missing fires (the page has scripts), but
  // the trusted sink doesn't create an unprotected-sink finding.
  assert(
    f.length === 1 && f[0].kind === 'tt.directive-missing',
    'trusted sink → only directive-missing fires',
    JSON.stringify(f),
  );
}

// 6. Untrusted sink WITH directive + allowlist → no unprotected-
//    sink finding (browser enforces; detector defers to enforcement).
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    hasRequireDirective: true,
    trustedTypesDirective: 'loom-editor',
    sinks: [{ kind: 'innerHTML', preview: '<b>x</b>', trusted: false, t: 100 }],
  }));
  assert(
    f.length === 0,
    'untrusted sink WITH TT directive + allowlist → no findings (browser blocks at runtime)',
    JSON.stringify(f),
  );
}

// 7. require-trusted-types-for set but no allowlist → info.
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    hasRequireDirective: true,
    trustedTypesDirective: '',
  }));
  // The wildcard form (no allowlist) trips tt.policy-undeclared.
  assert(
    f.length === 1 && f[0].kind === 'tt.policy-undeclared',
    'TT directive without allowlist → info',
    JSON.stringify(f),
  );
}

// 8. Full coverage — directive + allowlist + trusted sinks → clean.
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    hasRequireDirective: true,
    trustedTypesDirective: 'loom-editor',
    sinks: [{ kind: 'innerHTML', preview: '<b>x</b>', trusted: true, t: 100 }],
  }));
  assert(f.length === 0, 'full TT coverage → clean', JSON.stringify(f));
}

// 9. Multi-sink aggregation in the detail string.
{
  const f = detectTrustedTypesIssues(snap({
    hasScripts: true,
    sinks: [
      { kind: 'innerHTML', preview: '<b>x</b>', trusted: false, t: 100 },
      { kind: 'innerHTML', preview: '<i>y</i>', trusted: false, t: 200 },
      { kind: 'document.write', preview: 'z', trusted: false, t: 300 },
    ],
  }));
  const sinkFinding = f.find((x) => x.kind === 'tt.unprotected-sink');
  assert(
    !!sinkFinding && (sinkFinding.evidence.count as number) === 3,
    'multi-sink aggregates count',
    JSON.stringify(f),
  );
  assert(
    !!sinkFinding &&
      (sinkFinding.evidence.byKind as Record<string, number>).innerHTML === 2 &&
      (sinkFinding.evidence.byKind as Record<string, number>)['document.write'] === 1,
    'multi-sink byKind breakdown',
    JSON.stringify(f),
  );
}

// Summary
console.log('=== trustedTypesRuntime.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
for (const p of PASSED) console.log('  ✓ ' + p);
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  for (const f of FAILED) console.log(`  ✗ ${f.name}\n    ${f.reason}`);
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
