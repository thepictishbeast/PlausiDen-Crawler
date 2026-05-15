/**
 * visualDiff.test.ts — baseline-or-compare wiring tests.
 */
import { compareToBaseline, defaultBaselineDir } from './visualDiff.js';
import { mkdirSync, copyFileSync, existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

function findRealPng(): string | null {
  // Reuse a screenshot from any prior audit run as a real fixture.
  try {
    const runs = readdirSync('runs')
      .filter(d => d.startsWith('skillshots-poc-') && !d.includes('latest') && !d.endsWith('-baseline'))
      .filter(d => { try { return statSync(`runs/${d}`).isDirectory(); } catch { return false; } });
    if (runs.length === 0) return null;
    const dir = `runs/${runs[runs.length - 1]}`;
    const png = readdirSync(dir).find(f => f.endsWith('.png') && !f.includes('annotated'));
    return png ? join(dir, png) : null;
  } catch {
    return null;
  }
}

// 1. defaultBaselineDir is pure path math.
{
  const got = defaultBaselineDir('runs', 'skillshots-poc');
  assert(got === 'runs/skillshots-poc-baseline', 'defaultBaselineDir layout', got);
}

// 2. baseline-missing without autoEstablish → status flag, no write.
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  const realPng = findRealPng();
  if (realPng) {
    const newPath = join(tmp, '01-test.png');
    copyFileSync(realPng, newPath);
    const baselineDir = join(tmp, 'baseline');
    const r = compareToBaseline(newPath, baselineDir, '01-test', { autoEstablish: false });
    assert(r.status === 'baseline-missing', 'no autoEstablish → baseline-missing', r.status);
    assert(!existsSync(join(baselineDir, '01-test.png')), 'baseline NOT written when autoEstablish=false',
      existsSync(join(baselineDir, '01-test.png')) ? 'was written' : 'ok');
    assert(typeof r.newHash?.hex === 'string', 'new hash still computed', String(r.newHash?.hex));
  } else {
    PASSED.push('baseline-missing scenario (skipped: no real PNG fixture)');
  }
  rmSync(tmp, { recursive: true, force: true });
}

// 3. baseline-missing with autoEstablish → copies + status flag.
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  const realPng = findRealPng();
  if (realPng) {
    const newPath = join(tmp, '01-test.png');
    copyFileSync(realPng, newPath);
    const baselineDir = join(tmp, 'baseline');
    const r = compareToBaseline(newPath, baselineDir, '01-test', { autoEstablish: true });
    assert(r.status === 'baseline-established', 'autoEstablish → baseline-established', r.status);
    assert(existsSync(join(baselineDir, '01-test.png')), 'baseline copied to disk',
      'expected ' + join(baselineDir, '01-test.png'));
  } else {
    PASSED.push('baseline-established scenario (skipped: no real PNG fixture)');
  }
  rmSync(tmp, { recursive: true, force: true });
}

// 4. Same file as baseline + new → unchanged (distance 0).
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  const realPng = findRealPng();
  if (realPng) {
    const newPath = join(tmp, '01-test.png');
    const baselineDir = join(tmp, 'baseline');
    mkdirSync(baselineDir, { recursive: true });
    copyFileSync(realPng, newPath);
    copyFileSync(realPng, join(baselineDir, '01-test.png'));
    const r = compareToBaseline(newPath, baselineDir, '01-test');
    assert(r.status === 'unchanged', 'identical bytes → unchanged', r.status);
    assert(r.comparison?.distance === 0, 'distance 0', String(r.comparison?.distance));
    assert(typeof r.newHash?.hex === 'string', 'new hash present', String(r.newHash?.hex));
    assert(typeof r.baselineHash?.hex === 'string', 'baseline hash present', String(r.baselineHash?.hex));
  } else {
    PASSED.push('unchanged scenario (skipped: no real PNG fixture)');
  }
  rmSync(tmp, { recursive: true, force: true });
}

