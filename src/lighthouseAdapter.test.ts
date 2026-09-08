/**
 * lighthouseAdapter.test.ts — the adapter boundary, against a REAL LHR.
 *
 * `fixtures/lighthouse/sample-lhr.json` is not hand-written: it is a
 * trimmed copy of an actual Lighthouse 13.4.1 run against
 * https://plausiden.com/, keeping every allowlisted audit verbatim plus
 * the ones that must be dropped. A hand-written fixture would only ever
 * prove that the adapter agrees with whoever wrote the fixture.
 *
 * The three things this has to prove, because each one has a matching
 * way of being quietly wrong:
 *
 *   1. Nothing outside the allowlist escapes. An a11y id reaching
 *      analytics double-counts against axe-core; an LCP/CLS id
 *      double-counts against the web-vitals dependency.
 *   2. `score: null` survives as null. Coerced to 0 it fabricates a
 *      failure, to 1 a pass.
 *   3. Every way the run can be worthless has a name. An empty result
 *      and a clean result must never look alike.
 */
import { readFileSync } from 'node:fs';
import { adapt } from './lighthouseAdapter.js';
import { ALLOWED_IDS, BLOCKED_BY_NODE_VERSION } from './lighthouseAllowlist.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const LHR = JSON.parse(
  readFileSync(new URL('../fixtures/lighthouse/sample-lhr.json', import.meta.url), 'utf8'),
);
const NOW = 1_757_300_000; // 2026-09-08T04:13:20Z — fixed, not a clock race.

const base = (over: Record<string, unknown> = {}) =>
  adapt({
    requestedUrl: 'https://plausiden.com/',
    formFactor: 'mobile',
    lhr: LHR,
    durationMs: 18_342,
    nowSec: NOW,
    throttling: { method: 'provided' },
    emulation: { width: 412, height: 823 },
    ...over,
  } as any);

// 1. The happy path emits exactly one run record, first, then findings.
{
  const out = base();
  const runs = out.records.filter((r) => r.kind === 'ux_run');
  assert(runs.length === 1, 'exactly one ux_run record', `got ${runs.length}`);
  assert(
    out.records[0].kind === 'ux_run',
    'the run record is FIRST (analytics enforces the foreign key)',
    out.records[0].kind,
  );
  assert(
    out.records.length === 1 + ALLOWED_IDS.length,
    `1 run + ${ALLOWED_IDS.length} findings`,
    `got ${out.records.length}`,
  );
}

// 2. NOTHING outside the allowlist escapes. This is the whole point of
//    the boundary: the fixture deliberately carries a11y and vitals
//    audits that would double-count downstream.
{
  const out = base();
  const emitted = out.records
    .filter((r) => r.kind === 'ux_finding')
    .map((r) => r.data.audit as string);
  const strays = emitted.filter((id) => !ALLOWED_IDS.includes(id));
  assert(strays.length === 0, 'no id outside the allowlist is emitted', strays.join(','));

  const forbidden = [
    'color-contrast',
    'aria-allowed-attr',
    'image-alt',
    'largest-contentful-paint',
    'cumulative-layout-shift',
    'total-blocking-time',
    'render-blocking-insight',
    'image-delivery-insight',
  ];
  const present = forbidden.filter((f) => f in LHR.audits);
  assert(
    present.length === forbidden.length,
    'the fixture really does carry the audits that must be dropped',
    `only ${present.join(',')} present — the test would pass vacuously`,
  );
  const leaked = forbidden.filter((f) => emitted.includes(f));
  assert(
    leaked.length === 0,
    'axe/vitals/detector-duplicating ids cannot escape the adapter',
    leaked.join(','),
  );
}

// 3. The URL is recorded, both halves, and a redirect is called out.
{
  const out = base();
  const run = out.records[0];
  assert(
    run.data.requested_url === 'https://plausiden.com/',
    'requested_url is recorded',
    String(run.data.requested_url),
  );
  assert(
    run.data.final_url === LHR.finalDisplayedUrl,
    'final_url comes from finalDisplayedUrl (the LH13 field, not the remembered one)',
    String(run.data.final_url),
  );
  assert(run.data.redirected === false, 'no redirect on the fixture', String(run.data.redirected));

  const moved = base({
    lhr: { ...LHR, finalDisplayedUrl: 'https://plausiden.com/signin' },
  });
  assert(
    moved.records[0].data.redirected === true,
    'a redirect is flagged rather than silently trended as the same page',
    JSON.stringify(moved.records[0].data.final_url),
  );
}

// 4. score:null survives as null.
{
  const withNull = JSON.parse(JSON.stringify(LHR));
  withNull.audits['speed-index'].score = null;
  withNull.audits['speed-index'].scoreDisplayMode = 'notApplicable';
  const out = base({ lhr: withNull });
  const f = out.records.find((r) => r.data.audit === 'speed-index')!;
  assert(f.data.score === null, 'a null score stays null, never 0 and never 1', String(f.data.score));
}

