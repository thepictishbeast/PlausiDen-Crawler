/**
 * Report — JSON bundle of a run's captured events, step results, and
 * a diff against the prior baseline. Consumed by the runner + CI
 * gate.
 */
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import type { StepResult } from './journey';

export interface CapturedEvent {
  t: number;
  kind:
    | 'console'
    | 'pageerror'
    | 'request-failed'
    | 'response-error'
    | 'csp-violation'
    | 'a11y-violation'
    | 'css-health'
    | 'ui-overflow';
  level?: string;
  text: string;
  url?: string;
  status?: number;
  stack?: string;
  impact?: 'minor' | 'moderate' | 'serious' | 'critical';
  ruleId?: string;
  /** css-health-only: severity bucket from cssHealth detector. */
  severity?: 'strict' | 'warn';
}

export interface Report {
  target: string;
  journey: string;
  viewport: { w: number; h: number };
  started: string;
  durationMs: number;
  counts: {
    consoleErrors: number;
    pageErrors: number;
    failedRequests: number;
    a11yViolations: number;
    cssHealthFindings: number;
    cssHealthFindingsStrict: number;
    uiOverflowFindings: number;
    uiOverflowFindingsStrict: number;
    total: number;
    stepsOk: number;
    stepsFailed: number;
  };
  events: CapturedEvent[];
  steps: StepResult[];
  /** Events grouped by what the crawler was doing when they fired.
   *  "during step N (label X): event kind/text" — answers the user's
   *  question "what was the crawler doing when the console log hit?" */
  eventsByStep?: Array<{
    stepIndex: number;
    stepLabel: string;
    stepKind: string;
    windowMs: [number, number];
    events: CapturedEvent[];
  }>;
}

export interface Diff {
  newConsoleErrors: CapturedEvent[];
  newPageErrors: CapturedEvent[];
  newFailedRequests: CapturedEvent[];
  newA11yViolations: CapturedEvent[];
  /**
   * cssHealth findings new in this run vs the prior baseline. A new
   * strict cssHealth finding is the same severity-class as a new
   * console error: ship-blocking. T71 (2026-05-04).
   */
  newCssHealthFindings: CapturedEvent[];
  /**
   * uiOverflow findings new in this run vs the prior baseline.
   * Strict = page-h-scroll, element-bleed, text-clipped, or
   * tap-target on mobile. T28 (2026-05-04).
   */
  newUiOverflowFindings: CapturedEvent[];
  newlyBrokenSteps: StepResult[];
  fixedSteps: StepResult[];
}

const key = (e: CapturedEvent) =>
  `${e.kind}|${(e.text || '').slice(0, 200)}|${e.url || ''}|${e.status || ''}|${e.ruleId || ''}`;

export function diffReports(current: Report, prior: Report | null): Diff {
  const out: Diff = {
    newConsoleErrors: [],
    newPageErrors: [],
    newFailedRequests: [],
    newA11yViolations: [],
    newCssHealthFindings: [],
    newUiOverflowFindings: [],
    newlyBrokenSteps: [],
    fixedSteps: [],
  };
  if (!prior) {
    out.newConsoleErrors = current.events.filter(e => e.kind === 'console' && e.level === 'error');
    out.newPageErrors = current.events.filter(e => e.kind === 'pageerror');
    out.newFailedRequests = current.events.filter(e => e.kind === 'request-failed' || e.kind === 'response-error');
    out.newA11yViolations = current.events.filter(e => e.kind === 'a11y-violation');
    out.newCssHealthFindings = current.events.filter(e => e.kind === 'css-health');
    out.newUiOverflowFindings = current.events.filter(e => e.kind === 'ui-overflow');
    return out;
  }
  const priorKeys = new Set(prior.events.map(key));
  for (const e of current.events) {
    if (priorKeys.has(key(e))) continue;
    if (e.kind === 'console' && e.level === 'error') out.newConsoleErrors.push(e);
    else if (e.kind === 'pageerror') out.newPageErrors.push(e);
    else if (e.kind === 'request-failed' || e.kind === 'response-error') out.newFailedRequests.push(e);
    else if (e.kind === 'a11y-violation') out.newA11yViolations.push(e);
    else if (e.kind === 'css-health') out.newCssHealthFindings.push(e);
    else if (e.kind === 'ui-overflow') out.newUiOverflowFindings.push(e);
  }
  const priorStepLabels = new Map(
    prior.steps.map((s, i) => [s.step.label || `${s.step.kind}-${i}`, s])
  );
  for (const s of current.steps) {
    const id = s.step.label || `${s.step.kind}-${s.index}`;
    const was = priorStepLabels.get(id);
    if (s.ok && was && !was.ok) out.fixedSteps.push(s);
    if (!s.ok && was && was.ok) out.newlyBrokenSteps.push(s);
  }
  return out;
}

export function findPriorRun(runsDir: string, exceptPath?: string): Report | null {
  if (!existsSync(runsDir)) return null;
  const entries = readdirSync(runsDir).filter(n => !n.startsWith('.')).sort();
  const candidates = entries.filter(n => !exceptPath || !exceptPath.endsWith(n));
  const prior = candidates[candidates.length - 1];
  if (!prior) return null;
  const path = join(runsDir, prior, 'report.json');
  if (!existsSync(path)) return null;
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
}
