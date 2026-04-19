/**
 * PlausiDen-Crawler — headless-browser audit entry point.
 *
 * Usage:
 *   npm run audit -- --url http://10.99.0.3:3000/ [--journey <name>]
 *
 * Output: runs/<iso-ts>/report.json + per-step PNGs.
 * Non-zero exit if console errors OR failed fetches exceed budget.
 */
import { chromium, type Page, type ConsoleMessage, type Request } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

interface CapturedEvent {
  t: number;                 // ms since run start
  kind: 'console' | 'pageerror' | 'request-failed' | 'response-error' | 'csp-violation';
  level?: string;
  text: string;
  url?: string;
  status?: number;
  stack?: string;
}

interface Budget {
  consoleErrors: number;
  failedRequests: number;
}

const DEFAULT_BUDGET: Budget = { consoleErrors: 5, failedRequests: 5 };

async function run(targetUrl: string, journey: string = 'smoke'): Promise<number> {
  const tsTag = new Date().toISOString().replace(/[:.]/g, '-');
  const outDir = join('runs', tsTag);
  mkdirSync(outDir, { recursive: true });

  const start = Date.now();
  const events: CapturedEvent[] = [];
  const log = (e: Omit<CapturedEvent, 't'>) => events.push({ ...e, t: Date.now() - start });

  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();

  page.on('console', (msg: ConsoleMessage) => {
    log({ kind: 'console', level: msg.type(), text: msg.text(), url: msg.location().url });
  });
  page.on('pageerror', (err) => {
    log({ kind: 'pageerror', text: err.message, stack: err.stack });
  });
  page.on('requestfailed', (req: Request) => {
    log({ kind: 'request-failed', text: req.failure()?.errorText || 'unknown', url: req.url() });
  });
  page.on('response', async (res) => {
    if (res.status() >= 400) {
      log({ kind: 'response-error', text: res.statusText(), url: res.url(), status: res.status() });
    }
  });

  console.log(`[crawler] navigating ${targetUrl} (journey=${journey})`);
  try {
    await page.goto(targetUrl, { waitUntil: 'networkidle', timeout: 30_000 });
  } catch (e: any) {
    log({ kind: 'pageerror', text: `navigate failed: ${e.message || e}` });
  }

  // Stub journey — future: load from journeys/${journey}.ts and execute
  // a sequence of click/fill/assert actions.
  await page.waitForTimeout(2000);
  const snap1 = join(outDir, '01-landing.png');
  await page.screenshot({ path: snap1, fullPage: true }).catch(() => undefined);

  // Smoke: try clicking each [role=button] visible in the viewport.
  const buttons = await page.$$('[role="button"], button');
  console.log(`[crawler] found ${buttons.length} button(s)`);
  for (let i = 0; i < Math.min(buttons.length, 10); i++) {
    try {
      await buttons[i].click({ timeout: 2000, trial: false }).catch(() => undefined);
      await page.waitForTimeout(400);
    } catch { /* ignore */ }
  }
  const snap2 = join(outDir, '02-post-clicks.png');
  await page.screenshot({ path: snap2, fullPage: true }).catch(() => undefined);

  await browser.close();

  const consoleErrors = events.filter(e => e.kind === 'console' && e.level === 'error').length;
  const pageErrors = events.filter(e => e.kind === 'pageerror').length;
  const failedRequests = events.filter(e => e.kind === 'request-failed' || e.kind === 'response-error').length;

  const report = {
    target: targetUrl,
    journey,
    started: new Date(start).toISOString(),
    durationMs: Date.now() - start,
    counts: { consoleErrors, pageErrors, failedRequests, total: events.length },
    events,
    screenshots: [snap1, snap2],
  };
  writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));

  console.log(`\n[crawler] report: ${outDir}/report.json`);
  console.log(`  console errors: ${consoleErrors} (budget ${DEFAULT_BUDGET.consoleErrors})`);
  console.log(`  page errors:    ${pageErrors}`);
  console.log(`  failed fetches: ${failedRequests} (budget ${DEFAULT_BUDGET.failedRequests})`);

  const overBudget = consoleErrors > DEFAULT_BUDGET.consoleErrors
    || failedRequests > DEFAULT_BUDGET.failedRequests
    || pageErrors > 0;
  return overBudget ? 1 : 0;
}

// Parse args.
const args = process.argv.slice(2);
const urlIdx = args.indexOf('--url');
const journeyIdx = args.indexOf('--journey');
const url = urlIdx >= 0 ? args[urlIdx + 1] : 'http://localhost:3000/';
const journey = journeyIdx >= 0 ? args[journeyIdx + 1] : 'smoke';

run(url, journey).then((code) => process.exit(code)).catch((e) => {
  console.error('[crawler] fatal:', e);
  process.exit(2);
});
