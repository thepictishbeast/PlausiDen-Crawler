/**
 * visualDiff.ts — T33 cycle 2 (closes #585): per-step screenshot
 * baseline-or-compare wiring around imageHash.dHash.
 *
 * Workflow per screenshot step:
 *   • Compute dHash of newly-saved PNG.
 *   • Look for `<baselineDir>/<base>.png`.
 *     - Present  → load both, diff, emit a `visual-diff` event
 *                  if Hamming distance > threshold (default 5).
 *     - Absent   → copy current to baseline (auto-promote first
 *                  run); emit informational `visual-diff` event
 *                  noting baseline establishment.
 *
 * The baseline is intentionally per-journey, not per-run: it
 * represents "the look we approve". Checked into the repo in
 * `runs/<journey>-baseline/`. A `git diff` over the baseline
 * directory is the human-reviewable approval.
 *
 * Thresholds:
 *   • 0–2  bits = noise (font hinting, sub-pixel AA jitter)
 *   • 3–5  bits = layout-stable, content drift
 *   • 6–10 bits = layout shift, missing element, theme swap
 *   • >10  bits = page changed substantially
 *
 * Default threshold is 5 — matches imageHash.compareHashes default.
 *
 * AVP-2 doctrine:
 *   • No new dependency: only Node stdlib + the existing
 *     imageHash module.
 *   • Baseline-write is opt-in (`autoEstablish`): CI doesn't
 *     accidentally promote a regression.
 *   • Function returns a typed result; caller decides whether
 *     to log/exit/escalate.
 */

import { readFileSync, existsSync, mkdirSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { dHash, compareHashes, type DHashResult, type DHashCompare } from './imageHash.js';

export interface VisualDiffOptions {
  /** Hamming-distance threshold; > threshold = changed. Default 5. */
  threshold?: number;
  /** If true and no baseline exists, copy the current shot to the
   *  baseline path. Default false (CI-safe). Set true for the
   *  initial promote run. */
  autoEstablish?: boolean;
}

export type VisualDiffStatus =
  | 'baseline-established'  // no baseline → just promoted (autoEstablish=true)
  | 'baseline-missing'      // no baseline → autoEstablish=false → cannot diff
  | 'unchanged'             // distance <= threshold
  | 'changed'               // distance > threshold
  | 'error';                // I/O or decode failure

export interface VisualDiffResult {
  status: VisualDiffStatus;
  /** Step base label (e.g. "01-start"). */
  base: string;
  /** Where the new screenshot lives. */
  newPath: string;
  /** Where the baseline lives (or would live). */
  baselinePath: string;
  /** dHash of the new screenshot, when computable. */
  newHash?: DHashResult;
  /** dHash of the baseline, when computable. */
  baselineHash?: DHashResult;
  /** Distance comparison, when both hashes computed. */
  comparison?: DHashCompare;
  /** Free-text human/log message. Always populated. */
  message: string;
  /** Threshold actually used. */
  thresholdUsed: number;
}

/**
 * Compare a newly-captured screenshot against a per-journey
 * baseline. Returns a typed result the caller can log or
 * convert into a CapturedEvent.
 *
 * @param newPath        absolute or run-relative path to the new PNG
 * @param baselineDir    directory holding `<base>.png` baselines
 * @param base           step base name (without `.png` extension)
 * @param opts           threshold / auto-establish toggle
 */
export function compareToBaseline(
  newPath: string,
  baselineDir: string,
  base: string,
  opts: VisualDiffOptions = {},
): VisualDiffResult {
  const threshold = opts.threshold ?? 5;
  const baselinePath = join(baselineDir, `${base}.png`);
  const baseResult: Pick<VisualDiffResult, 'base' | 'newPath' | 'baselinePath' | 'thresholdUsed'> = {
    base,
    newPath,
    baselinePath,
    thresholdUsed: threshold,
  };

  if (!existsSync(newPath)) {
    return {
      ...baseResult,
      status: 'error',
      message: `new screenshot missing: ${newPath}`,
    };
  }

  let newBuf: Buffer;
  try {
    newBuf = readFileSync(newPath);
  } catch (e: any) {
    return {
      ...baseResult,
      status: 'error',
      message: `read new failed: ${String(e?.message ?? e)}`,
    };
  }

  let newHash: DHashResult;
  try {
    newHash = dHash(newBuf);
  } catch (e: any) {
    return {
      ...baseResult,
      status: 'error',
      message: `dHash new failed: ${String(e?.message ?? e)}`,
    };
  }

  if (!existsSync(baselinePath)) {
    if (opts.autoEstablish) {
      try {
        mkdirSync(dirname(baselinePath), { recursive: true });
        copyFileSync(newPath, baselinePath);
        return {
          ...baseResult,
          status: 'baseline-established',
          newHash,
          message: `baseline established at ${baselinePath} (hash=${newHash.hex})`,
        };
      } catch (e: any) {
        return {
          ...baseResult,
          status: 'error',
          newHash,
          message: `baseline write failed: ${String(e?.message ?? e)}`,
        };
      }
    }
    return {
      ...baseResult,
      status: 'baseline-missing',
      newHash,
      message: `no baseline at ${baselinePath} (hash=${newHash.hex}) — use autoEstablish or commit one`,
    };
  }

  let baselineBuf: Buffer;
  try {
    baselineBuf = readFileSync(baselinePath);
  } catch (e: any) {
    return {
      ...baseResult,
      status: 'error',
      newHash,
      message: `read baseline failed: ${String(e?.message ?? e)}`,
    };
  }

  let baselineHash: DHashResult;
  try {
    baselineHash = dHash(baselineBuf);
  } catch (e: any) {
    return {
      ...baseResult,
      status: 'error',
      newHash,
      message: `dHash baseline failed: ${String(e?.message ?? e)}`,
    };
  }

  const comparison = compareHashes(newHash, baselineHash, threshold);
  const status: VisualDiffStatus = comparison.changed ? 'changed' : 'unchanged';
  const message = comparison.changed
    ? `visual change detected: distance=${comparison.distance}/64 > ${threshold} (new=${newHash.hex} base=${baselineHash.hex})`
    : `unchanged: distance=${comparison.distance}/64 <= ${threshold}`;
  return {
    ...baseResult,
    status,
    newHash,
    baselineHash,
    comparison,
    message,
  };
}

/**
 * Convenience: derive the baseline directory for a journey.
 * Convention: `<runsDir>/<journeyName>-baseline/`.
 *
 * Pure path math; does not stat or create.
 */
export function defaultBaselineDir(runsDir: string, journeyName: string): string {
  return join(runsDir, `${journeyName}-baseline`);
}
