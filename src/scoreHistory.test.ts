/**
 * scoreHistory.test.ts — persistent score-trend tests. T76 cycle 33.
 */
import {
  buildScoreHistoryEntry,
  readScoreHistory,
  appendScoreHistoryEntry,
  detectScoreRegression,
  renderScoreRegression,
  type ScoreHistoryEntry,
} from './scoreHistory.js';
import type { SupersocietyScore } from './supersocietyScore.js';
import { mkdtempSync, rmSync, appendFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

function makeScore(opts: {
  composite?: number;
  grade?: string;
  totalStrict?: number;
  totalWarn?: number;
  categories?: Array<{ category: string; score?: number; grade?: string; strict?: number; warn?: number }>;
}): SupersocietyScore {
  const categories = (opts.categories ?? []).map((c) => ({
    category: c.category,
    weight: 1.0,
    score: c.score ?? 100,
    grade: c.grade ?? 'A',
    strict: c.strict ?? 0,
    warn: c.warn ?? 0,
    contributingKinds: [],
  }));
  return {
    composite: opts.composite ?? 100,
    grade: opts.grade ?? 'A',
    categories,
    totalStrict: opts.totalStrict ?? 0,
    totalWarn: opts.totalWarn ?? 0,
    unbucketed: [],
    headline: '',
  };
}

function mkTmp(): string {
  return mkdtempSync(join(tmpdir(), 'score-history-test-'));
}

// 1. buildScoreHistoryEntry timestamp + journey populate correctly.
{
  const s = makeScore({ composite: 87, grade: 'B', totalStrict: 1, totalWarn: 3 });
  const e = buildScoreHistoryEntry(s, { journey: 'test', timestamp: '2026-05-14T00:00:00Z', commit: 'abc' });
  assert(e.timestamp === '2026-05-14T00:00:00Z', 'timestamp preserved', e.timestamp);
  assert(e.journey === 'test', 'journey preserved', e.journey);
  assert(e.composite === 87 && e.grade === 'B', 'composite + grade preserved', JSON.stringify(e));
  assert(e.commit === 'abc', 'commit preserved', e.commit ?? '');
}

// 2. buildScoreHistoryEntry timestamp defaults to now-ish.
{
  const s = makeScore({});
  const e = buildScoreHistoryEntry(s, { journey: 'test' });
  // ISO 8601 prefix is YYYY-MM-DD
  assert(/^\d{4}-\d{2}-\d{2}T/.test(e.timestamp), 'timestamp ISO 8601', e.timestamp);
}

// 3. readScoreHistory returns empty when file missing.
{
  const dir = mkTmp();
  try {
    const h = readScoreHistory(dir, 'no-such-journey');
    assert(h.length === 0, 'missing file → empty array', JSON.stringify(h));
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 4. append + read round-trip.
{
  const dir = mkTmp();
  try {
    const s = makeScore({ composite: 92 });
    const e = buildScoreHistoryEntry(s, { journey: 'test', timestamp: '2026-05-14T00:00:00Z' });
    appendScoreHistoryEntry(dir, e);
    const h = readScoreHistory(dir, 'test');
    assert(h.length === 1 && h[0].composite === 92, 'one entry round-tripped', JSON.stringify(h));
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 5. Multiple appends preserve order (oldest first).
{
  const dir = mkTmp();
  try {
    for (let i = 0; i < 3; i++) {
      const e = buildScoreHistoryEntry(
        makeScore({ composite: 80 + i }),
        { journey: 'test', timestamp: `2026-05-14T0${i}:00:00Z` },
      );
      appendScoreHistoryEntry(dir, e);
    }
    const h = readScoreHistory(dir, 'test');
    assert(h.length === 3, '3 entries', JSON.stringify(h.length));
    assert(h[0].composite === 80 && h[2].composite === 82, 'order preserved oldest-first', JSON.stringify(h.map((x) => x.composite)));
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 6. Malformed line skipped.
{
  const dir = mkTmp();
  try {
    const e = buildScoreHistoryEntry(makeScore({}), { journey: 'test' });
    appendScoreHistoryEntry(dir, e);
    // Manually append a malformed line.
    const path = join(dir, 'test-score-history.jsonl');
    appendFileSync(path, 'this is not json\n');
    const e2 = buildScoreHistoryEntry(makeScore({ composite: 88 }), { journey: 'test' });
    appendScoreHistoryEntry(dir, e2);
    const h = readScoreHistory(dir, 'test');
    assert(h.length === 2, 'malformed line skipped, valid retained', JSON.stringify(h.length));
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 7. Journey slug sanitisation.
{
  const dir = mkTmp();
  try {
    const e = buildScoreHistoryEntry(makeScore({}), { journey: 'journeys/skillshots-poc.json' });
    appendScoreHistoryEntry(dir, e);
    // Either full path or bare name should resolve to same file.
    const h1 = readScoreHistory(dir, 'journeys/skillshots-poc.json');
    const h2 = readScoreHistory(dir, 'skillshots-poc');
    assert(h1.length === 1, 'full-path read finds entry', JSON.stringify(h1));
    assert(h2.length === 1, 'bare-name read finds same entry', JSON.stringify(h2));
  } finally {
    rmSync(dir, { recursive: true });
  }
}

// 8. detectScoreRegression: no prior history → no regression.
{
  const e = buildScoreHistoryEntry(makeScore({ composite: 75 }), { journey: 'test' });
  const r = detectScoreRegression(e, []);
  assert(!r.hasRegression, 'empty history → no regression', JSON.stringify(r));
  assert(r.headline.includes('First-ever'), 'first-ever headline', r.headline);
}

// 9. detectScoreRegression: composite improved → no regression.
{
  const prior = buildScoreHistoryEntry(makeScore({ composite: 80, grade: 'B' }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({ composite: 90, grade: 'A' }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(!r.hasRegression, 'improvement → no regression', JSON.stringify(r));
  assert(r.compositeDelta === 10, 'composite delta +10', `${r.compositeDelta}`);
  assert(r.headline.includes('improved'), 'improved headline', r.headline);
}

// 10. detectScoreRegression: composite dropped 5+ → regression.
{
  const prior = buildScoreHistoryEntry(makeScore({ composite: 95, grade: 'A' }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({ composite: 80, grade: 'B' }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(r.hasRegression, 'composite -15 → regression', JSON.stringify(r));
  assert(r.overallGradeDropped, 'grade dropped flag', `${r.overallGradeDropped}`);
}

// 11. detectScoreRegression: small composite drop (-3) → no regression unless category regresses.
{
  const prior = buildScoreHistoryEntry(makeScore({ composite: 95, grade: 'A' }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({ composite: 92, grade: 'A' }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(!r.hasRegression, 'composite -3 → no regression', JSON.stringify(r));
}

// 12. Category regression: score drop ≥10 → flagged.
{
  const prior = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    categories: [{ category: 'transportSecurity', score: 100, grade: 'A' }],
  }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',  // composite still high
    categories: [{ category: 'transportSecurity', score: 75, grade: 'C', strict: 1 }],
  }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(r.hasRegression, 'category score drop ≥10 → regression', JSON.stringify(r));
  assert(r.categoryRegressions.length === 1, 'one category flagged', JSON.stringify(r.categoryRegressions));
  assert(r.categoryRegressions[0].category === 'transportSecurity', 'right category', r.categoryRegressions[0].category);
}

// 13. Category grade drop alone → flagged.
{
  const prior = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    categories: [{ category: 'transportSecurity', score: 85, grade: 'B' }],
  }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    // Score dropped only 8 (below 10 threshold) but grade dropped B→C.
    categories: [{ category: 'transportSecurity', score: 77, grade: 'C', warn: 5 }],
  }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(r.hasRegression, 'grade drop alone → regression', JSON.stringify(r));
  assert(r.categoryRegressions[0].gradeDropped, 'gradeDropped flag', `${r.categoryRegressions[0].gradeDropped}`);
}

// 14. New strict finding alone → flagged.
{
  const prior = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    categories: [{ category: 'transportSecurity', score: 100, grade: 'A', strict: 0 }],
  }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    // Score stayed (somehow), but strict went 0→1.
    categories: [{ category: 'transportSecurity', score: 100, grade: 'A', strict: 1 }],
  }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(r.hasRegression, 'new strict → regression', JSON.stringify(r));
  assert(r.categoryRegressions[0].newStrict, 'newStrict flag', `${r.categoryRegressions[0].newStrict}`);
}

// 15. Regressions sorted worst-first.
{
  const prior = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    categories: [
      { category: 'a', score: 100, grade: 'A' },
      { category: 'b', score: 100, grade: 'A' },
    ],
  }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({
    composite: 60, grade: 'D',
    categories: [
      { category: 'a', score: 50, grade: 'F', strict: 2 },  // delta -50
      { category: 'b', score: 80, grade: 'B', warn: 4 },    // delta -20
    ],
  }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  assert(r.categoryRegressions[0].category === 'a', 'worst regression first', r.categoryRegressions[0].category);
  assert(r.categoryRegressions[1].category === 'b', 'second-worst second', r.categoryRegressions[1].category);
}

// 16. renderScoreRegression renders the regressions.
{
  const prior = buildScoreHistoryEntry(makeScore({
    composite: 95, grade: 'A',
    categories: [{ category: 'transportSecurity', score: 100, grade: 'A' }],
  }), { journey: 'test' });
  const cur = buildScoreHistoryEntry(makeScore({
    composite: 75, grade: 'C',
    categories: [{ category: 'transportSecurity', score: 50, grade: 'F', strict: 2 }],
  }), { journey: 'test' });
  const r = detectScoreRegression(cur, [prior]);
  const out = renderScoreRegression(r);
  assert(out.includes('REGRESSION'), 'render mentions REGRESSION', out);
  assert(out.includes('transportSecurity'), 'render lists category', out);
}

console.log('\n=== scoreHistory.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
