/**
 * htmlReport.ts — single-file self-contained HTML report
 * renderer. T76 cycle 34.
 *
 * Generates one HTML file per audit run that operators can
 * open in any browser to see the Supersociety Score + trend
 * + per-category breakdown + regressions + finding totals.
 *
 * Design philosophy: supersociety frontend stack.
 *
 *   - Single file. No external CSS, no external JS, no
 *     images, no font links. The whole report is one HTML
 *     blob — load it from disk, email it as an attachment,
 *     check it into git. It works offline, works in any
 *     browser, works in 2030, works in a 1990s-style HTML
 *     viewer with most features intact.
 *
 *   - Inline SVG. Charts are vanilla SVG <line>, <rect>,
 *     <text> elements. No D3, no Chart.js, no anything.
 *     Auto-scales from 1 trend entry to 1000.
 *
 *   - Zero supply-chain attack surface. Every byte is
 *     emitted by this module. No CDN, no npm dep that could
 *     get hijacked.
 *
 *   - Zero JS-framework lock-in. The HTML works without
 *     JavaScript at all. If we later add interactivity, we
 *     do it via vanilla DOM in a tiny inline <script>.
 *
 *   - Minimal CSS in one <style> tag. Variables for the
 *     palette so dark-mode is a 4-line addition later.
 *
 * Inputs:
 *   - The current SupersocietyScore.
 *   - The full ScoreHistory (for the trend chart).
 *   - The ScoreRegression (for the prior-run delta block).
 *   - Some context: journey name, run timestamp, optional
 *     commit SHA.
 *
 * Output: a string. Writer in main.ts persists to
 * runs/<run-dir>/supersociety-report.html.
 *
 * The report is HUMAN-facing. JSON dumps continue to exist
 * for machine consumers.
 *
 * REGRESSION-GUARD: All input strings are HTML-escaped via
 * `esc()` before interpolation. Operators may run this on
 * untrusted journey names / commit SHAs without XSS risk.
 */

import type {
  SupersocietyScore,
  SupersocietyCategoryScore,
} from './supersocietyScore.js';
import type {
  ScoreHistoryEntry,
  ScoreRegression,
  CategoryRegression,
} from './scoreHistory.js';

export interface HtmlReportInputs {
  score: SupersocietyScore;
  history: ScoreHistoryEntry[];
  regression: ScoreRegression;
  journey: string;
  timestamp: string;
  commit?: string;
}

// ----- HTML escaping -----

