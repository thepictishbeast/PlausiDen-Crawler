#!/usr/bin/env node
// Smoke test for the assertCount step kind.
//
// AVP-2 Tier 1 (Existence Proof): exercise every branch of the new
// assertCount runner — literal match, var-expression match, recordAs
// binding, min/max bounds, polling-until-deadline, and every author-error
// path. No Playwright server required; we stub Page with a counter.
//
// Run: tsx scripts/test-assert-count.mjs
import { runStep } from '../src/journey.ts';

let passed = 0;
let failed = 0;

function assert(cond, label) {
  if (cond) { passed++; console.log(`  ok   ${label}`); }
  else { failed++; console.log(`  FAIL ${label}`); }
}

// Minimal Page stub: locator(sel).count() returns whatever the test
// sequence dictates; the counter advances on each call so tests can model
// "post-action settle" by returning a stale value first then the real one.
function makeStubPage(countSequence) {
  let i = 0;
  return {
    locator: (_sel) => ({
      count: async () => {
        const v = countSequence[Math.min(i, countSequence.length - 1)];
        i++;
        return v;
      },
    }),
    waitForTimeout: async (_ms) => {},
  };
}

async function test(label, fn) {
  try {
    await fn();
    console.log(`PASS: ${label}`);
  } catch (e) {
    failed++;
    console.log(`FAIL: ${label}: ${e.message}`);
  }
}

await test('literal count match', async () => {
  const page = makeStubPage([3]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 3 }, 1000, vars);
  assert(r.ok === true, 'ok=true');
  assert(!r.error, 'no error');
});

await test('literal count mismatch errors', async () => {
  const page = makeStubPage([2, 2, 2, 2, 2, 2]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 5 }, 400, vars);
  assert(r.ok === false, 'ok=false');
  assert(/count=2 did not satisfy count=5/.test(r.error || ''), 'error names actual & expected');
});

await test('recordAs binds var', async () => {
  const page = makeStubPage([7]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', min: 0, recordAs: 'preCount' }, 1000, vars);
  assert(r.ok === true, 'ok=true');
  assert(vars.get('preCount') === 7, 'var bound to actual count');
});

await test('var-expression preCount+1', async () => {
  const page = makeStubPage([8]);
  const vars = new Map([['preCount', 7]]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 'preCount+1' }, 1000, vars);
  assert(r.ok === true, 'ok=true');
});

await test('var-expression preCount-1', async () => {
  const page = makeStubPage([6]);
  const vars = new Map([['preCount', 7]]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 'preCount-1' }, 1000, vars);
  assert(r.ok === true, 'ok=true');
});

await test('var-expression unbound errors', async () => {
  const page = makeStubPage([3]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 'preCount+1' }, 400, vars);
  assert(r.ok === false, 'ok=false');
  assert(/not bound/.test(r.error || ''), 'error mentions unbound var');
});

await test('min lower bound respected', async () => {
  const page = makeStubPage([4]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', min: 3 }, 1000, vars);
  assert(r.ok === true, 'count=4 >= min=3 passes');
});

await test('min lower bound violated', async () => {
  const page = makeStubPage([2, 2, 2, 2, 2]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', min: 5 }, 400, vars);
  assert(r.ok === false, 'count=2 < min=5 fails');
});

await test('max upper bound respected', async () => {
  const page = makeStubPage([2]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', max: 3 }, 1000, vars);
  assert(r.ok === true, 'count=2 <= max=3 passes');
});

await test('max upper bound violated', async () => {
  const page = makeStubPage([5, 5, 5, 5, 5]);
  const vars = new Map();
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', max: 3 }, 400, vars);
  assert(r.ok === false, 'count=5 > max=3 fails');
});

await test('missing selector errors', async () => {
  const page = makeStubPage([3]);
  const r = await runStep(page, { kind: 'assertCount', count: 3 }, 1000, new Map());
  assert(r.ok === false, 'ok=false');
  assert(/missing selector/.test(r.error || ''), 'error mentions missing selector');
});

await test('no expectation errors', async () => {
  const page = makeStubPage([3]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row' }, 1000, new Map());
  assert(r.ok === false, 'ok=false');
  assert(/missing expectation/.test(r.error || ''), 'error mentions missing expectation');
});

await test('polling: stale 0 then settled 8', async () => {
  // Mimic the post-register settle pattern: count is briefly stale at 0
  // before the SPA refetch invalidates the cache and renders 8 rows.
  const page = makeStubPage([0, 0, 0, 8]);
  const vars = new Map([['preCount', 7]]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 'preCount+1' }, 1500, vars);
  assert(r.ok === true, 'eventually passes after polling');
});

await test('recordAs with invalid identifier errors', async () => {
  const page = makeStubPage([3]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 3, recordAs: '1bad' }, 1000, new Map());
  assert(r.ok === false, 'ok=false');
  assert(/not a valid identifier/.test(r.error || ''), 'error mentions invalid identifier');
});

await test('numeric string count works', async () => {
  const page = makeStubPage([42]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: '42' }, 1000, new Map());
  assert(r.ok === true, 'numeric string treated as literal');
});

await test('combined count + min + max all satisfied', async () => {
  const page = makeStubPage([4]);
  const r = await runStep(page, { kind: 'assertCount', selector: '.row', count: 4, min: 1, max: 10 }, 1000, new Map());
  assert(r.ok === true, 'all expectations met');
});

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