// 5. Every audit unscored is `unusable`, not `clean`. Fourteen null
//    scores would otherwise be a fully-accepted, fully-green lie.
{
  const allNull = JSON.parse(JSON.stringify(LHR));
  for (const id of ALLOWED_IDS) {
    allNull.audits[id].score = null;
    allNull.audits[id].scoreDisplayMode = 'notApplicable';
  }
  const out = base({ lhr: allNull });
  assert(out.state === 'unusable', 'all-unscored is unusable, not clean', out.state);
  assert(
    String(out.records[0].data.error).includes('nothing to measure'),
    'and it says why',
    String(out.records[0].data.error),
  );
}

// 6. A renamed or dropped audit id is LOUD. This is the version-drift
//    guard, and it is the failure that actually happened: Lighthouse 13
//    dropped render-blocking-resources, third-party-summary,
//    uses-responsive-images and modern-image-formats, so an adapter
//    coded from the docs would have emitted nothing and called it clean.
{
  const shrunk = JSON.parse(JSON.stringify(LHR));
  delete shrunk.audits['speed-index'];
  delete shrunk.audits['bootup-time'];
  const out = base({ lhr: shrunk });
  assert(out.state === 'unusable', 'a missing allowlisted id is unusable', out.state);
  const err = String(out.records[0].data.error);
  assert(
    err.includes('speed-index') && err.includes('bootup-time'),
    'and it NAMES the ids, so the fix is obvious',
    err,
  );
  assert(
    out.records.filter((r) => r.kind === 'ux_finding').length === ALLOWED_IDS.length - 2,
    'the records it did get are still emitted for forensics',
    String(out.records.length),
  );
}

// 7. Lighthouse throwing produces a named failure, not an empty file.
{
  const out = base({ lhr: null, error: 'could not launch Chromium: ENOENT' });
  assert(out.state === 'failed', 'a throw is `failed`', out.state);
  assert(out.records.length === 1, 'and still writes a run record', String(out.records.length));
  assert(
    String(out.records[0].data.error).includes('ENOENT'),
    'carrying the real error text',
    String(out.records[0].data.error),
  );
  assert(
    out.records[0].data.audits_emitted === 0 && out.records[0].data.audits_scored === 0,
    'with zero counted audits, so nothing downstream can read it as a pass',
    JSON.stringify(out.records[0].data),
  );
}

// 8. An LHR carrying its own runtimeError is a failure too — this is how
//    a 500 or a DNS failure comes back: as a valid LHR object.
{
  const errored = {
    ...LHR,
    runtimeError: { code: 'ERRORED_DOCUMENT_REQUEST', message: 'Status code: 500' },
  };
  const out = base({ lhr: errored });
  assert(out.state === 'failed', 'runtimeError in a valid LHR is `failed`', out.state);
  assert(
    String(out.records[0].data.error).includes('ERRORED_DOCUMENT_REQUEST'),
    'named by its Lighthouse code',
    String(out.records[0].data.error),
  );
}

// 9. The wire envelope is the one inbox.rs parses, with a plausible ts.
{
  const out = base();
  for (const r of out.records) {
    assert(r.v === 1, 'v is 1', JSON.stringify(r.v));
    assert(typeof r.id === 'string' && r.id.length > 0 && r.id.length <= 200, 'id is a usable idempotency key', r.id);
    assert(r.ts > 946_684_800 && r.ts < 4_102_444_800, 'ts is epoch SECONDS, not milliseconds', String(r.ts));
    assert(typeof r.source === 'string' && r.source !== '', 'source names the producer', r.source);
    assert(r.source === 'lighthouse-13.4.1', 'source carries the exact version', r.source);
  }
  const f = out.records.find((r) => r.data.audit === 'unused-css-rules')!;
  assert(f.data.run_id === out.runId, 'each finding points back at its run', String(f.data.run_id));
  assert((f.data.savings_bytes as number) > 0, 'byte savings are carried through', String(f.data.savings_bytes));
}

// 10. The Node-blocked ids are recorded but NOT allowlisted, so they can
//     never be the reason a run is permanently unusable.
{
  const overlap = BLOCKED_BY_NODE_VERSION.filter((id) => ALLOWED_IDS.includes(id));
  assert(
    overlap.length === 0,
    'an audit this Node cannot run is not on the allowlist',
    overlap.join(','),
  );
  assert(
    BLOCKED_BY_NODE_VERSION.every((id) => LHR.audits[id] === undefined || LHR.audits[id].scoreDisplayMode === 'error'),
    'and the fixture confirms they really do error here',
    BLOCKED_BY_NODE_VERSION.map((id) => `${id}=${LHR.audits[id]?.scoreDisplayMode}`).join(','),
  );
}

console.log('\n=== lighthouseAdapter.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} assertions passed.`);
