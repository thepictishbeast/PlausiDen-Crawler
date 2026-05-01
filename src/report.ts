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
  kind: 'console' | 'pageerror' | 'request-failed' | 'response-error' | 'csp-violation' | 'a11y-violation' | 'd0-violation';
  level?: string;
  text: string;
  url?: string;
  status?: number;
  stack?: string;
  impact?: 'minor' | 'moderate' | 'serious' | 'critical';
  ruleId?: string;
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
    d0Violations: number;
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
  newD0Violations: CapturedEvent[];
  newlyBrokenSteps: StepResult[];
  fixedSteps: StepResult[];
}

const key = (e: CapturedEvent) =>
  `${e.kind}|${(e.text || '').slice(0, 200)}|${e.url || ''}|${e.status || ''}|${e.ruleId || ''}`;

/**
 * Allowlist matcher. Each entry can pin by `kind`, by URL substring, by
 * status, or by a regex on text. An event matching any pinned entry is
 * dropped from the regression diff so a journey that legitimately surfaces
 * known-tolerated noise (e.g. /api/voter/dashboard 400 with a TEST seed
 * that has no voter row) doesn't make every clean run look broken.
 */
export interface AllowEntry {
  kind?: CapturedEvent['kind'];
  level?: string;
  urlIncludes?: string;
  status?: number;
  textMatches?: string;
  reason?: string;
}

const matchesAllow = (e: CapturedEvent, allow: AllowEntry[]): boolean => {
  for (const a of allow) {
    if (a.kind && a.kind !== e.kind) continue;
    if (a.level && a.level !== (e.level || '')) continue;
    if (a.urlIncludes && !(e.url || '').includes(a.urlIncludes)) continue;
    if (a.status !== undefined && a.status !== (e.status || 0)) continue;
    if (a.textMatches && !new RegExp(a.textMatches).test(e.text || '')) continue;
    return true;
  }
  return false;
};

export function diffReports(current: Report, prior: Report | null, allow: AllowEntry[] = []): Diff {
  const out: Diff = {
    newConsoleErrors: [],
    newPageErrors: [],
    newFailedRequests: [],
    newA11yViolations: [],
    newD0Violations: [],
    newlyBrokenSteps: [],
    fixedSteps: [],
  };
  if (!prior) {
    const keep = (e: CapturedEvent) => !matchesAllow(e, allow);
    out.newConsoleErrors = current.events.filter(e => e.kind === 'console' && e.level === 'error').filter(keep);
    out.newPageErrors = current.events.filter(e => e.kind === 'pageerror').filter(keep);
    out.newFailedRequests = current.events.filter(e => e.kind === 'request-failed' || e.kind === 'response-error').filter(keep);
    out.newA11yViolations = current.events.filter(e => e.kind === 'a11y-violation').filter(keep);
    out.newD0Violations = current.events.filter(e => e.kind === 'd0-violation').filter(keep);
    return out;
  }
  const priorKeys = new Set(prior.events.map(key));
  for (const e of current.events) {
    if (priorKeys.has(key(e))) continue;
    if (matchesAllow(e, allow)) continue;
    if (e.kind === 'console' && e.level === 'error') out.newConsoleErrors.push(e);
    else if (e.kind === 'pageerror') out.newPageErrors.push(e);
    else if (e.kind === 'request-failed' || e.kind === 'response-error') out.newFailedRequests.push(e);
    else if (e.kind === 'a11y-violation') out.newA11yViolations.push(e);
    else if (e.kind === 'd0-violation') out.newD0Violations.push(e);
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
 * Pick the most-recent prior run for the SAME journey, not whatever
 * happens to be lexically last in `runs/`. Without journey scoping,
 * the voter crawl diffs against the admin crawl (or `sacredvote-org`)
 * and every run looks "regressing" because the journeys touch
 * different code paths and surface different known-tolerated errors.
 *
 * Each run dir is named `<journey>-<iso-timestamp-with-dashes>`. We
 * derive the journey prefix from `exceptPath` (the current run dir)
 * by stripping the trailing ISO-stamp, then keep only candidates
 * sharing that prefix. Falls back to global last-run if `journeyName`
 * is supplied without `exceptPath` (CLI callers).
 */
export function findPriorRun(
  runsDir: string,
  exceptPath?: string,
  journeyName?: string,
): Report | null {
  if (!existsSync(runsDir)) return null;
  const entries = readdirSync(runsDir).filter(n => !n.startsWith('.')).sort();
  // ISO-timestamp suffix appended by main.ts: `YYYY-MM-DDTHH-MM-SS-mmmZ`.
  const isoSuffix = /-\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-\d{3}Z$/;
  const prefixOf = (n: string): string => n.replace(isoSuffix, '');
  let prefix = journeyName || '';
  if (!prefix && exceptPath) {
    const base = exceptPath.replace(/\/$/, '').split('/').pop() || '';
    prefix = prefixOf(base);
  }
  let candidates = entries;
  if (prefix) {
    candidates = entries.filter(n => prefixOf(n) === prefix);
  }
  if (exceptPath) {
    candidates = candidates.filter(n => !exceptPath.endsWith(n));
  }
  const prior = candidates[candidates.length - 1];
  if (!prior) return null;
  const path = join(runsDir, prior, 'report.json');
  if (!existsSync(path)) return null;
  try { return JSON.parse(readFileSync(path, 'utf8')); } catch { return null; }
}
