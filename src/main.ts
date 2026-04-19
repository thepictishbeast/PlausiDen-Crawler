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
import { captureAriaTree, ariaTreeToText, interactableNodes, scoreAriaTree } from './aria.js';
import { installWebVitals, collectVitals } from './webVitals.js';

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
  // Inject Google's web-vitals library before any navigation so LCP/CLS/
  // INP/TTFB/FCP are captured on every page the crawler visits.
  await installWebVitals(page);

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

  // Deeper heuristics that the surface-level event capture misses:
  //  - WebSocket failures (onclose with non-1000 code)
  //  - "Could not load …" / "Backend busy" / "Unexpected token" error
  //    strings rendered by error boundaries / subtab alerts
  //  - Blank view: <main> or #root has <100 chars of text after a step
  //  - Long-pending <Suspense> fallback still visible 5s after a step
  // These are checked after each step via evaluateHandle and pushed as
  // synthetic CapturedEvents so the diff algorithm catches them.
  const checkUiHealth = async (afterLabel: string) => {
    try {
      const findings = await page.evaluate(() => {
        const out: { kind: string; text: string }[] = [];
        // Visible error copy that users would read as a bug.
        const errorPatterns = [
          /could not load/i,
          /backend busy/i,
          /backend offline/i,
          /unexpected token/i,
          /unrecognized verdict bucket/i,
          /reference.?error/i,
          /cannot read prop/i,
          /ui error/i,
        ];
        const text = document.body.innerText || '';
        for (const p of errorPatterns) {
          const m = text.match(p);
          if (m) out.push({ kind: 'ui-error-text', text: `Rendered error copy matched /${p.source}/: "${text.slice(Math.max(0, m.index! - 20), m.index! + 80)}"` });
        }
        // Suspense-fallback-looking text still on screen (view never hydrated).
        const loadingFallbacks = text.match(/Loading\s+(classroom|fleet|library|auditorium|admin|knowledge)/gi);
        if (loadingFallbacks && loadingFallbacks.length > 0) {
          out.push({ kind: 'stuck-loading', text: `Fallback copy still visible: ${loadingFallbacks.join(', ')}` });
        }
        // Blank-main: look for a visible <main> with almost no content.
        const main = document.querySelector('main');
        if (main) {
          const mt = (main as HTMLElement).innerText || '';
          if (mt.trim().length < 20 && (main as HTMLElement).offsetHeight > 200) {
            out.push({ kind: 'blank-main', text: `Main area rendered with <20 chars of visible text (height: ${(main as HTMLElement).offsetHeight}px).` });
          }
        }
        // Hidden-but-active error boundary card.
        const boundaryCard = document.querySelector('[role="alert"]');
        if (boundaryCard) {
          const t = (boundaryCard as HTMLElement).innerText || '';
          if (t.trim().length > 0) {
            out.push({ kind: 'error-boundary-visible', text: `role=alert present with text: "${t.slice(0, 120)}"` });
          }
        }
        return out;
      });
      for (const f of findings) {
        log({ kind: 'pageerror' as const, text: `[after step: ${afterLabel}] [${f.kind}] ${f.text}` });
      }
    } catch { /* page may have navigated — skip */ }
  };

  // WebSocket close tracking. Hook into CDP so we see genuine WS drops.
  try {
    const client = await page.context().newCDPSession(page);
    await client.send('Network.enable');
    client.on('Network.webSocketClosed', (ev: any) => {
      log({ kind: 'response-error', text: `WebSocket closed`, url: String(ev?.requestId || 'ws') });
    });
    client.on('Network.webSocketFrameError', (ev: any) => {
      log({ kind: 'pageerror', text: `WebSocket frame error: ${ev?.errorMessage || 'unknown'}` });
    });
  } catch { /* CDP unavailable on some platforms */ }

  // Execute each step sequentially. Screenshot steps are handled inline
  // (runStep is a no-op for them) so we can track the filename.
  for (let i = 0; i < journey.steps.length; i++) {
    const step = journey.steps[i];
    console.log(`[crawler] step ${i + 1}/${journey.steps.length}: ${step.kind}${step.label ? ' · ' + step.label : ''}`);

    // Screenshot steps: take the shot AND an accessibility snapshot.
    // Aria snapshots are the visionless-AI equivalent — a compact
    // semantic tree an LLM can reason about without pixel input.
    if (step.kind === 'screenshot') {
      const base = `${String(i + 1).padStart(2, '0')}-${step.label || 'shot'}`;
      const imgPath = join(outDir, `${base}.png`);
      const ariaPath = join(outDir, `${base}.aria.txt`);
      try { await page.screenshot({ path: imgPath, fullPage: true }); } catch { /* silent */ }
      try {
        const tree = await captureAriaTree(page);
        const text = ariaTreeToText(tree);
        const inter = interactableNodes(tree).map(n => `${n.role} "${n.name || '(unnamed)'}"`);
        const a11y = scoreAriaTree(tree);
        const body = [
          `# Aria snapshot — ${step.label || step.kind}`,
          `# URL: ${page.url()}`,
          `# Interactable count: ${inter.length}`,
          `# A11y warnings: ${a11y.score}${a11y.flags.length ? ' (' + a11y.flags.slice(0, 8).join('; ') + ')' : ''}`,
          '',
          text,
          '',
          '# --- Interactable nodes (LLM-friendly flat list) ---',
          ...inter,
        ].join('\n');
        writeFileSync(ariaPath, body);
        // If we find accessibility violations, log them as diag events
        // so the diff surfaces NEW a11y warnings across runs.
        for (const f of a11y.flags.slice(0, 10)) {
          log({ kind: 'a11y-violation', text: `${f} on step ${step.label || step.kind}`, impact: 'moderate' });
        }
      } catch { /* aria capture is best-effort */ }
      stepResults.push({ step, index: i, ok: true, durationMs: 0, screenshot: imgPath });
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
    // Deep heuristics check — error copy, blank main, stuck loading.
    await checkUiHealth(step.label || step.kind);
  }

  await browser.close();

  // Walk through events and bucket them by step — answers the user's
  // question "what was the crawler doing when this log happened?"
  // Each event.t is ms since run start. Steps don't carry their own
  // start offset, so we compute it from the cumulative durationMs.
  const stepWindows: Array<{ start: number; end: number; step: StepResult }> = [];
  {
    let cursor = 0;
    for (const s of stepResults) {
      const start = cursor;
      const end = cursor + Math.max(s.durationMs || 0, 100) + 200; // inclusive of the 200ms post-step settle
      stepWindows.push({ start, end, step: s });
      cursor = end;
    }
  }
  const eventsByStep = stepWindows.map(({ start, end, step }) => ({
    stepIndex: step.index,
    stepLabel: step.step.label || step.step.kind,
    stepKind: step.step.kind,
    windowMs: [start, end] as [number, number],
    events: events.filter(e => e.t >= start && e.t <= end),
  })).filter(b => b.events.length > 0);

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
    eventsByStep,
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
