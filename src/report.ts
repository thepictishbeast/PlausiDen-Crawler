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
    | 'ui-overflow'
    | 'runtime-contrast'
    | 'runtime-images'
    | 'runtime-focus'
    | 'web-vitals';
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
    runtimeContrastFindings: number;
    runtimeContrastFindingsStrict: number;
    runtimeImagesFindings: number;
    runtimeImagesFindingsStrict: number;
    runtimeFocusFindings: number;
    runtimeFocusFindingsStrict: number;
    webVitalsFindings: number;
    webVitalsFindingsStrict: number;
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
  /**
   * runtime-contrast findings new in this run vs prior. Strict =
   * body-text below WCAG AA 4.5:1, warn = large-text below 3:1.
   * T29 (2026-05-04).
   */
  newRuntimeContrastFindings: CapturedEvent[];
  /**
   * runtime-images findings new in this run vs prior. Strict =
   * broken/empty-src/missing-alt; warn = CLS-risk. T75 (2026-05-04).
   */
  newRuntimeImagesFindings: CapturedEvent[];
  /**
   * runtime-focus findings new in this run vs prior. Strict =
   * interactive element with no visible focus indicator (WCAG
   * 2.4.7). T79 (2026-05-04).
   */
  newRuntimeFocusFindings: CapturedEvent[];
  /**
   * web-vitals findings new in this run vs prior. Strict =
   * Core Web Vitals 'poor' band (LCP > 4s, CLS > 0.25, INP > 500ms).
   * Warn = 'needs-improvement'. T45 (2026-05-04).
   */
  newWebVitalsFindings: CapturedEvent[];
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
    newRuntimeContrastFindings: [],
    newRuntimeImagesFindings: [],
    newRuntimeFocusFindings: [],
    newWebVitalsFindings: [],
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
    out.newRuntimeContrastFindings = current.events.filter(e => e.kind === 'runtime-contrast');
    out.newRuntimeImagesFindings = current.events.filter(e => e.kind === 'runtime-images');
    out.newRuntimeFocusFindings = current.events.filter(e => e.kind === 'runtime-focus');
    out.newWebVitalsFindings = current.events.filter(e => e.kind === 'web-vitals');
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
    else if (e.kind === 'runtime-contrast') out.newRuntimeContrastFindings.push(e);
    else if (e.kind === 'runtime-images') out.newRuntimeImagesFindings.push(e);
    else if (e.kind === 'runtime-focus') out.newRuntimeFocusFindings.push(e);
    else if (e.kind === 'web-vitals') out.newWebVitalsFindings.push(e);
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

/**
 * T2: render a "positive signal" summary that makes the silent-pass
 * state legible. The diff alone says "0 NEW errors" — that could
 * mean "all checks passed" OR "no checks ran". This makes the
 * difference visible: per axis, "checked N steps, K total findings,
 * J new vs prior".
 *
 * Operator sees:
 *
 *   axis            steps  total  new  status
 *   axe-static      27     40     0    pass (40 baseline frozen)
 *   cssHealth       27     0      0    pass (silent)
 *   uiOverflow      27     0      0    pass (silent)
 *   runtimeContrast 27     0      0    pass (silent)
 *   runtimeImages   27     0      0    pass (silent)
 *
 * 5 detection axes, all silent — that's a positive confirmation,
 * not a maybe.
 */
export function renderPositiveSignal(report: Report, diff: Diff): string {
  const stepCount = report.steps.length;
  const axes: { name: string; total: number; news: number }[] = [
    { name: 'console-errors',     total: report.counts.consoleErrors,                news: diff.newConsoleErrors.length },
    { name: 'page-errors',        total: report.counts.pageErrors,                   news: diff.newPageErrors.length },
    { name: 'failed-requests',    total: report.counts.failedRequests,               news: diff.newFailedRequests.length },
    { name: 'axe-static-a11y',    total: report.counts.a11yViolations,               news: diff.newA11yViolations.length },
    { name: 'cssHealth',          total: report.counts.cssHealthFindings,            news: diff.newCssHealthFindings.length },
    { name: 'uiOverflow',         total: report.counts.uiOverflowFindings,           news: diff.newUiOverflowFindings.length },
    { name: 'runtimeContrast',    total: report.counts.runtimeContrastFindings,      news: diff.newRuntimeContrastFindings.length },
    { name: 'runtimeImages',      total: report.counts.runtimeImagesFindings,        news: diff.newRuntimeImagesFindings.length },
    { name: 'runtimeFocus',       total: report.counts.runtimeFocusFindings,         news: diff.newRuntimeFocusFindings.length },
    { name: 'webVitals',          total: report.counts.webVitalsFindings,            news: diff.newWebVitalsFindings.length },
  ];
  const lines: string[] = [];
  lines.push(`=== positive signal (${axes.length} detection axes) ===`);
  lines.push('');
  lines.push(`  axis              steps   total    new   status`);
  lines.push(`  ----------------  ------  -------  ----  --------`);
  for (const a of axes) {
    let status: string;
    if (a.news > 0) status = 'REGRESSION';
    else if (a.total > 0) status = `pass (${a.total} baseline frozen)`;
    else status = 'pass (silent)';
    lines.push(
      `  ${a.name.padEnd(16)}  ${String(stepCount).padStart(6)}  ${String(a.total).padStart(7)}  ${String(a.news).padStart(4)}  ${status}`,
    );
  }
  const allClean = axes.every((a) => a.news === 0);
  lines.push('');
  if (allClean) {
    lines.push(`  ✓ all ${axes.length} axes silent vs prior run — positive PASS confirmation`);
  } else {
    const dirty = axes.filter((a) => a.news > 0).map((a) => a.name).join(', ');
    lines.push(`  ✗ ${axes.filter((a) => a.news > 0).length} axis/axes regressed: ${dirty}`);
  }
  return lines.join('\n');
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
