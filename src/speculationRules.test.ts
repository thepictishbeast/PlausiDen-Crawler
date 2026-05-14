/**
 * speculationRules.test.ts — Speculation Rules detector tests.
 * T76 cycle 92.
 */
import {
  detectSpeculationRulesIssues,
  type SpeculationRulesSnapshot,
  type CapturedSpecRulesBlock,
} from './speculationRules.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/page';
const PAGE_ORIGIN = 'https://example.com';

function block(over: Partial<CapturedSpecRulesBlock>): CapturedSpecRulesBlock {
  return {
    rawText: '{}',
    parsedOk: true,
    parseError: null,
    prefetchCount: 0,
    prerenderCount: 0,
    hasEager: false,
    hasLegacyCrossOriginUrls: false,
    hasUnshieldedCrossOriginPrerender: false,
    unshieldedCrossOriginExamples: [],
    legacyCrossOriginUrlExamples: [],
    isEmptyRuleSet: false,
    ...over,
  };
}

function snap(...blocks: CapturedSpecRulesBlock[]): SpeculationRulesSnapshot {
  return { pageUrl: PAGE, pageOrigin: PAGE_ORIGIN, blocks };
}

// 1. No blocks → no findings.
{
  const f = detectSpeculationRulesIssues(snap());
  assert(f.length === 0, 'empty page → no findings', JSON.stringify(f));
}

// 2. A valid same-origin prerender → no findings.
{
  const f = detectSpeculationRulesIssues(snap(block({
    prerenderCount: 1,
  })));
  assert(f.length === 0, 'same-origin prerender ignored', JSON.stringify(f));
}

// 3. Invalid JSON → invalid-json warn.
{
  const f = detectSpeculationRulesIssues(snap(block({
    parsedOk: false,
    parseError: 'Unexpected token , in JSON at position 12',
  })));
  assert(
    f.length === 1 &&
      f[0].kind === 'speculation-rules.invalid-json' &&
      f[0].severity === 'warn',
    'invalid JSON → warn',
    JSON.stringify(f),
  );
}

// 4. Cross-origin prerender without anonymous-ip → strict.
{
  const f = detectSpeculationRulesIssues(snap(block({
    prerenderCount: 1,
    hasUnshieldedCrossOriginPrerender: true,
    unshieldedCrossOriginExamples: ['https://other.example/x'],
  })));
  assert(
    f.length === 1 &&
      f[0].kind ===
        'speculation-rules.cross-origin-prerender-no-anonymous-ip' &&
      f[0].severity === 'strict',
    'unshielded cross-origin prerender → strict',
    JSON.stringify(f),
  );
}

// 5. Legacy urls-form with cross-origin → warn.
{
  const f = detectSpeculationRulesIssues(snap(block({
    prefetchCount: 1,
    hasLegacyCrossOriginUrls: true,
    legacyCrossOriginUrlExamples: ['https://other.example/y'],
  })));
  assert(
    f.length === 1 &&
      f[0].kind === 'speculation-rules.legacy-cross-origin-urls-form' &&
      f[0].severity === 'warn',
    'legacy cross-origin urls form → warn',
    JSON.stringify(f),
  );
}

// 6. eagerness: eager → warn.
{
  const f = detectSpeculationRulesIssues(snap(block({
    prefetchCount: 1,
    hasEager: true,
  })));
  assert(
    f.length === 1 &&
      f[0].kind === 'speculation-rules.eager-eagerness' &&
      f[0].severity === 'warn',
    'eager eagerness → warn',
    JSON.stringify(f),
  );
}

// 7. Empty rule set → warn.
{
  const f = detectSpeculationRulesIssues(snap(block({
    isEmptyRuleSet: true,
  })));
  assert(
    f.length === 1 &&
      f[0].kind === 'speculation-rules.empty-rule-set' &&
      f[0].severity === 'warn',
    'empty rule set → warn',
    JSON.stringify(f),
  );
}

// 8. Multiple problems on the same block → multiple findings.
{
  const f = detectSpeculationRulesIssues(snap(block({
    prerenderCount: 2,
    hasUnshieldedCrossOriginPrerender: true,
    unshieldedCrossOriginExamples: ['https://other.example/z'],
    hasEager: true,
    hasLegacyCrossOriginUrls: true,
    legacyCrossOriginUrlExamples: ['https://other.example/w'],
  })));
  assert(
    f.length === 3 &&
      f.some((x) =>
        x.kind === 'speculation-rules.cross-origin-prerender-no-anonymous-ip'
      ) &&
      f.some((x) => x.kind === 'speculation-rules.legacy-cross-origin-urls-form') &&
      f.some((x) => x.kind === 'speculation-rules.eager-eagerness'),
    'multiple problems → multiple findings',
    JSON.stringify(f),
  );
}

// 9. Two blocks, one invalid + one valid-but-eager → 2 findings.
{
  const f = detectSpeculationRulesIssues(snap(
    block({ parsedOk: false, parseError: 'EOF' }),
    block({ prefetchCount: 1, hasEager: true }),
  ));
  assert(
    f.length === 2 &&
      f.some((x) => x.kind === 'speculation-rules.invalid-json') &&
      f.some((x) => x.kind === 'speculation-rules.eager-eagerness'),
    'two-block diagnoses are independent',
    JSON.stringify(f),
  );
}

// 10. Invalid JSON block does NOT trigger empty-rule-set or other
// follow-on checks (early continue).
{
  const f = detectSpeculationRulesIssues(snap(block({
    parsedOk: false,
    parseError: 'oops',
    // these would emit if checks weren't gated on parsedOk
    isEmptyRuleSet: true,
    hasEager: true,
  })));
  assert(
    f.length === 1 && f[0].kind === 'speculation-rules.invalid-json',
    'invalid JSON short-circuits other checks',
    JSON.stringify(f),
  );
}

// 11. Examples are passed through to evidence for audit-trail.
{
  const f = detectSpeculationRulesIssues(snap(block({
    prerenderCount: 1,
    hasUnshieldedCrossOriginPrerender: true,
    unshieldedCrossOriginExamples: [
      'https://attacker.example/a',
      'https://attacker.example/b',
      'https://attacker.example/c',
    ],
  })));
  const fnd = f.find(
    (x) => x.kind === 'speculation-rules.cross-origin-prerender-no-anonymous-ip',
  );
  const examples = fnd?.evidence.examples as string[];
  assert(
    examples !== undefined && examples.length === 3,
    'evidence carries audit-trail examples',
    JSON.stringify(f),
  );
}

// 12. Strict severity outweighs warns — at least one strict
// surfaces when present (composite finding ordering invariant).
{
  const f = detectSpeculationRulesIssues(snap(block({
    prerenderCount: 1,
    hasUnshieldedCrossOriginPrerender: true,
    unshieldedCrossOriginExamples: ['https://x.example/y'],
    hasEager: true,
  })));
  assert(
    f.some((x) => x.severity === 'strict') &&
      f.some((x) => x.severity === 'warn'),
    'both severities present when both kinds detected',
    JSON.stringify(f),
  );
}

console.log('\n=== speculationRules.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
