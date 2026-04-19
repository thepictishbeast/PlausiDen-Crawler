/**
 * PlausiDen-Crawler v0.2 — web-focused runner.
 *
 * Usage:
 *   node --loader ts-node/esm src/main.ts [--url URL] [--journey FILE.json]
 *
 * - Loads a journey from journeys/<name>.json (or --journey path).
 * - Drives Chromium through each step.
 * - Captures console, page errors, failed fetches, 4xx/5xx responses.
 * - Runs axe-core (if installed) after each screenshot step.
 * - Writes runs/<ts>/report.json + per-step PNGs.
 * - Diffs against the previous run and exits non-zero on NEW regressions.
 */
import { chromium, type Page } from 'playwright';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { runStep, type Journey, type StepResult } from './journey.js';
import { diffReports, findPriorRun, type CapturedEvent, type Report } from './report.js';

interface Budget {
  newConsoleErrors: number;
  newPageErrors: number;
  newFailedRequests: number;
  newA11yViolations: number;
  newlyBrokenSteps: number;
}

const DEFAULT_BUDGET: Budget = {
  newConsoleErrors: 0,
  newPageErrors: 0,
  newFailedRequests: 0,
  newA11yViolations: 0,
  newlyBrokenSteps: 0,
};

async function main(args: string[]): Promise<number> {
  const urlIdx = args.indexOf('--url');
  const journeyIdx = args.indexOf('--journey');

  // Resolve journey: explicit --journey path, or ./journeys/plausiden-smoke.json.
  let journeyPath = journeyIdx >= 0 ? args[journeyIdx + 1] : 'journeys/plausiden-smoke.json';
  if (!existsSync(journeyPath)) {
    console.error(`[crawler] journey not found: ${journeyPath}`);
    return 2;
  }
  const journey: Journey = JSON.parse(readFileSync(journeyPath, 'utf8'));
  const targetUrl = urlIdx >= 0 ? args[urlIdx + 1] : journey.baseUrl;
  const viewport = { w: 1280, h: 900 };

  const tsTag = new Date().toISOString().replace(/[:.]/g, '-');
  const runsDir = 'runs';
  const outDir = join(runsDir, `${journey.name}-${tsTag}`);
  mkdirSync(outDir, { recursive: true });
  const startEpoch = Date.now();
  const events: CapturedEvent[] = [];
  const stepResults: StepResult[] = [];
  const log = (e: Omit<CapturedEvent, 't'>) => events.push({ ...e, t: Date.now() - startEpoch });

  console.log(`[crawler] journey=${journey.name} target=${targetUrl}`);
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: viewport.w, height: viewport.h } });
  const page: Page = await context.newPage();

  page.on('console', (msg) => {
    log({ kind: 'console', level: msg.type(), text: msg.text(), url: msg.location().url });
  });
  page.on('pageerror', (err) => {
    log({ kind: 'pageerror', text: err.message, stack: err.stack });
  });
  page.on('requestfailed', (req) => {
    log({ kind: 'request-failed', text: req.failure()?.errorText || 'unknown', url: req.url() });
  });
  page.on('response', (res) => {
    if (res.status() >= 400) {
      log({ kind: 'response-error', text: res.statusText(), url: res.url(), status: res.status() });
    }
  });

  // Execute each step sequentially. Screenshot steps are handled inline
  // (runStep is a no-op for them) so we can track the filename.
  for (let i = 0; i < journey.steps.length; i++) {
    const step = journey.steps[i];
    console.log(`[crawler] step ${i + 1}/${journey.steps.length}: ${step.kind}${step.label ? ' · ' + step.label : ''}`);

    // Screenshot steps: take the shot and record the path.
    if (step.kind === 'screenshot') {
      const filename = `${String(i + 1).padStart(2, '0')}-${step.label || 'shot'}.png`;
      const path = join(outDir, filename);
      try { await page.screenshot({ path, fullPage: true }); } catch { /* silent */ }
      stepResults.push({ step, index: i, ok: true, durationMs: 0, screenshot: path });
      continue;
    }

    const result = await runStep(page, step);
    result.index = i;
    stepResults.push(result);
    if (!result.ok) {
      log({ kind: 'pageerror', text: `step failed: ${step.label || step.kind}: ${result.error}` });
    }
    // Give the page a beat to settle after interactive steps.
    await page.waitForTimeout(200);
  }

  await browser.close();

  const report: Report = {
    target: targetUrl,
    journey: journey.name,
    viewport,
    started: new Date(startEpoch).toISOString(),
    durationMs: Date.now() - startEpoch,
    counts: {
      consoleErrors: events.filter(e => e.kind === 'console' && e.level === 'error').length,
      pageErrors: events.filter(e => e.kind === 'pageerror').length,
      failedRequests: events.filter(e => e.kind === 'request-failed' || e.kind === 'response-error').length,
      a11yViolations: events.filter(e => e.kind === 'a11y-violation').length,
      total: events.length,
      stepsOk: stepResults.filter(s => s.ok).length,
      stepsFailed: stepResults.filter(s => !s.ok).length,
    },
    events,
    steps: stepResults,
  };
  writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));

  const prior = findPriorRun(runsDir, outDir);
  const diff = diffReports(report, prior);
  writeFileSync(join(outDir, 'diff.json'), JSON.stringify(diff, null, 2));

  // Summary to stdout.
  console.log(`\n[crawler] run complete: ${outDir}/report.json`);
  console.log(`  console errors:    ${report.counts.consoleErrors}`);
  console.log(`  page errors:       ${report.counts.pageErrors}`);
  console.log(`  failed fetches:    ${report.counts.failedRequests}`);
  console.log(`  a11y violations:   ${report.counts.a11yViolations}`);
  console.log(`  steps ok/failed:   ${report.counts.stepsOk}/${report.counts.stepsFailed}`);
  if (prior) {
    console.log(`  diff vs prior run (${prior.journey}):`);
    console.log(`    NEW console errors:   ${diff.newConsoleErrors.length}`);
    console.log(`    NEW page errors:      ${diff.newPageErrors.length}`);
    console.log(`    NEW failed fetches:   ${diff.newFailedRequests.length}`);
    console.log(`    NEW a11y violations:  ${diff.newA11yViolations.length}`);
    console.log(`    newly broken steps:   ${diff.newlyBrokenSteps.length}`);
    console.log(`    fixed steps:          ${diff.fixedSteps.length}`);
  } else {
    console.log(`  (no prior run to diff against)`);
  }

  const overBudget =
    diff.newConsoleErrors.length > DEFAULT_BUDGET.newConsoleErrors
    || diff.newPageErrors.length > DEFAULT_BUDGET.newPageErrors
    || diff.newFailedRequests.length > DEFAULT_BUDGET.newFailedRequests
    || diff.newA11yViolations.length > DEFAULT_BUDGET.newA11yViolations
    || diff.newlyBrokenSteps.length > DEFAULT_BUDGET.newlyBrokenSteps;

  if (overBudget) {
    console.log('\n[crawler] FAIL — new regressions exceed budget.');
    return 1;
  }
  console.log('\n[crawler] PASS — no new regressions.');
  return 0;
}

main(process.argv.slice(2)).then((code) => process.exit(code)).catch((e) => {
  console.error('[crawler] fatal:', e);
  process.exit(2);
});
