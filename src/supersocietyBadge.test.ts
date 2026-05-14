/**
 * supersocietyBadge.test.ts — SVG badge renderer tests. T76 cycle 36.
 */
import { renderSupersocietyBadge } from './supersocietyBadge.js';
import type { SupersocietyScore } from './supersocietyScore.js';

const PASSED: string[] = [];
const FAILED: { name: string; reason: string }[] = [];
const assert = (c: boolean, name: string, reason: string) =>
  c ? PASSED.push(name) : FAILED.push({ name, reason });

function mkScore(composite: number, grade: string): SupersocietyScore {
  return {
    composite,
    grade,
    categories: [],
    totalStrict: 0,
    totalWarn: 0,
    unbucketed: [],
    headline: '',
  };
}

// 1. Output is valid-looking SVG.
{
  const svg = renderSupersocietyBadge(mkScore(97, 'A'));
  assert(svg.includes('<svg'), 'starts with <svg', svg.slice(0, 80));
  assert(svg.includes('</svg>'), 'closes with </svg>', '');
}

// 2. Grade and score appear in the SVG.
{
  const svg = renderSupersocietyBadge(mkScore(97, 'A'));
  assert(svg.includes('A 97/100'), 'grade + score visible', '');
}

// 3. Each grade maps to its expected colour.
{
  const cases: Array<[string, string]> = [
    ['A', '#22c55e'],
    ['B', '#84cc16'],
    ['C', '#facc15'],
    ['D', '#fb923c'],
    ['F', '#ef4444'],
  ];
  for (const [grade, color] of cases) {
    const svg = renderSupersocietyBadge(mkScore(50, grade));
    assert(svg.includes(color), `${grade} → ${color}`, '');
  }
}

// 4. Invalid grade falls back to muted colour.
{
  const svg = renderSupersocietyBadge(mkScore(50, 'X'));
  // The muted fallback colour is in the SVG.
  assert(svg.includes('#9990bb'), 'unknown grade → muted', '');
}

// 5. aria-label is present and meaningful.
{
  const svg = renderSupersocietyBadge(mkScore(73, 'C'));
  assert(
    svg.includes('aria-label="Supersociety Score: grade C, 73 out of 100"'),
    'aria-label populated',
    '',
  );
}

// 6. Title element matches aria-label.
{
  const svg = renderSupersocietyBadge(mkScore(73, 'C'));
  assert(
    svg.includes('<title>Supersociety Score: grade C, 73 out of 100</title>'),
    '<title> populated',
    '',
  );
}

// 7. Fixed-width 140 px.
{
  const svg = renderSupersocietyBadge(mkScore(100, 'A'));
  assert(svg.includes('width="140"') && svg.includes('height="20"'), '140×20 dimensions', '');
}

// 8. No external CSS / JS / images.
{
  const svg = renderSupersocietyBadge(mkScore(80, 'B'));
  assert(!/<link\s+[^>]*href=/i.test(svg), 'no <link>', '');
  assert(!/<script/i.test(svg), 'no <script>', '');
  assert(!/<img/i.test(svg), 'no <img>', '');
  assert(!/url\(["']?http/i.test(svg), 'no external url()', '');
}

// 9. HTML-escape protects against XSS via grade (even though gradeColor
// constrains to a fixed set, the value text passes through esc).
{
  // Synthesise a malicious score (TypeScript won't normally let us pass
  // arbitrary strings as grade, but downstream callers could).
  const svg = renderSupersocietyBadge({
    composite: 99,
    grade: '<script>alert(1)</script>',
    categories: [],
    totalStrict: 0,
    totalWarn: 0,
    unbucketed: [],
    headline: '',
  });
  assert(!svg.includes('<script>alert(1)</script>'), 'XSS grade not raw', '');
  assert(svg.includes('&lt;script&gt;'), 'XSS grade escaped', '');
}

// 10. Composite number renders as plain digits (no exponent / etc).
{
  const svg = renderSupersocietyBadge(mkScore(0, 'F'));
  assert(svg.includes('F 0/100'), 'composite 0', '');
}
{
  const svg = renderSupersocietyBadge(mkScore(100, 'A'));
  assert(svg.includes('A 100/100'), 'composite 100', '');
}

console.log('\n=== supersocietyBadge.test.ts ===');
console.log(`PASSED ${PASSED.length}:`);
PASSED.forEach((p) => console.log(`  ✓ ${p}`));
if (FAILED.length > 0) {
  console.log(`FAILED ${FAILED.length}:`);
  FAILED.forEach((f) => console.log(`  ✗ ${f.name}: ${f.reason}`));
  process.exit(1);
}
console.log(`All ${PASSED.length} scenarios passed.`);
