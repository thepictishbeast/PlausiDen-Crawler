/**
 * Journey — a scripted sequence of interactions the crawler runs against
 * a target URL. Each step declares what to click/wait/type/assert, and
 * the runner records events around it.
 *
 * Simple plain-data format so the same journey file can be authored by
 * hand, generated from a test recorder, or checked into a repo for
 * regression diffing.
 */
import type { Page } from 'playwright';

export type StepKind =
  | 'goto'
  | 'click'
  | 'fill'
  | 'type'
  | 'press'
  | 'wait'
  | 'waitForSelector'
  | 'screenshot'
  | 'assertText'
  | 'scroll'
  | 'reload'
  | 'discover'
  | 'probe';

export interface Step {
  kind: StepKind;
  /** CSS selector for click/fill/type/waitForSelector/assertText. */
  selector?: string;
  /** Text value for fill/type/assertText. */
  text?: string;
  /** Key for press (e.g. 'Enter', 'Escape', 'Tab'). */
  key?: string;
  /** URL for goto. */
  url?: string;
  /** Label used in the report and as screenshot filename prefix. */
  label?: string;
  /** Per-step timeout override (ms). Defaults to 10_000. */
  timeout?: number;
  /** For wait: ms to sleep. */
  ms?: number;
  /** For scroll: vertical pixel delta (negative scrolls up). */
  dy?: number;
  /**
   * For discover: BFS configuration. The runner switches to autonomous
   * mode and crawls same-origin links from the current URL (or step.url
   * if provided), capturing aria + screenshot + interactables on every
   * page reached. See src/discover.ts for the full DiscoverConfig shape.
   */
  discover?: {
    maxPages?: number;
    maxDepth?: number;
    sameOrigin?: boolean;
    includePatterns?: string[];
    denyPatterns?: string[];
    interactButtons?: 'never' | 'safe-buttons' | 'all-buttons';
    settleMs?: number;
    navTimeoutMs?: number;
  };
  /**
   * For probe: adversarial URL-parameter mutation. Takes URLs (explicit
   * + harvested from a prior discover step's discover-pages.json) and
   * rewrites int/hex/UUID-shaped path segments into malformed variants
   * (null bytes, oversize, traversal, type-confusion). 5xx responses,
   * unexpected 200s on garbage input, body echoes of injected payloads,
   * and partial-entropy hash leaks become CapturedEvent findings. See
   * src/probe.ts for the full ProbeConfig shape.
   */
  probe?: {
    urls?: string[];
    mutators?: Array<'int' | 'hex' | 'uuid' | 'string'>;
    maxRequestsPerUrl?: number;
    maxRequestsTotal?: number;
    timeoutMs?: number;
    failStatuses?: number[];
    okStatuses?: number[];
    hashLeakPrefixes?: string[];
    includePatterns?: string[];
    denyPatterns?: string[];
    inheritDiscoverUrls?: boolean;
  };
}

export interface Journey {
  name: string;
  description?: string;
  /** Baseline URL — goto steps without a url prefix use this. */
  baseUrl: string;
  steps: Step[];
  /**
   * Path to a Playwright storageState JSON (cookies + localStorage) captured
   * from a prior interactive login. Loaded into the browser context before
   * any step runs, so authenticated journeys can crawl admin/voter views
   * without ever inserting fake credentials into the prod database.
   */
  storageState?: string;
}

export interface StepResult {
  step: Step;
  index: number;
  ok: boolean;
  durationMs: number;
  error?: string;
  screenshot?: string;
}

export async function runStep(page: Page, step: Step, timeout = 10_000): Promise<StepResult> {
  const start = Date.now();
  const out: StepResult = { step, index: 0, ok: true, durationMs: 0 };
  try {
    switch (step.kind) {
      case 'goto':
        // PlausiDen's SPA keeps WebSocket reconnection attempts + long-
        // polling open, so 'networkidle' never fires → 10s timeout on
        // every goto. Use 'domcontentloaded' (fires when the HTML is
        // parsed) which is what we actually want: the page is loaded,
        // subsequent steps wait for specific selectors anyway.
        await page.goto(step.url || '', { waitUntil: 'domcontentloaded', timeout: step.timeout || timeout });
        break;
      case 'click':
        if (!step.selector) throw new Error('click: missing selector');
        await page.click(step.selector, { timeout: step.timeout || timeout });
        break;
      case 'fill':
        if (!step.selector) throw new Error('fill: missing selector');
        await page.fill(step.selector, step.text || '', { timeout: step.timeout || timeout });
        break;
      case 'type':
        if (!step.selector) throw new Error('type: missing selector');
        await page.type(step.selector, step.text || '', { delay: 30 });
        break;
      case 'press':
        if (!step.key) throw new Error('press: missing key');
        if (step.selector) await page.press(step.selector, step.key);
        else await page.keyboard.press(step.key);
        break;
      case 'wait':
        await page.waitForTimeout(step.ms ?? 500);
        break;
      case 'waitForSelector':
        if (!step.selector) throw new Error('waitForSelector: missing selector');
        await page.waitForSelector(step.selector, { timeout: step.timeout || timeout });
        break;
      case 'screenshot':
        // handled by runner so it can track the filename
        break;
      case 'assertText':
        if (!step.selector) throw new Error('assertText: missing selector');
        const actual = await page.textContent(step.selector, { timeout: step.timeout || timeout });
        if (!actual?.includes(step.text || '')) {
          throw new Error(`assertText: "${step.text}" not found in ${step.selector}`);
        }
        break;
      case 'scroll':
        await page.evaluate((dy: number) => window.scrollBy(0, dy), step.dy ?? 500);
        break;
      case 'reload':
        await page.reload({ waitUntil: 'domcontentloaded' });
        break;
    }
  } catch (e: any) {
    out.ok = false;
    out.error = String(e?.message || e);
  } finally {
    out.durationMs = Date.now() - start;
  }
  return out;
}
