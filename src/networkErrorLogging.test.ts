/**
 * networkErrorLogging.test.ts — NEL detector tests. T76 cycle 65.
 */
import {
  buildNelSnapshot,
  detectNelIssues,
} from './networkErrorLogging.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const PROD = 'https://example.com/';
const LOCAL = 'http://127.0.0.1:8123/';

// 1. localhost exempt
{
  const f = detectNelIssues(buildNelSnapshot(LOCAL, undefined));
  assert(f.length === 0, 'localhost → no findings', JSON.stringify(f));
}

// 2. no header → missing warn
{
  const f = detectNelIssues(buildNelSnapshot(PROD, undefined));
  assert(
    f.length === 1 && f[0].kind === 'nel.missing',
    'no NEL header → missing warn',
    JSON.stringify(f),
  );
}

// 3. fully-configured NEL → clean
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '{"report_to":"default","max_age":2592000,"failure_fraction":1.0}',
  }));
  assert(f.length === 0, 'fully-configured NEL → clean', JSON.stringify(f));
}

// 4. invalid JSON → invalid warn
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: 'not json',
  }));
  assert(
    f.length === 1 && f[0].kind === 'nel.invalid',
    'unparseable → invalid warn',
    JSON.stringify(f),
  );
}

// 5. JSON array instead of object → invalid
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '[]',
  }));
  assert(
    f.length === 1 && f[0].kind === 'nel.invalid',
    'JSON array → invalid warn',
    JSON.stringify(f),
  );
}

// 6. missing report_to field → warn
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '{"max_age":2592000}',
  }));
  assert(
    f.length === 1 && f[0].kind === 'nel.report-to-missing',
    'missing report_to → warn',
    JSON.stringify(f),
  );
}

// 7. empty report_to string → warn
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '{"report_to":"","max_age":2592000}',
  }));
  assert(
    f.length === 1 && f[0].kind === 'nel.report-to-missing',
    'empty report_to → warn',
    JSON.stringify(f),
  );
}

// 8. max_age=0 → opt-out warn
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '{"report_to":"default","max_age":0}',
  }));
  assert(
    f.some((x) => x.kind === 'nel.max-age-zero'),
    'max_age=0 → opt-out warn',
    JSON.stringify(f),
  );
}

// 9. failure_fraction=0 → defeats-purpose warn
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '{"report_to":"default","max_age":2592000,"failure_fraction":0}',
  }));
  assert(
    f.some((x) => x.kind === 'nel.failure-fraction-zero'),
    'failure_fraction=0 → defeats-purpose warn',
    JSON.stringify(f),
  );
}

// 10. case-insensitive header lookup
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    NEL: '{"report_to":"default","max_age":2592000}',
  }));
  assert(f.length === 0, 'mixed-case NEL header → clean', JSON.stringify(f));
}

// 11. multiple warns coexist
{
  const f = detectNelIssues(buildNelSnapshot(PROD, {
    nel: '{"max_age":0,"failure_fraction":0}',
  }));
  const kinds = f.map((x) => x.kind).sort();
  assert(
    kinds.includes('nel.report-to-missing') &&
      kinds.includes('nel.max-age-zero') &&
      kinds.includes('nel.failure-fraction-zero'),
    'multiple issues fire independently',
    JSON.stringify(f),
  );
}

console.log('=== networkErrorLogging.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
for (const p of PASSED) console.log('  ✓ ' + p);
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  for (const f of FAILED) console.log(`  ✗ ${f.name}\n    ${f.reason}`);
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
