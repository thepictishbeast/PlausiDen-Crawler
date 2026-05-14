/**
 * supersocietyBadgeAggregate.ts — combine all journey scores
 * into one badge. T76 cycle 61.
 *
 * The per-journey badge (cycle 36) is great for "is THIS surface
 * supersociety?" but doesn't answer "is the WHOLE PlausiDen
 * project supersociety?" — which is the question a visitor to
 * the GitHub repo README wants to know.
 *
 * This module reads every `runs/<journey>-latest-score.json`
 * file in the `runs/` directory and produces a single
 * aggregated badge that shows:
 *   - the LOWEST composite + grade across all journeys
 *     (the worst is the bottleneck — the dashboard isn't
 *     supersociety until every surface is)
 *   - the count of journeys averaged
 *   - a tooltip listing the per-journey breakdown
 *
 * Output is a 200×20 px SVG (wider than the per-journey badge
 * to fit `supersociety N/M` value form). Auto-emitted at
 * `badges/supersociety.svg` at repo root on every audit run.
 *
 * REGRESSION-GUARD: NaN / missing-file inputs degrade
 * gracefully (badge shows '?' grade with muted colour) rather
 * than crashing the audit.
 */

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { SupersocietyScore } from './supersocietyScore.js';

export interface AggregateScore {
  /** Minimum composite across all journeys (0-100, integer). */
  composite: number;
  /** Grade for the minimum composite. */
  grade: 'A' | 'B' | 'C' | 'D' | 'F';
  /** How many journeys contributed. */
  journeyCount: number;
  /** Per-journey breakdown for the badge tooltip. */
  perJourney: Array<{ name: string; composite: number; grade: string }>;
}

function esc(s: unknown): string {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function gradeFor(composite: number): 'A' | 'B' | 'C' | 'D' | 'F' {
  if (composite >= 90) return 'A';
  if (composite >= 80) return 'B';
  if (composite >= 70) return 'C';
  if (composite >= 60) return 'D';
  return 'F';
}

const COLORS: Record<string, string> = {
  A: '#22c55e',
  B: '#84cc16',
  C: '#facc15',
  D: '#fb923c',
  F: '#ef4444',
};

const TEXT_ON_COLOR: Record<string, string> = {
  A: '#0a1a0a',
  B: '#0a1a0a',
  C: '#1a1500',
  D: '#1a0a00',
  F: '#1a0000',
};

/**
 * Walk the runs directory, find every `*-latest-score.json`
 * file, and compute the aggregate score. Empty / unreadable
 * directory returns a sentinel score so the badge still
 * renders.
 */
export function computeAggregateScore(runsDir: string): AggregateScore {
  let entries: string[] = [];
  try {
    entries = readdirSync(runsDir);
  } catch {
    return {
      composite: 0,
      grade: 'F',
      journeyCount: 0,
      perJourney: [],
    };
  }
  // T76 cycle 61: exclude detector test-fixture journeys.
  // `t76-detector-fixtures*` are intentional-fail fixtures
  // designed to verify that detectors emit the right findings
  // on broken inputs — their composite scores are SUPPOSED to
  // be low. Including them would drag the aggregate badge to a
  // misleading F even when every real surface is at 100/100.
  const scoreFiles = entries.filter(
    (n) =>
      n.endsWith('-latest-score.json') &&
      !n.startsWith('t76-detector-fixtures'),
  );
  const perJourney: Array<{ name: string; composite: number; grade: string }> = [];
  for (const f of scoreFiles) {
    try {
      const raw = readFileSync(join(runsDir, f), 'utf8');
      const score = JSON.parse(raw) as SupersocietyScore;
      if (typeof score.composite !== 'number' || Number.isNaN(score.composite)) continue;
      const journeyName = f.replace(/-latest-score\.json$/, '');
      perJourney.push({
        name: journeyName,
        composite: Math.round(score.composite),
        grade: score.grade,
      });
    } catch {
      // unreadable / unparseable — skip; the operator can re-run
      // the audit to regenerate the file.
    }
  }
  if (perJourney.length === 0) {
    return {
      composite: 0,
      grade: 'F',
      journeyCount: 0,
      perJourney: [],
    };
  }
  perJourney.sort((a, b) => a.name.localeCompare(b.name));
  const minComposite = perJourney.reduce(
    (acc, j) => Math.min(acc, j.composite),
    100,
  );
  return {
    composite: minComposite,
    grade: gradeFor(minComposite),
    journeyCount: perJourney.length,
    perJourney,
  };
}

/**
 * Render the aggregate badge.
 *
 * Layout (200 × 20):
 *   [   supersociety   |   A 95/100 (13)   ]
 *           80px              120px
 *
 * The `(13)` suffix on the value side shows how many journeys
 * were averaged — gives context for "are we close to clean
 * across the board, or is it just one cherry-picked journey?"
 */
export function renderAggregateBadge(score: AggregateScore): string {
  const W = 200;
  const H = 20;
  const SPLIT = 80;
  const color = COLORS[score.grade] ?? '#9990bb';
  const textColor = TEXT_ON_COLOR[score.grade] ?? '#ffffff';
  const value = score.journeyCount > 0
    ? `${score.grade} ${score.composite}/100 (${score.journeyCount})`
    : 'no data';
  const ariaLabel = score.journeyCount > 0
    ? `Supersociety aggregate: grade ${score.grade}, ${score.composite} out of 100, across ${score.journeyCount} journey(s) — worst-of-N`
    : 'Supersociety aggregate: no journey data available';
  const tooltip = score.perJourney.length > 0
    ? score.perJourney.map((j) => `${j.name}: ${j.grade} ${j.composite}`).join('\n')
    : ariaLabel;

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(ariaLabel)}">
  <title>${esc(tooltip)}</title>
  <linearGradient id="b" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r">
    <rect width="${W}" height="${H}" rx="3" fill="#fff"/>
  </clipPath>
  <g clip-path="url(#r)">
    <rect width="${SPLIT}" height="${H}" fill="#555"/>
    <rect x="${SPLIT}" width="${W - SPLIT}" height="${H}" fill="${color}"/>
    <rect width="${W}" height="${H}" fill="url(#b)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="system-ui, -apple-system, Segoe UI, Roboto, sans-serif" font-size="11">
    <text x="${SPLIT / 2}" y="15" fill="#010101" fill-opacity=".3">supersociety</text>
    <text x="${SPLIT / 2}" y="14">supersociety</text>
    <text x="${SPLIT + (W - SPLIT) / 2}" y="15" fill="#010101" fill-opacity=".3">${esc(value)}</text>
    <text x="${SPLIT + (W - SPLIT) / 2}" y="14" fill="${textColor}">${esc(value)}</text>
  </g>
</svg>
`;
}
