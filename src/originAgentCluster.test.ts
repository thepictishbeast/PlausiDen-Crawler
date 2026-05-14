/**
 * originAgentCluster.test.ts — Origin-Agent-Cluster detector tests. T76 cycle 45.
 */
import {
  buildOriginAgentClusterSnapshot,
  detectOriginAgentClusterIssues,
} from './originAgentCluster.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PAGE = 'https://example.com/';

// 1. No header → missing warn.
{
  const f = detectOriginAgentClusterIssues(buildOriginAgentClusterSnapshot(PAGE, {}));
  assert(
    f.length === 1 && f[0].kind === 'origin-agent-cluster.missing',
    'no header → missing',
    JSON.stringify(f),
  );
}

// 2. Localhost exempt.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot('https://localhost:3000/', {}),
  );
  assert(f.length === 0, 'localhost exempt', JSON.stringify(f));
}

// 3. ?1 → no findings.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': '?1' }),
  );
  assert(f.length === 0, '?1 clean', JSON.stringify(f));
}

// 4. ?0 → disabled warn.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': '?0' }),
  );
  assert(
    f.length === 1 && f[0].kind === 'origin-agent-cluster.disabled',
    '?0 → disabled',
    JSON.stringify(f),
  );
}

// 5. Garbage → invalid warn.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': 'true' }),
  );
  assert(
    f.length === 1 && f[0].kind === 'origin-agent-cluster.invalid',
    'invalid value',
    JSON.stringify(f),
  );
}

// 6. Empty string → invalid (empty isn't ?0 or ?1).
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': '' }),
  );
  assert(
    f.some((x) => x.kind === 'origin-agent-cluster.invalid'),
    'empty → invalid',
    JSON.stringify(f),
  );
}

// 7. Header name case-insensitive.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'Origin-Agent-Cluster': '?1' }),
  );
  assert(f.length === 0, 'header name case-insensitive', JSON.stringify(f));
}

// 8. Whitespace tolerated.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': '  ?1  ' }),
  );
  assert(f.length === 0, 'whitespace tolerated', JSON.stringify(f));
}

// 9. The structured-fields-true (`?true` is NOT the spec form) is invalid.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': '?true' }),
  );
  assert(
    f.some((x) => x.kind === 'origin-agent-cluster.invalid'),
    'bare ?true rejected',
    JSON.stringify(f),
  );
}

// 10. Trailing semicolon (some servers add it) - we don't yet support
//     SF parameters, so it counts as invalid. Documented expectation.
{
  const f = detectOriginAgentClusterIssues(
    buildOriginAgentClusterSnapshot(PAGE, { 'origin-agent-cluster': '?1;' }),
  );
  assert(
    f.some((x) => x.kind === 'origin-agent-cluster.invalid'),
    'trailing semicolon currently invalid',
    JSON.stringify(f),
  );
}

console.log('\n=== originAgentCluster.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
