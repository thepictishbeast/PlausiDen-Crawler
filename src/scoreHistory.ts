/**
 * scoreHistory.ts — persistent Supersociety Score trend
 * tracking. T76 cycle 33.
 *
 * Cycle 32 introduced the Supersociety Score: a 0-100 grade
 * computed from every audit run. A single number is useful
 * in isolation, but the operator's real question is "did
 * this commit make it better or worse?". This module persists
 * the per-run composite + category breakdown in a journey-
 * scoped JSONL file (`runs/<journey>-score-history.jsonl`),
 * detects regressions vs the most-recent prior entry, and
 * emits structured deltas for the console summary.
 *
 * Storage: JSONL (one JSON object per line) for trivial
 * append + tail. No JSON Schema enforcement here; the
 * entries are read tolerantly (malformed lines skipped).
 *
 * Per-journey isolation: each journey has its own history.
 * A site that runs both `plausiden-smoke` and `skillshots-poc`
 * shouldn't conflate them — they have different baselines.
 * Filename: `runs/<journey-slug>-score-history.jsonl`.
 *
 * Regression policy:
 *   - Composite drop ≥ 5 points       → flagged.
 *   - Category drop ≥ 10 points       → flagged.
 *   - Category drop ≥ one letter grade → flagged.
 *   - New strict findings in a category → flagged.
 *
 * Out of scope:
 *   * Chart visualisation (HTML renderer is a separate cycle).
 *   * Pruning old entries (a future maintenance pass could
 *     prune entries older than N days; the file is small).
 *   * Threshold configurability per journey.
 */

import { existsSync, readFileSync, appendFileSync, mkdirSync } from 'fs';
import { dirname, join } from 'path';
import type { SupersocietyScore, SupersocietyCategoryScore } from './supersocietyScore.js';

export interface ScoreHistoryEntry {
  /** ISO 8601 UTC timestamp. */
  timestamp: string;
  /** Journey name (no extension). */
  journey: string;
  /** 0-100 composite score. */
  composite: number;
  /** Letter grade A..F. */
  grade: string;
  /** Total strict findings. */
  totalStrict: number;
  /** Total warn findings. */
  totalWarn: number;
  /** Per-category scores. Minimal shape for the trend file —
   *  weight + contributingKinds are NOT persisted (the
   *  categoriser's weights can change over time; we want the
   *  historical scores to remain meaningful regardless). */
  categories: Array<{
    category: string;
    score: number;
    grade: string;
    strict: number;
    warn: number;
  }>;
  /** Optional commit SHA / build ID — populated by the caller
   *  if known. Lets us trace regressions back to a release. */
  commit?: string;
}

export interface CategoryRegression {
  category: string;
  /** Score in the prior entry. */
  priorScore: number;
  /** Score in this entry. */
  currentScore: number;
  /** Delta — negative numbers are regressions. */
  delta: number;
  /** True iff the letter grade dropped. */
  gradeDropped: boolean;
  /** True iff at least one new strict finding appeared. */
  newStrict: boolean;
  /** Human-readable explanation. */
  message: string;
}

export interface ScoreRegression {
  /** True if the composite dropped ≥ 5 points OR any
   *  category regression flagged below. */
  hasRegression: boolean;
  /** Composite delta (negative = regression). */
  compositeDelta: number;
  /** Letter grade delta — true if the overall grade dropped. */
  overallGradeDropped: boolean;
  /** Per-category regressions, sorted worst-first. */
  categoryRegressions: CategoryRegression[];
  /** Headline summary for the console. */
  headline: string;
}

const COMPOSITE_REGRESSION_THRESHOLD = 5;
const CATEGORY_REGRESSION_THRESHOLD = 10;
const GRADE_RANK: Record<string, number> = { A: 4, B: 3, C: 2, D: 1, F: 0 };

function journeySlug(journey: string): string {
  // Tolerate full paths or bare names; strip dirname + extension.
  const base = journey.replace(/\.json$/i, '').split(/[\\/]/).pop() ?? journey;
  // Sanitize to safe filename characters.
  return base.replace(/[^a-zA-Z0-9._-]/g, '_');
}

function historyPath(runsDir: string, journey: string): string {
  return join(runsDir, `${journeySlug(journey)}-score-history.jsonl`);
}

/**
 * Convert a SupersocietyScore into a persist-ready entry.
 * Splitting this from `appendScoreHistoryEntry` lets the
 * caller stamp the timestamp + commit independently.
 */
export function buildScoreHistoryEntry(
  score: SupersocietyScore,
  opts: { journey: string; timestamp?: string; commit?: string },
): ScoreHistoryEntry {
  return {
    timestamp: opts.timestamp ?? new Date().toISOString(),
    journey: opts.journey,
    composite: score.composite,
    grade: score.grade,
    totalStrict: score.totalStrict,
    totalWarn: score.totalWarn,
    categories: score.categories.map((c) => ({
      category: c.category,
      score: c.score,
      grade: c.grade,
      strict: c.strict,
      warn: c.warn,
    })),
    commit: opts.commit,
  };
}

/**
 * Read every entry from the journey's history file. Tolerates
 * missing file, blank lines, and lines that fail to parse
 * (skip + continue). Returns entries in file order (oldest
 * first; appended-to from the bottom).
 */
