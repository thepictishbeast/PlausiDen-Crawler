/**
 * htmlReport.test.ts — single-file HTML report renderer tests.
 * T76 cycle 34.
 */
import { renderHtmlReport } from './htmlReport.js';
import type { SupersocietyScore } from './supersocietyScore.js';
import type { ScoreHistoryEntry, ScoreRegression } from './scoreHistory.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

function mkScore(
  composite: number,
  grade: string,
  totalStrict = 0,
  totalWarn = 0,
): SupersocietyScore {
  return {
    composite,
    grade,
    categories: [
      { category: 'transportSecurity', weight: 2, score: 100, grade: 'A', strict: 0, warn: 0, contributingKinds: [] },
      { category: 'accessibility', weight: 1.5, score: 65, grade: 'D', strict: 0, warn: 7, contributingKinds: ['tap-targets'] },
    ],
    totalStrict,
    totalWarn,
    unbucketed: [],
    headline: `Grade ${grade} (${composite}/100).`,
  };
}

function mkHistory(scores: number[]): ScoreHistoryEntry[] {
  return scores.map((s, i) => ({
    timestamp: `2026-05-14T0${i}:00:00Z`,
    journey: 'test',
    composite: s,
    grade: 'A',
    totalStrict: 0,
    totalWarn: 0,
    categories: [],
  }));
}

const stableRegression: ScoreRegression = {
  hasRegression: false,
  compositeDelta: 0,
  overallGradeDropped: false,
  categoryRegressions: [],
  headline: 'Score stable at 97 (Δ0). No regressions.',
};

// 1. Output starts with the doctype.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A', 0, 7),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(html.startsWith('<!doctype html>'), 'doctype', html.slice(0, 32));
}

// 2. Composite score + grade appear in the page.
{
  const html = renderHtmlReport({
    score: mkScore(72, 'C', 1, 4),
    history: mkHistory([72]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(html.includes('>72<'), 'composite 72 present', '');
  assert(html.includes('>C<'), 'grade C present', '');
}

// 3. HTML-escaping protects against journey-name XSS.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A'),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: '<script>alert(1)</script>',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(!html.includes('<script>alert(1)</script>'), 'XSS journey not rendered', '');
  assert(html.includes('&lt;script&gt;alert(1)&lt;/script&gt;'), 'XSS journey escaped', '');
}

// 4. Empty history → first-run badge.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A'),
    history: [],
    regression: {
      hasRegression: false,
      compositeDelta: 0,
      overallGradeDropped: false,
      categoryRegressions: [],
      headline: 'First-ever run for this journey — no prior to compare.',
    },
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(html.includes('First run for this journey'), 'first-run badge', '');
  assert(html.includes('No prior runs'), 'empty-trend SVG content', '');
}

// 5. Regression badge shows on a real regression.
{
  const html = renderHtmlReport({
    score: mkScore(75, 'C', 2, 3),
    history: mkHistory([95, 75]),
    regression: {
      hasRegression: true,
      compositeDelta: -20,
      overallGradeDropped: true,
      categoryRegressions: [{
        category: 'transportSecurity',
        priorScore: 100,
        currentScore: 50,
        delta: -50,
        gradeDropped: true,
        newStrict: true,
        message: 'transportSecurity: score 100→50 (-50), grade A→F, +2 strict',
      }],
      headline: 'REGRESSION: composite 95→75 (-20, grade A→C), 1 category regression(s).',
    },
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(html.includes('REGRESSION'), 'regression badge', '');
  assert(html.includes('transportSecurity'), 'category in list', '');
}

// 6. Trend chart includes SVG <circle> per history entry.
{
  const html = renderHtmlReport({
    score: mkScore(85, 'B'),
    history: mkHistory([80, 82, 85, 90, 85]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  const circles = (html.match(/<circle /g) ?? []).length;
  assert(circles === 5, '5 circles for 5 entries', `circles=${circles}`);
}

// 7. Category bars rendered for each category.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A'),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(html.includes('transportSecurity'), 'category label rendered', '');
  assert(html.includes('accessibility'), 'second category label rendered', '');
}

// 8. Findings table rows only for categories with findings.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A', 0, 7),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  // transportSecurity has 0 findings → no row. accessibility has 7 → 1 row.
  assert(html.includes('<table class="findings">'), 'findings table present', '');
  assert(html.includes('tap-targets'), 'contributing kind shown', '');
}

// 9. No findings → empty-state message.
{
  const score: SupersocietyScore = {
    composite: 100, grade: 'A',
    categories: [
      { category: 'transportSecurity', weight: 2, score: 100, grade: 'A', strict: 0, warn: 0, contributingKinds: [] },
    ],
    totalStrict: 0, totalWarn: 0,
    unbucketed: [],
    headline: 'Grade A (100/100). Supersociety baseline met.',
  };
  const html = renderHtmlReport({
    score,
    history: mkHistory([100]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  assert(html.includes('Clean run'), 'empty-findings state', '');
}

// 10. Commit footer when commit is provided.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A'),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
    commit: 'abc123def',
  });
  assert(html.includes('abc123def'), 'commit rendered', '');
}

// 11. No commit → no commit code block.
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A'),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  // Footer should have journey + timestamp but no third <code>.
  const footerMatch = html.match(/<footer>([\s\S]*?)<\/footer>/);
  assert(!!footerMatch, 'footer present', '');
  if (footerMatch) {
    const codeCount = (footerMatch[1].match(/<code>/g) ?? []).length;
    assert(codeCount === 2, 'footer has 2 <code> blocks (no commit)', `codes=${codeCount}`);
  }
}

// 12. Output is a single self-contained HTML doc (no external refs).
{
  const html = renderHtmlReport({
    score: mkScore(97, 'A'),
    history: mkHistory([97]),
    regression: stableRegression,
    journey: 'test',
    timestamp: '2026-05-14T00:00:00Z',
  });
  // No external CSS / JS / image references — sanity check.
  assert(!/<link\s+[^>]*href=/i.test(html), 'no <link> tags', '');
  assert(!/<script\s+[^>]*src=/i.test(html), 'no external <script src>', '');
  assert(!/<img\s+[^>]*src=/i.test(html), 'no <img>', '');
}

console.log('\n=== htmlReport.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
