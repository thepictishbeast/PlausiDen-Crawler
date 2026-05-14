/**
 * scoreWhitelist.test.ts — accepted-risk filter tests. T76 cycle 35.
 */
import {
  whitelistPathFor,
  readWhitelist,
  applyWhitelist,
  renderWhitelistSummary,
  type WhitelistEntry,
  type ApplyEvent,
} from './scoreWhitelist.js';
import { mkdtempSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

const NOW = new Date('2026-05-14T12:00:00Z');
const FUTURE = '2027-01-01';
const PAST = '2026-01-01';

// 1. whitelistPathFor: bare name → journeys/<name>.whitelist.json.
{
  const p = whitelistPathFor('skillshots-poc');
  assert(p.endsWith('journeys/skillshots-poc.whitelist.json') || p.endsWith('journeys\\skillshots-poc.whitelist.json'), 'bare name → journeys/', p);
}

// 2. whitelistPathFor: full path → sibling .whitelist.json.
{
  const p = whitelistPathFor('journeys/skillshots-poc.json');
  assert(p === 'journeys/skillshots-poc.whitelist.json', 'full path → sibling', p);
}

// 3. whitelistPathFor: path with no .json → still appends .whitelist.json.
{
  const p = whitelistPathFor('some/dir/site');
  assert(p === 'some/dir/site.whitelist.json', 'no-suffix path', p);
}

// 4. readWhitelist: missing file → empty list.
{
  const dir = mkdtempSync(join(tmpdir(), 'wl-test-'));
  try {
    const r = readWhitelist(join(dir, 'no-such.json'));
    assert(r.length === 0, 'missing → empty', `len=${r.length}`);
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 5. readWhitelist: malformed JSON → empty + log.
{
  const dir = mkdtempSync(join(tmpdir(), 'wl-test-'));
  try {
    writeFileSync(join(dir, 'bad.whitelist.json'), 'not json at all');
    let logged = '';
    const r = readWhitelist(join(dir, 'bad.json'), (m) => { logged = m; });
    assert(r.length === 0, 'malformed → empty', `len=${r.length}`);
    assert(logged.includes("couldn't parse"), 'logged parse error', logged);
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 6. readWhitelist: not an array → empty + log.
{
  const dir = mkdtempSync(join(tmpdir(), 'wl-test-'));
  try {
    writeFileSync(join(dir, 'obj.whitelist.json'), '{"not": "an array"}');
    let logged = '';
    const r = readWhitelist(join(dir, 'obj.json'), (m) => { logged = m; });
    assert(r.length === 0, 'non-array → empty', `len=${r.length}`);
    assert(logged.includes('not a JSON array'), 'logged non-array', logged);
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 7. readWhitelist: valid entries parsed.
{
  const dir = mkdtempSync(join(tmpdir(), 'wl-test-'));
  try {
    writeFileSync(
      join(dir, 'good.whitelist.json'),
      JSON.stringify([
        { kind: 'tap-targets', ruleId: 'tap.too-small', reason: 'baseline' },
        { kind: 'info-leak' },  // wildcard
      ]),
    );
    const r = readWhitelist(join(dir, 'good.json'));
    assert(r.length === 2, 'two entries parsed', `len=${r.length}`);
    assert(r[0].kind === 'tap-targets' && r[0].ruleId === 'tap.too-small', 'first parsed', JSON.stringify(r[0]));
    assert(r[1].kind === 'info-leak' && r[1].ruleId === undefined, 'wildcard preserved', JSON.stringify(r[1]));
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 8. readWhitelist: entries missing 'kind' are skipped.
{
  const dir = mkdtempSync(join(tmpdir(), 'wl-test-'));
  try {
    writeFileSync(
      join(dir, 'bad-entry.whitelist.json'),
      JSON.stringify([
        { ruleId: 'no-kind' },           // missing kind
        { kind: 'tap-targets' },         // ok
        { kind: '', ruleId: 'empty' },   // empty kind
      ]),
    );
    let warnings = 0;
    const r = readWhitelist(join(dir, 'bad-entry.json'), () => { warnings += 1; });
    assert(r.length === 1, 'one valid entry kept', `len=${r.length}`);
    assert(warnings === 2, 'two skipped warnings', `warnings=${warnings}`);
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 9. applyWhitelist: exact kind+ruleId match suppresses.
{
  const events: ApplyEvent[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', severity: 'warn' },
    { kind: 'tap-targets', ruleId: 'tap.below-recommended', severity: 'warn' },
    { kind: 'csp-policy', ruleId: 'csp.missing', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small' },
  ];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.kept.length === 2, '2 kept (suppress 1)', `kept=${r.kept.length}`);
  assert(r.whitelisted.length === 1, '1 suppressed', `wl=${r.whitelisted.length}`);
  assert(r.whitelisted[0].ruleId === 'tap.too-small', 'right rule suppressed', JSON.stringify(r.whitelisted[0]));
}

// 10. applyWhitelist: wildcard (no ruleId) matches all of that kind.
{
  const events: ApplyEvent[] = [
    { kind: 'info-leak', ruleId: 'info-leak.server-version', severity: 'warn' },
    { kind: 'info-leak', ruleId: 'info-leak.x-powered-by', severity: 'warn' },
    { kind: 'csp-policy', ruleId: 'csp.missing', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [{ kind: 'info-leak' }];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.kept.length === 1, 'csp kept', `kept=${r.kept.length}`);
  assert(r.whitelisted.length === 2, 'both info-leak suppressed', `wl=${r.whitelisted.length}`);
}

// 11. applyWhitelist: expired entry is NOT applied.
{
  const events: ApplyEvent[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', until: PAST },
  ];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.kept.length === 1, 'expired entry doesnt suppress', `kept=${r.kept.length}`);
  assert(r.whitelisted.length === 0, 'no suppressions', `wl=${r.whitelisted.length}`);
  assert(r.expired.length === 1, 'expired list populated', `exp=${r.expired.length}`);
}

// 12. applyWhitelist: future expiry is active.
{
  const events: ApplyEvent[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', until: FUTURE },
  ];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.kept.length === 0, 'future expiry still suppresses', `kept=${r.kept.length}`);
  assert(r.whitelisted.length === 1, 'one suppression', `wl=${r.whitelisted.length}`);
  assert(r.expired.length === 0, 'no expired', `exp=${r.expired.length}`);
}

// 13. applyWhitelist: unused entries surfaced.
{
  const events: ApplyEvent[] = [
    { kind: 'csp-policy', ruleId: 'csp.missing', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small' },  // doesn't match
    { kind: 'csp-policy', ruleId: 'csp.missing' },     // matches
  ];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.unused.length === 1, 'one unused entry', `unused=${r.unused.length}`);
  assert(r.unused[0].kind === 'tap-targets', 'right unused entry', JSON.stringify(r.unused[0]));
}

// 14. applyWhitelist: malformed `until` doesn't crash, entry stays active.
{
  const events: ApplyEvent[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', until: 'totally not a date' },
  ];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.kept.length === 0, 'malformed until → still active', `kept=${r.kept.length}`);
  assert(r.whitelisted.length === 1, 'suppressed despite bad until', `wl=${r.whitelisted.length}`);
}

// 15. applyWhitelist: matchedEntry carries through (HTML report uses this).
{
  const events: ApplyEvent[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', severity: 'warn' },
  ];
  const wl: WhitelistEntry[] = [
    { kind: 'tap-targets', ruleId: 'tap.too-small', reason: 'baseline' },
  ];
  const r = applyWhitelist(events, wl, NOW);
  assert(r.whitelisted[0].matchedEntry.reason === 'baseline', 'reason threaded', JSON.stringify(r.whitelisted[0].matchedEntry));
}

// 16. renderWhitelistSummary: empty → friendly no-op message.
{
  const out = renderWhitelistSummary({ kept: [], whitelisted: [], unused: [], expired: [] });
  assert(out.includes('No whitelist file or no entries'), 'empty render', out);
}

// 17. renderWhitelistSummary: suppressed count.
{
  const out = renderWhitelistSummary({
    kept: [],
    whitelisted: [
      { kind: 'tap-targets', ruleId: 'tap.too-small', matchedEntry: { kind: 'tap-targets', ruleId: 'tap.too-small' } },
    ],
    unused: [],
    expired: [],
  });
  assert(out.includes('suppressed: 1'), 'suppressed count', out);
}

// 18. renderWhitelistSummary: expired entry surfaced for renewal.
{
  const out = renderWhitelistSummary({
    kept: [], whitelisted: [], unused: [],
    expired: [{ kind: 'tap-targets', ruleId: 'tap.too-small', until: '2026-01-01' }],
  });
  assert(out.includes('EXPIRED'), 'EXPIRED label', out);
  assert(out.includes('tap-targets'), 'kind shown', out);
}

console.log('\n=== scoreWhitelist.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
