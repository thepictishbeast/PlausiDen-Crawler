/**
 * responseHeaderDetector.test.ts — pure-function tests for the
 * generic check-runner helper. T76.
 *
 * Verifies the helper's wiring contract: it must produce the
 * EXACT same captured-event shape that the inline form did
 * before the cycle-24 extraction. Any drift here invalidates
 * the HTTPS gate's existing baseline of 34 routes.
 */
import {
  makeResponseHeaderCheck,
  type PerStepRecord,
} from './responseHeaderDetector.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

interface FakeSnap { pageIsLocalhost: boolean; raw: string | null }
interface FakeFinding { severity: 'strict' | 'warn'; kind: string; detail: string }

function fakePage(url: string) {
  return { url: () => url } as any;
}

function makeRecorder() {
  const events: any[] = [];
  return { events, log: (e: any) => events.push(e) };
}

// 1. Localhost short-circuit honored when env var NOT set.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const headers = new Map<string, Record<string, string>>();
  headers.set('https://localhost:3000/', { 'x-test': 'present' });
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fake', eventKind: 'fake',
    page: fakePage('https://localhost:3000/'),
    topLevelResponseHeaders: headers,
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: (_u, h) => ({ pageIsLocalhost: true, raw: h?.['x-test'] ?? null }),
    detectIssues: (s) => s.pageIsLocalhost ? [] : [{ severity: 'warn', kind: 'fake.x', detail: 'd' }],
  });
  await check('label-1');
  assert(events.length === 0, 'localhost short-circuit emits no events', JSON.stringify(events));
  assert(findingsByStep.length === 1 && findingsByStep[0].findings.length === 0, 'per-step record still pushed', JSON.stringify(findingsByStep));
}

// 2. disableLocalhostExemption flips pageIsLocalhost.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const headers = new Map();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fake', eventKind: 'fake',
    page: fakePage('https://localhost:3000/'),
    topLevelResponseHeaders: headers,
    disableLocalhostExemption: true,
    findingsByStep, log,
    buildSnapshot: () => ({ pageIsLocalhost: true, raw: null }),
    detectIssues: (s) => s.pageIsLocalhost ? [] : [{ severity: 'warn', kind: 'fake.fired', detail: 'd' }],
  });
  await check('label-2');
  assert(events.length === 1 && events[0].kind === 'fake', 'env-var opt-out fires detector', JSON.stringify(events));
}

// 3. Captured-event field shape matches the pre-extraction inline form.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const headers = new Map();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fake', eventKind: 'csp-policy',
    page: fakePage('https://example.com/x/'),
    topLevelResponseHeaders: headers,
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: () => ({ pageIsLocalhost: false, raw: null }),
    detectIssues: () => [{ severity: 'strict', kind: 'csp.script-unsafe-inline', detail: 'D' }],
  });
  await check('step-3');
  const e = events[0];
  assert(e.kind === 'csp-policy', 'kind = eventKind', JSON.stringify(e));
  assert(e.text === '[csp.script-unsafe-inline] D', 'text format unchanged', JSON.stringify(e));
  assert(e.url === 'https://example.com/x/', 'url = page.url()', JSON.stringify(e));
  assert(e.severity === 'strict', 'severity passed through', JSON.stringify(e));
  assert(e.ruleId === 'csp.script-unsafe-inline', 'ruleId = finding.kind', JSON.stringify(e));
  assert(e.impact === 'serious', 'strict → serious', JSON.stringify(e));
}

// 4. warn → minor mapping.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fake', eventKind: 'fake',
    page: fakePage('https://example.com/'),
    topLevelResponseHeaders: new Map(),
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: () => ({ pageIsLocalhost: false, raw: null }),
    detectIssues: () => [{ severity: 'warn', kind: 'fake.w', detail: 'd' }],
  });
  await check('s4');
  assert(events[0].impact === 'minor', 'warn → minor', JSON.stringify(events[0]));
}

// 5. Multiple findings → multiple events, single per-step record.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fake', eventKind: 'fake',
    page: fakePage('https://example.com/'),
    topLevelResponseHeaders: new Map(),
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: () => ({ pageIsLocalhost: false, raw: null }),
    detectIssues: () => [
      { severity: 'warn', kind: 'a', detail: 'A' },
      { severity: 'strict', kind: 'b', detail: 'B' },
      { severity: 'warn', kind: 'c', detail: 'C' },
    ],
  });
  await check('s5');
  assert(events.length === 3, '3 findings → 3 events', JSON.stringify(events.length));
  assert(findingsByStep.length === 1 && findingsByStep[0].findings.length === 3, '1 record with all 3', JSON.stringify(findingsByStep));
}

// 6. Throw inside buildSnapshot → swallowed as pageerror.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fakeDet', eventKind: 'fake',
    page: fakePage('https://example.com/'),
    topLevelResponseHeaders: new Map(),
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: () => { throw new Error('parser blew up'); },
    detectIssues: () => [],
  });
  await check('s6');
  assert(events.length === 1 && events[0].kind === 'pageerror', 'throw → pageerror', JSON.stringify(events));
  assert(events[0].text.includes('[fakeDet]') && events[0].text.includes('s6'), 'pageerror tags detector + step', JSON.stringify(events[0]));
}

// 7. Throw inside detectIssues → also pageerror.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fakeDet', eventKind: 'fake',
    page: fakePage('https://example.com/'),
    topLevelResponseHeaders: new Map(),
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: () => ({ pageIsLocalhost: false, raw: null }),
    detectIssues: () => { throw new Error('classifier blew up'); },
  });
  await check('s7');
  assert(events.length === 1 && events[0].kind === 'pageerror', 'classifier throw → pageerror', JSON.stringify(events));
}

// 8. Empty findings → empty events, but per-step record still pushed.
{
  const findingsByStep: Array<PerStepRecord<FakeFinding>> = [];
  const { events, log } = makeRecorder();
  const check = makeResponseHeaderCheck<FakeSnap, FakeFinding>({
    detectorName: 'fake', eventKind: 'fake',
    page: fakePage('https://example.com/'),
    topLevelResponseHeaders: new Map(),
    disableLocalhostExemption: false,
    findingsByStep, log,
    buildSnapshot: () => ({ pageIsLocalhost: false, raw: null }),
    detectIssues: () => [],
  });
  await check('s8');
  assert(events.length === 0, 'no findings → no events', JSON.stringify(events));
  assert(findingsByStep.length === 1, 'per-step record still pushed (for JSON dump)', JSON.stringify(findingsByStep));
}

console.log('\n=== responseHeaderDetector.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