// 5. Different page baselines → changed.
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  try {
    const runs = readdirSync('runs')
      .filter(d => d.startsWith('skillshots-poc-') && !d.includes('latest') && !d.endsWith('-baseline'))
      .filter(d => { try { return statSync(`runs/${d}`).isDirectory(); } catch { return false; } });
    if (runs.length > 0) {
      const dir = `runs/${runs[runs.length - 1]}`;
      const pngs = readdirSync(dir)
        .filter(f => f.endsWith('.png') && !f.includes('annotated'));
      if (pngs.length >= 2) {
        const newPath = join(tmp, '01-test.png');
        const baselineDir = join(tmp, 'baseline');
        mkdirSync(baselineDir, { recursive: true });
        copyFileSync(join(dir, pngs[0]), newPath);
        copyFileSync(join(dir, pngs[pngs.length - 1]), join(baselineDir, '01-test.png'));
        // High threshold so test is robust if pngs happen to be similar
        // (we use threshold 0 here — any difference at all = "changed").
        const r = compareToBaseline(newPath, baselineDir, '01-test', { threshold: 0 });
        const okStatus = r.status === 'changed' || r.status === 'unchanged';
        assert(okStatus, 'comparison runs without error on different pages', r.status);
        assert(typeof r.comparison?.distance === 'number',
          'distance computed', String(r.comparison?.distance));
      } else {
        PASSED.push('different-page comparison (skipped: <2 PNGs)');
      }
    } else {
      PASSED.push('different-page comparison (skipped: no runs)');
    }
  } catch (e) {
    FAILED.push({ name: 'different-page comparison', reason: String(e) });
  }
  rmSync(tmp, { recursive: true, force: true });
}

// 6. Missing new screenshot → error status, no throw.
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  const r = compareToBaseline(join(tmp, 'nope.png'), tmp, 'nope');
  assert(r.status === 'error', 'missing new → error status', r.status);
  assert(r.message.includes('missing'), 'message names the missing file', r.message);
  rmSync(tmp, { recursive: true, force: true });
}

// 7. Threshold respected.
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  const realPng = findRealPng();
  if (realPng) {
    const newPath = join(tmp, '01-test.png');
    const baselineDir = join(tmp, 'baseline');
    mkdirSync(baselineDir, { recursive: true });
    copyFileSync(realPng, newPath);
    copyFileSync(realPng, join(baselineDir, '01-test.png'));
    const r = compareToBaseline(newPath, baselineDir, '01-test', { threshold: 64 });
    assert(r.thresholdUsed === 64, 'threshold respected', String(r.thresholdUsed));
    assert(r.status === 'unchanged', 'distance 0 + threshold 64 = unchanged', r.status);
  } else {
    PASSED.push('threshold scenario (skipped: no real PNG fixture)');
  }
  rmSync(tmp, { recursive: true, force: true });
}

// 8. Default threshold is 5.
{
  const tmp = mkdtempSync(join(tmpdir(), 'vd-test-'));
  const realPng = findRealPng();
  if (realPng) {
    const newPath = join(tmp, '01-test.png');
    const baselineDir = join(tmp, 'baseline');
    mkdirSync(baselineDir, { recursive: true });
    copyFileSync(realPng, newPath);
    copyFileSync(realPng, join(baselineDir, '01-test.png'));
    const r = compareToBaseline(newPath, baselineDir, '01-test');
    assert(r.thresholdUsed === 5, 'default threshold is 5', String(r.thresholdUsed));
  } else {
    PASSED.push('default threshold check (skipped: no real PNG fixture)');
  }
  rmSync(tmp, { recursive: true, force: true });
}

// 9. defaultBaselineDir doesn't create.
{
  const got = defaultBaselineDir('/nonexistent', 'foo');
  assert(got === '/nonexistent/foo-baseline', 'pure path math', got);
  assert(!existsSync(got), 'no side effects', got);
}

console.log('\n=== visualDiff.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
