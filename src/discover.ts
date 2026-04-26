/**
 * Autonomous site-discovery crawler.
 *
 * The journey runner only does what its JSON tells it to. For exhaustive
 * audits ("hit every page, every button, every input field") we need an
 * autonomous mode: BFS through same-origin links from a starting URL,
 * capturing aria + screenshot + console events on every page reached,
 * and recording the full list of buttons + inputs + forms found on each
 * page so a human (or LLM) can review what surfaces were touched.
 *
 * Read-only by default — never submits forms, never clicks buttons that
 * look state-mutating ("delete", "remove", "submit", "sign out", etc.).
 * The HARD `feedback_no_test_data_in_prod` rule applies to anything we
 * point this at: prod must be safe to crawl.
 *
 * Outputs feed back into the same Report.events / page-level captures
 * the journey runner already produces, so reports diff cleanly across
 * runs regardless of whether they came from a scripted journey or an
 * autonomous discover sweep.
 */
import type { Page } from 'playwright';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { captureAriaTree, ariaTreeToText, interactableNodes, scoreAriaTree, type AriaNode } from './aria.js';
import type { CapturedEvent } from './report.js';
import type { StepResult } from './journey.js';
import { runAxe, axeEventsFor, renderAxeFindings, annotateViolations, type AxePageResult } from './audit.js';

export interface DiscoverConfig {
  /** Max pages to visit total. Hard cap to bound runtime + report size. */
  maxPages?: number;
  /** Max link depth from start. 0 = start page only, 1 = + immediate children, etc. */
  maxDepth?: number;
  /** Restrict to same eTLD+1 (default true). False = follow off-site links too (rare). */
  sameOrigin?: boolean;
  /** Allowlist URL regex patterns. If set, only URLs matching one of these are visited. */
  includePatterns?: string[];
  /** Denylist URL regex patterns. URLs matching any are skipped. Always applied. */
  denyPatterns?: string[];
  /**
   * Interaction policy:
   *   - 'never' (default): no clicks beyond navigation. Pure read-only.
   *   - 'safe-buttons': click buttons whose accessible name matches a
   *     read-only allowlist (toggle, expand, show, view, filter, sort,
   *     next, previous, etc.). Skip any name matching the destructive
   *     denylist (delete, remove, submit, sign out, log out, save,
   *     publish, send, pay, charge, etc.).
   *   - 'all-buttons': click every button. ONLY for trusted dev/staging.
   */
  interactButtons?: 'never' | 'safe-buttons' | 'all-buttons';
  /** Wait this long after navigation before snapshot (ms). */
  settleMs?: number;
  /** Request timeout for goto (ms). */
  navTimeoutMs?: number;
}

export interface DiscoveredPage {
  /** Final URL after redirects. */
  url: string;
  /** Depth from start URL. */
  depth: number;
  /** HTTP status from the navigation response. */
  status?: number;
  /** Aria-tree text snapshot. */
  ariaText?: string;
  /** Interactable elements found (role + name). */
  interactables: Array<{ role: string; name: string }>;
  /** Form-field selectors found (CSS or aria locator). */
  inputs: Array<{ selector: string; type: string; name?: string; placeholder?: string; required?: boolean }>;
  /** Internal links found, before deduplication. */
  outgoingLinks: string[];
  /** Screenshot path (relative to outDir). */
  screenshot?: string;
  /** A11y warnings (one line each). */
  a11yFlags: string[];
  /** Buttons clicked (when interactButtons is enabled). */
  clickedButtons: string[];
  /** Errors encountered during discovery of this page. */
  errors: string[];
  /** axe-core scan result (full violation list + per-node selectors). */
  axe?: AxePageResult;
}

export interface DiscoverResult {
  pages: DiscoveredPage[];
  /** Same-shape StepResult per visited URL so downstream report code groups them. */
  stepResults: StepResult[];
  /** All events captured during discovery (will be merged into the global events list). */
  events: CapturedEvent[];
}

const DESTRUCTIVE_NAME = /\b(delete|remove|destroy|drop|purge|wipe|reset|submit|send|pay|charge|publish|finalize|finalise|sign\s*out|log\s*out|sign\s*in|log\s*in|register|create|enroll|enrol|book|buy|order|confirm|approve|reject|deny|accept|vote|cast|continue|next|save|update|edit)\b/i;
const SAFE_NAME = /\b(toggle|expand|collapse|show|hide|view|open|close|menu|filter|sort|tab|switch|select|search|preview|details|more|less|info|help|about|home|back|return|cancel)\b/i;