export function readScoreHistory(runsDir: string, journey: string): ScoreHistoryEntry[] {
  const path = historyPath(runsDir, journey);
  if (!existsSync(path)) return [];
  let raw: string;
  try {
    raw = readFileSync(path, 'utf8');
  } catch {
    return [];
  }
  const out: ScoreHistoryEntry[] = [];
  for (const line of raw.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const entry = JSON.parse(trimmed) as ScoreHistoryEntry;
      // Sanity-check shape — must have composite + timestamp.
      if (typeof entry.composite === 'number' && typeof entry.timestamp === 'string') {
        out.push(entry);
      }
    } catch {
      // Skip malformed line.
    }
  }
  return out;
}

/**
 * Append one entry to the journey's history file. Creates the
 * file (and runsDir if missing) on first call. Idempotency is
 * NOT enforced — re-running the same audit appends a new
 * entry every time, which is what we want for trend tracking.
 */
export function appendScoreHistoryEntry(
  runsDir: string,
  entry: ScoreHistoryEntry,
): void {
  const path = historyPath(runsDir, entry.journey);
  const dir = dirname(path);
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  appendFileSync(path, JSON.stringify(entry) + '\n', 'utf8');
}

/**
 * Compare a current entry against the most-recent prior
 * entry and emit a regression report. If there's no prior
 * entry (first-ever run for this journey), returns a zero-
 * delta non-regression result.
 */
export function detectScoreRegression(
  current: ScoreHistoryEntry,
  history: ScoreHistoryEntry[],
): ScoreRegression {
  // Find the most-recent prior entry (NOT the current one).
  // History is oldest-first; the prior is the last element
  // that isn't `current` itself. Since `current` may not be
  // in `history` yet, we use the simpler rule: last element
  // of history.
  if (history.length === 0) {
    return {
      hasRegression: false,
      compositeDelta: 0,
      overallGradeDropped: false,
      categoryRegressions: [],
      headline: 'First-ever run for this journey — no prior to compare.',
    };
  }
  const prior = history[history.length - 1];

  const compositeDelta = current.composite - prior.composite;
  const overallGradeDropped =
    (GRADE_RANK[current.grade] ?? 0) < (GRADE_RANK[prior.grade] ?? 0);

  const priorByCat = new Map(prior.categories.map((c) => [c.category, c]));
  const categoryRegressions: CategoryRegression[] = [];
  for (const cur of current.categories) {
    const p = priorByCat.get(cur.category);
    if (!p) continue;
    const delta = cur.score - p.score;
    const gradeDropped =
      (GRADE_RANK[cur.grade] ?? 0) < (GRADE_RANK[p.grade] ?? 0);
    const newStrict = cur.strict > p.strict;
    const isRegression =
      delta <= -CATEGORY_REGRESSION_THRESHOLD || gradeDropped || newStrict;
    if (isRegression) {
      const parts: string[] = [];
      if (delta < 0) parts.push(`score ${p.score}→${cur.score} (${delta})`);
      if (gradeDropped) parts.push(`grade ${p.grade}→${cur.grade}`);
      if (newStrict) parts.push(`+${cur.strict - p.strict} strict`);
      categoryRegressions.push({
        category: cur.category,
        priorScore: p.score,
        currentScore: cur.score,
        delta,
        gradeDropped,
        newStrict,
        message: `${cur.category}: ${parts.join(', ')}`,
      });
    }
  }
  categoryRegressions.sort((a, b) => a.delta - b.delta);

  const compositeRegressed = compositeDelta <= -COMPOSITE_REGRESSION_THRESHOLD;
  const hasRegression =
    compositeRegressed ||
    overallGradeDropped ||
    categoryRegressions.length > 0;

  let headline: string;
  if (!hasRegression) {
    if (compositeDelta > 0) {
      headline = `Score improved: ${prior.composite}→${current.composite} (+${compositeDelta}). No category regressions.`;
    } else {
      headline = `Score stable at ${current.composite} (Δ${compositeDelta}). No regressions.`;
    }
  } else {
    const bits: string[] = [];
    if (compositeRegressed || overallGradeDropped) {
      bits.push(`composite ${prior.composite}→${current.composite} (${compositeDelta}, grade ${prior.grade}→${current.grade})`);
    }
    if (categoryRegressions.length > 0) {
      bits.push(`${categoryRegressions.length} category regression(s)`);
    }
    headline = `REGRESSION: ${bits.join(', ')}.`;
  }

  return {
    hasRegression,
    compositeDelta,
    overallGradeDropped,
    categoryRegressions,
    headline,
  };
}

/**
 * Render the regression report into a multi-line console
 * block. Plain ASCII; mirrors the cycle-32 score renderer.
 */
export function renderScoreRegression(r: ScoreRegression): string {
  const lines: string[] = [];
  lines.push('');
  lines.push('=== Supersociety Score — vs prior run ===');
  lines.push(`  ${r.headline}`);
  if (r.categoryRegressions.length > 0) {
    lines.push('');
    lines.push('  category regressions (worst first):');
    for (const c of r.categoryRegressions) {
      lines.push(`    · ${c.message}`);
    }
  }
  return lines.join('\n');
}