function esc(s: unknown): string {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// ----- Colour palette -----

const PALETTE = {
  // Background gradient ends.
  bgFrom: '#0e0a1a',
  bgTo: '#1a1530',
  // Card / surface.
  surface: '#1d1733',
  surfaceMuted: '#252040',
  // Text.
  text: '#e6e0ff',
  textMuted: '#9990bb',
  // Accents.
  accent: '#a78bfa',  // soft violet — matches PlausiDen premium palette
  accentMuted: '#7c6dff',
  // Grade colours.
  gradeA: '#22c55e',  // emerald
  gradeB: '#84cc16',  // lime
  gradeC: '#facc15',  // amber
  gradeD: '#fb923c',  // orange
  gradeF: '#ef4444',  // red
  // Strict / warn.
  strict: '#ef4444',
  warn: '#facc15',
  ok: '#22c55e',
};

function gradeColor(grade: string): string {
  switch (grade) {
    case 'A': return PALETTE.gradeA;
    case 'B': return PALETTE.gradeB;
    case 'C': return PALETTE.gradeC;
    case 'D': return PALETTE.gradeD;
    case 'F': return PALETTE.gradeF;
    default: return PALETTE.textMuted;
  }
}

// ----- SVG charts -----

/**
 * Compose an SVG line chart of composite scores over time.
 * X-axis: entry index (treated as discrete time steps).
 * Y-axis: 0..100. Auto-scales the X but keeps Y fixed to
 * the score domain. Grade-band horizontal lines at 90/80/70/60.
 */
function renderTrendChart(history: ScoreHistoryEntry[]): string {
  const W = 800;
  const H = 220;
  const PAD_L = 40;
  const PAD_R = 16;
  const PAD_T = 16;
  const PAD_B = 32;
  const innerW = W - PAD_L - PAD_R;
  const innerH = H - PAD_T - PAD_B;

  const n = history.length;
  if (n === 0) {
    return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="trend chart (empty)">
  <rect x="0" y="0" width="${W}" height="${H}" fill="${PALETTE.surfaceMuted}" rx="8"/>
  <text x="${W / 2}" y="${H / 2}" text-anchor="middle" fill="${PALETTE.textMuted}" font-size="14" font-family="system-ui">
    No prior runs — this is the first audit.
  </text>
</svg>`;
  }

  // Map each entry to an (x, y) pair.
  function xAt(i: number): number {
    if (n === 1) return PAD_L + innerW / 2;
    return PAD_L + (i / (n - 1)) * innerW;
  }
  function yAt(score: number): number {
    const clamped = Math.max(0, Math.min(100, score));
    return PAD_T + (1 - clamped / 100) * innerH;
  }

  // Grade band lines (90, 80, 70, 60).
  const bands = [
    { y: 90, grade: 'A', color: PALETTE.gradeA },
    { y: 80, grade: 'B', color: PALETTE.gradeB },
    { y: 70, grade: 'C', color: PALETTE.gradeC },
    { y: 60, grade: 'D', color: PALETTE.gradeD },
  ];
  const bandLines = bands
    .map((b) => {
      const y = yAt(b.y).toFixed(1);
      return `<line x1="${PAD_L}" y1="${y}" x2="${W - PAD_R}" y2="${y}" stroke="${b.color}" stroke-opacity="0.25" stroke-dasharray="3,3" stroke-width="1"/>
  <text x="${W - PAD_R - 2}" y="${y}" text-anchor="end" dominant-baseline="middle" fill="${b.color}" font-size="10" font-family="system-ui" opacity="0.6">${b.grade}≥${b.y}</text>`;
    })
    .join('\n  ');

  // Line path through composite scores.
  const path = history
    .map((h, i) => `${i === 0 ? 'M' : 'L'} ${xAt(i).toFixed(1)} ${yAt(h.composite).toFixed(1)}`)
    .join(' ');

  // Points + grade-coloured dots.
  const dots = history
    .map((h, i) => {
      const cx = xAt(i).toFixed(1);
      const cy = yAt(h.composite).toFixed(1);
      const c = gradeColor(h.grade);
      const ts = esc(h.timestamp);
      return `<circle cx="${cx}" cy="${cy}" r="4" fill="${c}" stroke="${PALETTE.surface}" stroke-width="1.5">
    <title>${ts}: ${h.composite}/100 (${h.grade})</title>
  </circle>`;
    })
    .join('\n  ');

  // Y-axis labels.
  const yLabels = [0, 25, 50, 75, 100]
    .map(
      (v) => `<text x="${PAD_L - 6}" y="${yAt(v).toFixed(1)}" text-anchor="end" dominant-baseline="middle" fill="${PALETTE.textMuted}" font-size="10" font-family="system-ui">${v}</text>`,
    )
    .join('\n  ');

  // X-axis: "1" and "n" labels at endpoints.
  const xLabels = n === 1
    ? `<text x="${xAt(0).toFixed(1)}" y="${H - PAD_B + 14}" text-anchor="middle" fill="${PALETTE.textMuted}" font-size="10" font-family="system-ui">1</text>`
    : `<text x="${xAt(0).toFixed(1)}" y="${H - PAD_B + 14}" text-anchor="middle" fill="${PALETTE.textMuted}" font-size="10" font-family="system-ui">1</text>
  <text x="${xAt(n - 1).toFixed(1)}" y="${H - PAD_B + 14}" text-anchor="middle" fill="${PALETTE.textMuted}" font-size="10" font-family="system-ui">${n}</text>`;

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="composite score trend across ${n} run(s)">
  <rect x="0" y="0" width="${W}" height="${H}" fill="${PALETTE.surfaceMuted}" rx="8"/>
  ${bandLines}
  <path d="${path}" fill="none" stroke="${PALETTE.accent}" stroke-width="2"/>
  ${dots}
  ${yLabels}
  ${xLabels}
  <text x="${PAD_L}" y="${H - 4}" fill="${PALETTE.textMuted}" font-size="10" font-family="system-ui">run #</text>
  <text x="${PAD_L - 6}" y="${PAD_T - 2}" text-anchor="end" fill="${PALETTE.textMuted}" font-size="10" font-family="system-ui">score</text>
</svg>`;
}

/**
 * Per-category horizontal bar chart. Each category is a row;
 * bar fills 0..100 in the category's grade colour.
 */
function renderCategoryBars(categories: SupersocietyCategoryScore[]): string {
  const W = 800;
  const ROW_H = 30;
  const H = categories.length * ROW_H + 16;
  const LABEL_W = 200;
  const PAD_L = 16;
  const PAD_R = 16;
  const barX = PAD_L + LABEL_W;
  const barMaxW = W - barX - PAD_R - 80;  // leave room for the value text

  const rows = categories
    .map((c, i) => {
      const y = 8 + i * ROW_H;
      const barY = y + 8;
      const barH = ROW_H - 16;
      const barW = (c.score / 100) * barMaxW;
      const color = gradeColor(c.grade);
      return `<g>
    <text x="${barX - 8}" y="${y + ROW_H / 2}" text-anchor="end" dominant-baseline="middle" fill="${PALETTE.text}" font-size="12" font-family="system-ui">${esc(c.category)}</text>
    <rect x="${barX}" y="${barY}" width="${barMaxW}" height="${barH}" fill="${PALETTE.surface}" rx="3"/>
    <rect x="${barX}" y="${barY}" width="${barW.toFixed(1)}" height="${barH}" fill="${color}" rx="3"/>
    <text x="${barX + barMaxW + 8}" y="${y + ROW_H / 2}" dominant-baseline="middle" fill="${PALETTE.text}" font-size="12" font-family="system-ui">${c.score}/100 ${c.grade}</text>
  </g>`;
    })
    .join('\n  ');

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="per-category score breakdown">
  ${rows}
</svg>`;
}

// ----- HTML composition -----

function renderRegressionSection(r: ScoreRegression): string {
  if (!r.hasRegression && r.compositeDelta === 0 && r.categoryRegressions.length === 0) {
    if (r.headline.includes('First-ever')) {
      return `<div class="regression first-run">
  <strong>First run for this journey</strong> — nothing to compare against yet. Subsequent runs will show deltas.
</div>`;
    }
    return `<div class="regression stable">
  <strong>Score stable</strong> — no regressions vs prior run. ${esc(r.headline)}
</div>`;
  }
  if (!r.hasRegression && r.compositeDelta > 0) {
    return `<div class="regression improved">
  <strong>Score improved (Δ+${r.compositeDelta})</strong> — ${esc(r.headline)}
</div>`;
  }
  const list = r.categoryRegressions
    .map((c: CategoryRegression) => `<li>${esc(c.message)}</li>`)
    .join('\n      ');
  return `<div class="regression regressed">
  <strong>REGRESSION</strong> — ${esc(r.headline)}
  ${r.categoryRegressions.length > 0
      ? `<ul>\n      ${list}\n    </ul>`
      : ''}
</div>`;
}

function renderFindingTable(categories: SupersocietyCategoryScore[]): string {
  const rows = categories
    .filter((c) => c.strict > 0 || c.warn > 0)
    .map(
      (c) => `<tr>
    <td>${esc(c.category)}</td>
    <td class="num">${c.score}</td>
    <td class="grade grade-${esc(c.grade)}">${esc(c.grade)}</td>
    <td class="num strict">${c.strict}</td>
    <td class="num warn">${c.warn}</td>
    <td class="kinds">${c.contributingKinds.map(esc).join(', ')}</td>
  </tr>`,
    )
    .join('\n  ');
  if (!rows) {
    return `<div class="findings-empty">No findings across any category. Clean run.</div>`;
  }
  return `<table class="findings">
  <thead>
    <tr><th>category</th><th>score</th><th>grade</th><th>strict</th><th>warn</th><th>contributing kinds</th></tr>
  </thead>
  <tbody>
  ${rows}
  </tbody>
</table>`;
}

export function renderHtmlReport(inputs: HtmlReportInputs): string {
  const { score, history, regression, journey, timestamp, commit } = inputs;
  const headline = esc(score.headline);
  const trendChart = renderTrendChart(history);
  const categoryBars = renderCategoryBars(score.categories);
  const regressionSection = renderRegressionSection(regression);
  const findingTable = renderFindingTable(score.categories);

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Supersociety Score — ${esc(journey)} — ${esc(timestamp)}</title>
<style>
  :root {
    color-scheme: dark;
    --bg-from: ${PALETTE.bgFrom};
    --bg-to: ${PALETTE.bgTo};
    --surface: ${PALETTE.surface};
    --text: ${PALETTE.text};
    --text-muted: ${PALETTE.textMuted};
    --accent: ${PALETTE.accent};
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body {
    font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
    background: linear-gradient(135deg, var(--bg-from), var(--bg-to)) fixed;
    color: var(--text);
    line-height: 1.5;
    min-height: 100vh;
  }
  main {
    max-width: 920px;
    margin: 0 auto;
    padding: 32px 24px 80px;
  }
  header {
    margin-bottom: 32px;
  }
  h1 {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }
  .composite {
    display: flex;
    align-items: baseline;
    gap: 16px;
    margin-top: 8px;
  }
  .composite-number {
    font-size: 72px;
    font-weight: 800;
    color: ${gradeColor(score.grade)};
    line-height: 1;
  }
  .composite-grade {
    font-size: 48px;
    font-weight: 700;
    color: ${gradeColor(score.grade)};
    line-height: 1;
  }
  .composite-out-of {
    font-size: 18px;
    color: var(--text-muted);
  }
  .headline {
    margin-top: 12px;
    color: var(--text-muted);
    font-size: 14px;
    max-width: 60ch;
  }
  section {
    background: var(--surface);
    border-radius: 12px;
    padding: 20px 24px;
    margin-bottom: 24px;
    box-shadow: 0 4px 16px rgba(0,0,0,0.2);
  }
  section h2 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin-bottom: 16px;
  }
  svg { width: 100%; height: auto; display: block; }
  .regression {
    padding: 16px;
    border-radius: 8px;
    margin-bottom: 24px;
    border-left: 4px solid;
    background: rgba(255,255,255,0.02);
  }
  .regression.stable { border-left-color: ${PALETTE.ok}; }
  .regression.improved { border-left-color: ${PALETTE.ok}; }
  .regression.first-run { border-left-color: ${PALETTE.accent}; }
  .regression.regressed { border-left-color: ${PALETTE.strict}; }
  .regression strong { font-size: 14px; }
  .regression ul { margin-top: 12px; padding-left: 24px; color: var(--text-muted); font-size: 13px; }
  .regression li { margin: 4px 0; }
  table.findings {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  table.findings th, table.findings td {
    padding: 8px 10px;
    text-align: left;
    border-bottom: 1px solid rgba(255,255,255,0.05);
  }
  table.findings th {
    color: var(--text-muted);
    font-weight: 500;
    text-transform: uppercase;
    font-size: 11px;
    letter-spacing: 0.05em;
  }
  td.num { font-variant-numeric: tabular-nums; text-align: right; }
  td.strict { color: ${PALETTE.strict}; font-weight: 600; }
  td.warn { color: ${PALETTE.warn}; }
  td.grade {
    font-weight: 700;
    text-align: center;
    width: 50px;
  }
  td.grade-A { color: ${PALETTE.gradeA}; }
  td.grade-B { color: ${PALETTE.gradeB}; }
  td.grade-C { color: ${PALETTE.gradeC}; }
  td.grade-D { color: ${PALETTE.gradeD}; }
  td.grade-F { color: ${PALETTE.gradeF}; }
  td.kinds { color: var(--text-muted); font-size: 12px; font-family: ui-monospace, monospace; }
  .findings-empty {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-style: italic;
  }
  footer {
    margin-top: 48px;
    padding-top: 24px;
    border-top: 1px solid rgba(255,255,255,0.05);
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }
  footer code {
    font-family: ui-monospace, monospace;
    background: rgba(255,255,255,0.05);
    padding: 2px 6px;
    border-radius: 4px;
  }
</style>
</head>
<body>
<main>
  <header>
    <h1>Supersociety Score — ${esc(journey)}</h1>
    <div class="composite">
      <span class="composite-number">${score.composite}</span>
      <span class="composite-out-of">/100</span>
      <span class="composite-grade">${esc(score.grade)}</span>
    </div>
    <p class="headline">${headline}</p>
  </header>

  ${regressionSection}

  <section>
    <h2>Trend — composite across ${history.length} run${history.length === 1 ? '' : 's'}</h2>
    ${trendChart}
  </section>

  <section>
    <h2>Per-category breakdown</h2>
    ${categoryBars}
  </section>

  <section>
    <h2>Findings by category</h2>
    ${findingTable}
  </section>

  <footer>
    <code>${esc(journey)}</code> · <code>${esc(timestamp)}</code>${commit ? ` · <code>${esc(commit)}</code>` : ''}
    <br>
    Generated by PlausiDen-Crawler · ${score.totalStrict} strict + ${score.totalWarn} warn finding(s)
  </footer>
</main>
</body>
</html>
`;
}