function isSameOrigin(a: string, b: string): boolean {
  try { return new URL(a).origin === new URL(b).origin; } catch { return false; }
}

function normalizeUrl(raw: string, base: string): string | null {
  try {
    const u = new URL(raw, base);
    u.hash = '';
    // Strip trailing slash for the comparison key, except root.
    if (u.pathname.length > 1 && u.pathname.endsWith('/')) u.pathname = u.pathname.replace(/\/+$/, '');
    return u.toString();
  } catch { return null; }
}

function matchesAny(url: string, patterns?: string[]): boolean {
  if (!patterns || patterns.length === 0) return false;
  for (const p of patterns) {
    try { if (new RegExp(p).test(url)) return true; } catch { /* skip malformed regex */ }
  }
  return false;
}

/**
 * Snapshot every interactable + every form input on the current page.
 * Uses both the aria tree (for role/name) and a DOM scrape (for selectors
 * that survive React re-renders better than the aria index).
 */
async function snapshotPage(page: Page): Promise<{
  interactables: Array<{ role: string; name: string }>;
  inputs: Array<{ selector: string; type: string; name?: string; placeholder?: string; required?: boolean }>;
  buttonsToClick: string[];
  a11yFlags: string[];
}> {
  const tree = await captureAriaTree(page);
  const interactables = interactableNodes(tree).map(n => ({
    role: n.role,
    name: (n.name || '').slice(0, 200),
  }));
  const a11y = scoreAriaTree(tree);

  const dom = await page.evaluate(() => {
    const cssEscape = (s: string) =>
      // crude CSS.escape polyfill — quotes single quotes for attribute selectors
      s.replace(/'/g, "\\'").replace(/\n/g, ' ');

    const inputs: Array<{ selector: string; type: string; name?: string; placeholder?: string; required?: boolean }> = [];
    document.querySelectorAll('input, textarea, select').forEach((el) => {
      const e = el as HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;
      let selector = e.tagName.toLowerCase();
      if (e.id) selector = `#${e.id}`;
      else if ((e as HTMLInputElement).name) selector += `[name='${cssEscape((e as HTMLInputElement).name)}']`;
      else if ((e as HTMLInputElement).placeholder) selector += `[placeholder='${cssEscape((e as HTMLInputElement).placeholder)}']`;
      inputs.push({
        selector,
        type: ((e as HTMLInputElement).type || e.tagName.toLowerCase()),
        name: (e as HTMLInputElement).name || undefined,
        placeholder: (e as HTMLInputElement).placeholder || undefined,
        required: (e as HTMLInputElement).required || undefined,
      });
    });

    // Candidate buttons for interaction. We collect *every* visible
    // button-shaped element with an accessible name; the runner filters
    // by interactButtons policy.
    const buttons: string[] = [];
    document.querySelectorAll('button, [role="button"], [role="tab"], [role="menuitem"]').forEach((el) => {
      const e = el as HTMLElement;
      if (!e.offsetParent && e.tagName !== 'BODY') return; // not visible
      const name = (e.getAttribute('aria-label') || e.innerText || '').trim();
      if (!name) return;
      // Build a stable selector preference: id > data-testid > aria-label > role+name
      let sel = '';
      if (e.id) sel = `#${e.id}`;
      else if (e.getAttribute('data-testid')) sel = `[data-testid='${cssEscape(e.getAttribute('data-testid')!)}']`;
      else if (e.getAttribute('aria-label')) sel = `[aria-label='${cssEscape(e.getAttribute('aria-label')!)}']`;
      else sel = `${e.tagName.toLowerCase()}:has-text('${cssEscape(name.slice(0, 60))}')`;
      buttons.push(`${sel}|${name.slice(0, 80)}`);
    });

    return { inputs, buttons };
  }).catch(() => ({ inputs: [], buttons: [] }));

  return {
    interactables,
    inputs: dom.inputs,
    buttonsToClick: dom.buttons,
    a11yFlags: a11y.flags.slice(0, 10),
  };
}

/**
 * Collect same-origin links from the current page, with hash + tracking
 * params normalized away. Returns absolute URLs.
 */
async function collectLinks(page: Page, baseUrl: string): Promise<string[]> {
  const hrefs = await page.evaluate(() => {
    const out: string[] = [];
    document.querySelectorAll('a[href]').forEach(a => {
      const href = (a as HTMLAnchorElement).href;
      if (href) out.push(href);
    });
    return out;
  }).catch(() => [] as string[]);
  const norm = new Set<string>();
  for (const h of hrefs) {
    if (h.startsWith('javascript:') || h.startsWith('mailto:') || h.startsWith('tel:')) continue;
    const u = normalizeUrl(h, baseUrl);
    if (u) norm.add(u);
  }
  return Array.from(norm);
}

export async function runDiscover(
  page: Page,
  startUrl: string,
  config: DiscoverConfig,
  outDir: string,
  startEpoch: number,
  log: (e: Omit<CapturedEvent, 't'>) => void,
): Promise<DiscoverResult> {
  const maxPages = config.maxPages ?? 50;
  const maxDepth = config.maxDepth ?? 3;
  const sameOrigin = config.sameOrigin !== false;
  const settleMs = config.settleMs ?? 600;
  const navTimeout = config.navTimeoutMs ?? 20_000;
  const interactPolicy = config.interactButtons || 'never';

  const visited = new Set<string>();
  const pages: DiscoveredPage[] = [];
  const stepResults: StepResult[] = [];
  const events: CapturedEvent[] = [];

  const queue: Array<{ url: string; depth: number }> = [];
  const startNorm = normalizeUrl(startUrl, startUrl);
  if (!startNorm) {
    log({ kind: 'pageerror', text: `discover: malformed start URL: ${startUrl}` });
    return { pages, stepResults, events };
  }
  queue.push({ url: startNorm, depth: 0 });

  let virtualIndex = 1000; // offset so discover-step indices don't collide with journey steps

  while (queue.length > 0 && pages.length < maxPages) {
    const { url, depth } = queue.shift()!;
    if (visited.has(url)) continue;
    if (depth > maxDepth) continue;
    if (sameOrigin && !isSameOrigin(url, startNorm)) continue;
    if (matchesAny(url, config.denyPatterns)) {
      log({ kind: 'console', level: 'info', text: `discover: skip (denylist) ${url}` });
      continue;
    }
    if (config.includePatterns && config.includePatterns.length > 0 && !matchesAny(url, config.includePatterns)) {
      continue;
    }
    visited.add(url);

    const stepStart = Date.now();
    const errors: string[] = [];
    let status: number | undefined;
    let screenshot: string | undefined;
    let snap: Awaited<ReturnType<typeof snapshotPage>> = {
      interactables: [], inputs: [], buttonsToClick: [], a11yFlags: [],
    };
    const clickedButtons: string[] = [];
    let outgoing: string[] = [];

    try {
      const resp = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: navTimeout });
      status = resp?.status();
      await page.waitForTimeout(settleMs);

      // Capture aria + screenshot.
      const slug = url.replace(/^https?:\/\//, '').replace(/[^A-Za-z0-9]/g, '_').slice(0, 80);
      const base = `discover-${String(pages.length + 1).padStart(3, '0')}-${slug}`;
      try {
        screenshot = join(outDir, `${base}.png`);
        await page.screenshot({ path: screenshot, fullPage: true });
      } catch (e: any) {
        errors.push(`screenshot: ${e?.message || e}`);
      }
      try {
        const tree = await captureAriaTree(page);
        const text = ariaTreeToText(tree);
        writeFileSync(join(outDir, `${base}.aria.txt`), `# ${url}\n# depth=${depth}\n\n${text}`);
      } catch (e: any) {
        errors.push(`aria: ${e?.message || e}`);
      }

      snap = await snapshotPage(page);
      outgoing = await collectLinks(page, url);

      // Real WCAG checks via axe-core. This is in addition to the
      // homegrown scoreAriaTree heuristic — axe catches contrast,
      // ARIA misuse, label/name mismatches, etc. that the aria walk
      // can't see. We also write an annotated screenshot per page
      // (red outlines on every flagged element) so the user can see
      // *where* each issue is at a glance.
      try {
        const axeResult = await runAxe(page);
        (snap as any).__axe = axeResult;
        for (const ev of axeEventsFor(axeResult, startEpoch)) events.push(ev);
        if (!axeResult.ok) errors.push(`axe: ${axeResult.error}`);
        if (axeResult.ok && axeResult.violations.length > 0) {
          const annPath = join(outDir, `discover-${String(pages.length + 1).padStart(3, '0')}-${url.replace(/^https?:\/\//, '').replace(/[^A-Za-z0-9]/g, '_').slice(0, 80)}.annotated.png`);
          (snap as any).__annotated = await annotateViolations(page, axeResult, annPath);
        }
      } catch (e: any) {
        errors.push(`axe: ${e?.message || e}`);
      }

      // Optional: click read-only-safe buttons (stay on the same URL).
      if (interactPolicy !== 'never') {
        for (const b of snap.buttonsToClick) {
          const [sel, nameRaw] = b.split('|');
          const name = nameRaw || '';
          if (DESTRUCTIVE_NAME.test(name)) continue;
          if (interactPolicy === 'safe-buttons' && !SAFE_NAME.test(name)) continue;
          const beforeUrl = page.url();
          try {
            await page.click(sel, { timeout: 2_000 });
            await page.waitForTimeout(200);
            const afterUrl = page.url();
            clickedButtons.push(`${name} → ${afterUrl === beforeUrl ? 'no-nav' : 'nav:' + afterUrl}`);
            // If the click navigated, return to the discover URL so the
            // next button starts from the same baseline.
            if (afterUrl !== beforeUrl) {
              await page.goto(url, { waitUntil: 'domcontentloaded', timeout: navTimeout });
              await page.waitForTimeout(settleMs);
            }
          } catch (e: any) {
            errors.push(`click '${name}': ${e?.message || e}`);
          }
        }
      }
    } catch (e: any) {
      errors.push(`goto: ${e?.message || e}`);
    }

    pages.push({
      url, depth, status,
      ariaText: undefined,
      interactables: snap.interactables,
      inputs: snap.inputs,
      outgoingLinks: outgoing,
      screenshot,
      a11yFlags: snap.a11yFlags,
      clickedButtons,
      errors,
      axe: (snap as any).__axe as AxePageResult | undefined,
      annotated: (snap as any).__annotated as string | undefined,
    } as DiscoveredPage & { annotated?: string });

    stepResults.push({
      step: { kind: 'goto', url, label: `discover[d=${depth}] ${url}` },
      index: virtualIndex++,
      ok: errors.length === 0,
      durationMs: Date.now() - stepStart,
      error: errors[0],
      screenshot,
    });

    // A11y warnings as events so the diff catches new ones.
    for (const f of snap.a11yFlags.slice(0, 5)) {
      events.push({ kind: 'a11y-violation', text: `${f} on ${url}`, impact: 'moderate', t: Date.now() - startEpoch });
    }
    // Non-2xx responses as events.
    if (status && status >= 400) {
      events.push({ kind: 'response-error', text: `HTTP ${status} on ${url}`, url, status, t: Date.now() - startEpoch });
    }
    for (const e of errors) {
      events.push({ kind: 'pageerror', text: `discover ${url}: ${e}`, t: Date.now() - startEpoch });
    }

    // Enqueue same-origin children.
    for (const link of outgoing) {
      if (visited.has(link)) continue;
      if (sameOrigin && !isSameOrigin(link, startNorm)) continue;
      if (matchesAny(link, config.denyPatterns)) continue;
      if (config.includePatterns && config.includePatterns.length > 0 && !matchesAny(link, config.includePatterns)) continue;
      queue.push({ url: link, depth: depth + 1 });
    }

    console.log(`[crawler] discover ${pages.length}/${maxPages} d=${depth} status=${status ?? 'n/a'} interactables=${snap.interactables.length} inputs=${snap.inputs.length} new-links=${outgoing.length} ${url}`);
  }

  // Write a per-run discovery summary (independent of report.json so it
  // can be skimmed without parsing the full JSON).
  const summary = [
    `# Discovery summary`,
    `# Start: ${startUrl}`,
    `# Pages visited: ${pages.length}`,
    `# Max-depth: ${maxDepth}`,
    `# Max-pages: ${maxPages}`,
    `# Interaction: ${interactPolicy}`,
    ``,
    ...pages.map(p =>
      `- [${p.status ?? '???'}] depth=${p.depth} ${p.url}\n` +
      `    interactables: ${p.interactables.length}, inputs: ${p.inputs.length}, links: ${p.outgoingLinks.length}, errors: ${p.errors.length}`
    ),
  ].join('\n');
  try { writeFileSync(join(outDir, 'discover-summary.txt'), summary); } catch { /* ignore */ }

  // Per-page axe findings, one block per URL. This is what the user
  // triages: rule id, impact, selector, failure summary. Cheap to read,
  // cheap to skim, cheap to diff between runs.
  try {
    const findings = renderAxeFindings(
      pages.filter(p => p.axe).map(p => ({
        url: p.url,
        result: p.axe!,
        annotated: (p as any).annotated,
      }))
    );
    writeFileSync(join(outDir, 'discover-findings.txt'), findings);
  } catch { /* ignore */ }

  return { pages, stepResults, events };
}
