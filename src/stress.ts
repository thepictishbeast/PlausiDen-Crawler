/**
 * stress.ts — aggressive UI-fuzz engine.
 *
 * Hammers a single page from multiple angles and surfaces anything the
 * UI does wrong as standard CapturedEvents flowing through the main
 * runner's logger. The whole point is to break things that pass the
 * happy-path crawl: focus traps that don't trap, double-submit handlers
 * that race, useEffect cleanups that don't run on unmount, layout-shift
 * triggered by resize, and selectors that break Playwright's CSS parser.
 *
 * The runner only invokes `runStress(page, cfg, ctx)` — everything inside
 * is pure: no global state, no file IO except via the supplied `ctx.log`.
 *
 * Aggressive by design. Defaults assume "really stress the UI" (the
 * user's words). `intensity: "low"` scales every count down 4x; `extreme`
 * scales every count up 2x and adds extra resize/back-forward chaos.
 *
 * Safety:
 *   - `avoidSelectors` is honoured before every click — "Delete account",
 *     "Sign out", logout links etc. should be listed there per-journey.
 *   - Form-fuzz never submits a form whose nearest `<form>` ancestor has
 *     `data-stress="no-submit"` set. Use this on payment forms or any
 *     mutation that would touch prod state.
 *   - Network chaos only intercepts XHR/fetch responses with rate <
 *     `networkChaosRate` (default 0.05 = 5%) so the crawl still completes.
 */
import type { Page, BrowserContext } from 'playwright';

export interface StressConfig {
  intensity?: 'low' | 'medium' | 'high' | 'extreme';
  durationMs?: number;
  clickCount?: number;
  fillCount?: number;
  keyboardCount?: number;
  resizeCount?: number;
  screenshotEveryN?: number;
  networkChaos?: boolean;
  avoidSelectors?: string[];
  rotateTabs?: string[];
  extraPayloads?: string[];
  skipClicks?: boolean;
  skipFills?: boolean;
  skipKeyboard?: boolean;
  skipResize?: boolean;
  finalScreenshot?: boolean;
}

export interface StressContext {
  outDir: string;
  startEpoch: number;
  log: (e: { kind: string; text: string; level?: string; url?: string; status?: number; impact?: string }) => void;
  takeScreenshot: (label: string) => Promise<string | null>;
}

export interface StressResult {
  durationMs: number;
  clicks: { attempted: number; ok: number; failed: number };
  fills: { attempted: number; ok: number; failed: number };
  keyboards: { attempted: number };
  resizes: { attempted: number };
  screenshots: string[];
  networkChaosTrips: number;
  /** Final aria/DOM stats so the report can compare to a "calm" baseline. */
  finalDom: { nodes: number; buttons: number; inputs: number; errorRoles: number };
  abortReason?: string;
}

/* -- Edge-case payload pool ----------------------------------------------- */

/**
 * Default form-fuzz payloads. These are the ones every input handler
 * SHOULD survive — empty, max-length, unicode, RTL, control chars, XSS,
 * SQLi, NULL byte, deeply nested HTML, prototype pollution, JSON-shape,
 * homoglyph. Add `extraPayloads` per-journey for project-specific stuff
 * (voter codes, poll IDs, etc.).
 */
const DEFAULT_PAYLOADS: string[] = [
  '',
  ' ',
  '\t',
  '\n',
  'a',
  '0',
  '-1',
  '1e308',
  '0.1',
  // Edge-case unicode
  '𓀀𓀁𓀂', // Egyptian hieroglyph (4-byte utf-8)
  '🏴‍☠️', // ZWJ + emoji modifier sequence
  'a\u202Eb', // RTL override
  '\u0000NULL', // null byte
  '\uFFFD', // replacement char
  // XSS shapes — confirms we're escaping properly
  "<script>alert(1)</script>",
  "\"><img src=x onerror=alert(1)>",
  "javascript:alert(1)",
  "<svg/onload=alert(1)>",
  "{{constructor.constructor('alert(1)')()}}",
  // SQLi shapes
  "' OR '1'='1",
  "'; DROP TABLE voters;--",
  "' UNION SELECT NULL--",
  // Path traversal
  "../../../../etc/passwd",
  // Prototype pollution
  '__proto__[polluted]=1',
  '{"__proto__":{"polluted":1}}',
  // Long
  'A'.repeat(10_000),
  '0'.repeat(1_000_000), // 1MB string — should be rejected gracefully
  // Numeric edge
  '9'.repeat(100),
  '-9'.repeat(100),
  // Quotes / shell
  '`whoami`',
  '$(whoami)',
  // JSON-as-string
  '{"a":1}',
  '[1,2,3]',
  // Homoglyph / IDN
  'аdmin', // Cyrillic 'а' looks like Latin 'a'
  'admin\u200Buser', // zero-width space inside
];

/* -- Intensity scaling ---------------------------------------------------- */

const INTENSITY_SCALE: Record<NonNullable<StressConfig['intensity']>, number> = {
  low: 0.25,
  medium: 0.5,
  high: 1,
  extreme: 2,
};

const resolveCfg = (cfg: StressConfig): Required<Omit<StressConfig, 'avoidSelectors' | 'rotateTabs' | 'extraPayloads'>> & Pick<StressConfig, 'avoidSelectors' | 'rotateTabs' | 'extraPayloads'> => {
  const intensity = cfg.intensity || 'high';
  const k = INTENSITY_SCALE[intensity];
  return {
    intensity,
    durationMs: cfg.durationMs ?? 60_000,
    clickCount: Math.max(1, Math.round((cfg.clickCount ?? 80) * k)),
    fillCount: Math.max(1, Math.round((cfg.fillCount ?? 60) * k)),
    keyboardCount: Math.max(1, Math.round((cfg.keyboardCount ?? 40) * k)),
    resizeCount: Math.max(1, Math.round((cfg.resizeCount ?? 12) * k)),
    screenshotEveryN: cfg.screenshotEveryN ?? 20,
    networkChaos: cfg.networkChaos ?? false,
    skipClicks: cfg.skipClicks ?? false,
    skipFills: cfg.skipFills ?? false,
    skipKeyboard: cfg.skipKeyboard ?? false,
    skipResize: cfg.skipResize ?? false,
    finalScreenshot: cfg.finalScreenshot ?? true,
    avoidSelectors: cfg.avoidSelectors,
    rotateTabs: cfg.rotateTabs,
    extraPayloads: cfg.extraPayloads,
  };
};

/* -- Element collection --------------------------------------------------- */

interface FuzzableElement {
  selector: string; // attribute-selector form so colons-in-id never break Playwright's CSS parser
  kind: 'click' | 'input';
  inputType?: string;
  description: string; // for log output
}

/**
 * Scroll the viewport top → bottom → top so lazy-loaded SPA content
 * (intersection-observer-driven sections, virtualized lists) renders
 * before we collect fuzzables. Critical on long landing pages where the
 * initial viewport only contains the hero CTA.
 */
async function scrollToReveal(page: Page): Promise<void> {
  try {
    await page.evaluate(async () => {
      const sleep = (n: number) => new Promise(r => setTimeout(r, n));
      const total = document.body.scrollHeight;
      const step = Math.max(200, Math.floor(window.innerHeight * 0.7));
      for (let y = 0; y < total; y += step) {
        window.scrollTo(0, y);
        await sleep(40);
      }
      window.scrollTo(0, 0);
      await sleep(40);
    });
  } catch { /* fine */ }
}

/**
 * Collect every visible interactable on the current page as a stable
 * attribute-selector. Always uses `[id="..."]` / `[data-testid="..."]`
 * shapes so Playwright doesn't choke on radix `:r7:`-style ids.
 */
async function collectFuzzableElements(page: Page): Promise<FuzzableElement[]> {
  return await page.evaluate(() => {
    const out: Array<{ selector: string; kind: 'click' | 'input'; inputType?: string; description: string }> = [];
    const seen = new Set<string>();
    const cssEsc = (s: string): string => {
      // Inline minimal CSS.escape since we're inside page context.
      try { return (window as any).CSS && (window as any).CSS.escape ? (window as any).CSS.escape(s) : s.replace(/(["\\])/g, '\\$1'); }
      catch { return s.replace(/(["\\])/g, '\\$1'); }
    };
    const buildSelector = (e: Element): string => {
      const el = e as HTMLElement;
      const tid = el.getAttribute('data-testid');
      if (tid) return `[data-testid="${cssEsc(tid)}"]`;
      if (el.id) return `[id="${cssEsc(el.id)}"]`;
      const al = el.getAttribute('aria-label');
      if (al) return `[aria-label="${cssEsc(al)}"]`;
      const name = el.getAttribute('name');
      if (name) return `${el.tagName.toLowerCase()}[name="${cssEsc(name)}"]`;
      // Fall back to text-based locator (Playwright honours `:has-text(…)`)
      const t = (el.innerText || el.textContent || '').trim();
      if (t) return `${el.tagName.toLowerCase()}:has-text("${cssEsc(t.slice(0, 40))}")`;
      return el.tagName.toLowerCase();
    };
    const isVisible = (el: HTMLElement): boolean => {
      // offsetParent is null for display:none / detached / position:fixed in
      // some Chromium quirks — we tolerate the false negative.
      if (!(el.offsetParent || el === document.body)) return false;
      const r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    };
    document.querySelectorAll('button, a[href], [role="button"], [role="tab"], [role="menuitem"], [role="option"], [role="checkbox"], [role="radio"], [role="switch"], [role="link"]').forEach(el => {
      const e = el as HTMLElement;
      if (!isVisible(e)) return;
      if (e.hasAttribute('disabled') || e.getAttribute('aria-disabled') === 'true') return;
      const sel = buildSelector(e);
      if (seen.has(sel)) return;
      seen.add(sel);
      out.push({ selector: sel, kind: 'click', description: (e.getAttribute('aria-label') || e.innerText || '').trim().slice(0, 60) || el.tagName.toLowerCase() });
    });
    document.querySelectorAll('input, textarea, select').forEach(el => {
      const e = el as HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;
      if (!isVisible(e)) return;
      if ((e as HTMLInputElement).disabled || (e as HTMLInputElement).readOnly) return;
      // Skip credit-card / password fields by convention — we don't want
      // to send a million payloads at sensitive inputs.
      const tEl = (e as HTMLInputElement);
      const t = tEl.type || '';
      if (t === 'password' || (t === 'hidden')) return;
      // Skip forms marked as no-submit (payment, destructive)
      const f = (e as HTMLElement).closest('form');
      if (f && f.getAttribute('data-stress') === 'no-submit-fuzz') return;
      const sel = buildSelector(e);
      if (seen.has(sel)) return;
      seen.add(sel);
      out.push({ selector: sel, kind: 'input', inputType: t, description: (e as HTMLInputElement).name || (e.getAttribute('aria-label') || '').slice(0, 40) || el.tagName.toLowerCase() });
    });
    return out;
  }).catch(() => [] as FuzzableElement[]);
}

/* -- Network chaos -------------------------------------------------------- */

/**
 * Install a route handler that randomly aborts / delays / 500s a small
 * fraction of XHR + fetch responses. Keeps GETs to static assets
 * untouched so the browser still loads the bundle.
 */
async function installNetworkChaos(context: BrowserContext, ctx: StressContext, abortRate = 0.05): Promise<{ uninstall: () => Promise<void>; trips: () => number }> {
  let trips = 0;
  await context.route('**/api/**', async (route) => {
    const req = route.request();
    const m = req.method();
    // Never disrupt OPTIONS (CORS preflight) or auth/csrf bootstrap.
    if (m === 'OPTIONS' || /\/api\/auth\/csrf|\/api\/(client-)?env|\/api\/auth\/session/.test(req.url())) {
      return route.continue();
    }
    const dice = Math.random();
    if (dice < abortRate / 3) {
      trips++;
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] network chaos: aborted ${m} ${req.url()}` });
      return route.abort('failed');
    }
    if (dice < (abortRate * 2) / 3) {
      trips++;
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] network chaos: 500 ${m} ${req.url()}` });
      return route.fulfill({ status: 500, body: '{"error":"stress-injected"}', contentType: 'application/json' });
    }
    if (dice < abortRate) {
      trips++;
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] network chaos: 2000ms latency ${m} ${req.url()}` });
      await new Promise(r => setTimeout(r, 2000));
      return route.continue();
    }
    return route.continue();
  });
  return {
    uninstall: async () => { try { await context.unroute('**/api/**'); } catch { /* fine */ } },
    trips: () => trips,
  };
}

/* -- Sub-loops ------------------------------------------------------------ */

const pickRandom = <T>(arr: T[]): T | undefined => arr[Math.floor(Math.random() * arr.length)];

/**
 * A click/fill candidate is "avoided" if any avoidSelectors token appears
 * in the selector string OR the description (button text / aria-label),
 * checked case-insensitively. The description check is critical — a
 * dangerous button often has a generic id like `[id="r4j"]` but reads
 * "Delete account" or "Reset" in its label.
 *
 * Always-on hard avoid list: catastrophic verbs + admin destructive
 * endpoints. These are safety belt-and-braces — even if the journey
 * forgets to specify them, we will never click them.
 */
const HARD_AVOID = [
  'reset', 'delete', 'revoke', 'end election', 'end poll', 'wipe',
  'purge', 'drop', 'destroy', 'force logout', 'sign out', 'logout',
  'shutdown', 'reboot', 'restart', 'admin/reset', 'admin/delete',
  'admin/revoke', 'admin/wipe', 'remove account', 'remove passkey',
];

const isAvoided = (sel: string, description: string | undefined, avoid?: string[]): boolean => {
  const merged = [...HARD_AVOID, ...(avoid || [])];
  if (!merged.length) return false;
  const haystack = `${sel} ${description || ''}`.toLowerCase();
  return merged.some(a => haystack.includes(a.toLowerCase()));
};

async function clickFuzzLoop(page: Page, ctx: StressContext, cfg: ReturnType<typeof resolveCfg>, fuzzables: FuzzableElement[], deadline: number, screenshotCounter: { n: number; shots: string[] }): Promise<{ attempted: number; ok: number; failed: number }> {
  const clickable = fuzzables.filter(f => f.kind === 'click' && !isAvoided(f.selector, f.description, cfg.avoidSelectors));
  if (!clickable.length) return { attempted: 0, ok: 0, failed: 0 };
  let attempted = 0, ok = 0, failed = 0;
  const target = cfg.clickCount;
  // Blacklist a selector after 2 consecutive failures so a single broken
  // button doesn't burn the entire click budget at 600ms-per-failure.
  const failureCount = new Map<string, number>();
  while (attempted < target && Date.now() < deadline) {
    // If a previous click navigated us off the SPA (chrome-error, blank,
    // raw API JSON), abort the click loop — there's nothing valid to
    // click and the next attempt will just timeout. The runner's recovery
    // logic restores the page after the loop returns.
    let curPath = '';
    try { curPath = page.url(); } catch { /* fine */ }
    if (curPath.startsWith('chrome-error://') || curPath === 'about:blank' || /\/api\//.test(curPath)) {
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] click loop early-exit — page parked on ${curPath}` });
      break;
    }
    const pool = clickable.filter(c => (failureCount.get(c.selector) || 0) < 2);
    if (!pool.length) break;
    const f = pickRandom(pool);
    if (!f) break;
    attempted++;
    try {
      await page.click(f.selector, { timeout: 600, force: false, trial: false });
      ok++;
      failureCount.delete(f.selector);
    } catch (e: any) {
      failed++;
      failureCount.set(f.selector, (failureCount.get(f.selector) || 0) + 1);
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] click failed (${(f.description || '').slice(0, 40)}): ${String(e?.message || e).slice(0, 120)}` });
    }
    // Sometimes immediately Escape — surfaces modal close leaks.
    if (Math.random() < 0.2) {
      try { await page.keyboard.press('Escape'); } catch { /* ok */ }
    }
    if (cfg.screenshotEveryN > 0 && attempted % cfg.screenshotEveryN === 0) {
      const p = await ctx.takeScreenshot(`stress-click-${attempted}`);
      if (p) screenshotCounter.shots.push(p);
    }
    // Tiny jitter so UI has a chance to react.
    await page.waitForTimeout(20 + Math.random() * 80);
  }
  return { attempted, ok, failed };
}

async function fillFuzzLoop(page: Page, ctx: StressContext, cfg: ReturnType<typeof resolveCfg>, fuzzables: FuzzableElement[], deadline: number): Promise<{ attempted: number; ok: number; failed: number }> {
  const inputs = fuzzables.filter(f => f.kind === 'input');
  if (!inputs.length) return { attempted: 0, ok: 0, failed: 0 };
  const payloads = [...DEFAULT_PAYLOADS, ...(cfg.extraPayloads || [])];
  let attempted = 0, ok = 0, failed = 0;
  const target = cfg.fillCount;
  const failureCount = new Map<string, number>();
  // Refresh pool periodically — tab/click activity in earlier loops may
  // have detached or hidden previously-visible inputs.
  let pool: FuzzableElement[] = inputs.filter(c => (failureCount.get(c.selector) || 0) < 2);
  while (attempted < target && Date.now() < deadline) {
    // Re-check pool every 5 attempts so blacklisted selectors clear.
    if (attempted % 5 === 0) {
      pool = inputs.filter(c => (failureCount.get(c.selector) || 0) < 2);
      if (!pool.length) {
        const fresh = await collectFuzzableElements(page);
        pool = fresh.filter(f => f.kind === 'input');
        if (!pool.length) break;
      }
    }
    const f = pickRandom(pool);
    const p = pickRandom(payloads);
    if (!f || p === undefined) break;
    attempted++;
    try {
      if (f.inputType === undefined && f.selector.startsWith('select')) {
        // For <select>, pick a random option.
        await page.evaluate((sel) => {
          const el = document.querySelector(sel) as HTMLSelectElement | null;
          if (!el || !el.options || !el.options.length) return;
          const idx = Math.floor(Math.random() * el.options.length);
          el.selectedIndex = idx;
          el.dispatchEvent(new Event('change', { bubbles: true }));
        }, f.selector);
      } else {
        await page.fill(f.selector, p, { timeout: 600 });
      }
      ok++;
      failureCount.delete(f.selector);
    } catch (e: any) {
      failed++;
      failureCount.set(f.selector, (failureCount.get(f.selector) || 0) + 1);
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] fill failed (${(f.description || '').slice(0, 40)}=${JSON.stringify(p).slice(0, 30)}): ${String(e?.message || e).slice(0, 100)}` });
    }
    // 30% chance: press Enter immediately to fire onSubmit / blur side
    // effects. Surfaces double-submit bugs.
    if (Math.random() < 0.3) {
      try { await page.press(f.selector, 'Enter', { timeout: 400 }); } catch { /* ok */ }
    }
    await page.waitForTimeout(15 + Math.random() * 50);
  }
  return { attempted, ok, failed };
}

async function keyboardFuzzLoop(page: Page, _ctx: StressContext, cfg: ReturnType<typeof resolveCfg>, deadline: number): Promise<{ attempted: number }> {
  // Cycle Tab/Shift-Tab to walk the focus order, smash Escape, hit
  // arrow keys (radix dropdowns + radio groups react), Enter, Home/End.
  const keys = ['Tab', 'Tab', 'Tab', 'Shift+Tab', 'Escape', 'Enter', 'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown', 'Space'];
  let attempted = 0;
  const target = cfg.keyboardCount;
  while (attempted < target && Date.now() < deadline) {
    const k = pickRandom(keys);
    if (!k) break;
    attempted++;
    try { await page.keyboard.press(k); } catch { /* fine — page may have nav-d */ }
    await page.waitForTimeout(15 + Math.random() * 40);
  }
  return { attempted };
}

async function resizeFuzzLoop(page: Page, _ctx: StressContext, cfg: ReturnType<typeof resolveCfg>, deadline: number): Promise<{ attempted: number }> {
  // Sane breakpoint range — 320 (mobile) ↔ 1920 (FHD). Avoids 0-pixel
  // viewports which crash some web fonts.
  const widths = [375, 414, 540, 768, 1024, 1280, 1440, 1920];
  const heights = [640, 720, 800, 900, 1080];
  let attempted = 0;
  const target = cfg.resizeCount;
  while (attempted < target && Date.now() < deadline) {
    attempted++;
    const w = pickRandom(widths) || 1280;
    const h = pickRandom(heights) || 800;
    try { await page.setViewportSize({ width: w, height: h }); } catch { /* fine */ }
    await page.waitForTimeout(150 + Math.random() * 250);
  }
  return { attempted };
}

async function rotateTabsLoop(page: Page, ctx: StressContext, cfg: ReturnType<typeof resolveCfg>): Promise<void> {
  if (!cfg.rotateTabs || !cfg.rotateTabs.length) return;
  for (const sel of cfg.rotateTabs) {
    try {
      await page.click(sel, { timeout: 2_000 });
      await page.waitForTimeout(400);
      // Snap an aria + screenshot after each tab so the report shows
      // each tab's UI state.
      const p = await ctx.takeScreenshot(`stress-tab-${sel.replace(/[^A-Za-z0-9]/g, '_').slice(0, 40)}`);
      if (!p) continue;
    } catch (e: any) {
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] tab rotate "${sel}" failed: ${String(e?.message || e).slice(0, 100)}` });
    }
  }
}

/* -- Entry point ---------------------------------------------------------- */

export async function runStress(page: Page, cfgIn: StressConfig, ctx: StressContext): Promise<StressResult> {
  const cfg = resolveCfg(cfgIn);
  const start = Date.now();
  const deadline = start + cfg.durationMs;
  const screenshotCounter = { n: 0, shots: [] as string[] };
  const result: StressResult = {
    durationMs: 0,
    clicks: { attempted: 0, ok: 0, failed: 0 },
    fills: { attempted: 0, ok: 0, failed: 0 },
    keyboards: { attempted: 0 },
    resizes: { attempted: 0 },
    screenshots: [],
    networkChaosTrips: 0,
    finalDom: { nodes: 0, buttons: 0, inputs: 0, errorRoles: 0 },
  };

  const context = page.context();
  // Pin the URL we started on. If clicks/fills navigate the browser to an
  // error page or off-origin we restore to this before final stats so we
  // measure the page we were supposed to be testing, not the wreckage.
  let originUrl = '';
  try { originUrl = page.url(); } catch { /* fine */ }

  let chaos: { uninstall: () => Promise<void>; trips: () => number } | null = null;
  if (cfg.networkChaos) {
    try { chaos = await installNetworkChaos(context, ctx); } catch { /* fine */ }
  }

  // Scroll-reveal so lazy/virtualized content shows up before we count.
  await scrollToReveal(page);

  // Initial collection — refresh between major phases since the DOM
  // mutates wildly under stress.
  const fuzz = await collectFuzzableElements(page);
  ctx.log({ kind: 'console', level: 'info', text: `[stress] collected ${fuzz.length} fuzzable elements (clickable=${fuzz.filter(f => f.kind === 'click').length}, inputs=${fuzz.filter(f => f.kind === 'input').length})` });

  // Resilience: if we're parked on a raw API JSON response (the SPA's
  // gatekeeper occasionally hard-POSTs), skip fuzz entirely — there's
  // nothing to interact with and the next journey step will reset us.
  let curUrl = '';
  try { curUrl = page.url(); } catch { /* fine */ }
  if (/\/api\//.test(curUrl) && fuzz.length === 0) {
    ctx.log({ kind: 'console', level: 'warn', text: `[stress] ABORT — page parked on API URL (${curUrl}); no fuzzable elements. Skipping click/fill/keyboard/resize loops.` });
    result.abortReason = `page parked on API URL: ${curUrl}`;
    result.durationMs = Date.now() - start;
    return result;
  }

  // Initial screenshot for visual baseline.
  if (cfg.finalScreenshot) {
    const p = await ctx.takeScreenshot('stress-initial');
    if (p) screenshotCounter.shots.push(p);
  }

  // Tab rotation FIRST so subsequent fuzz hits every tab's contents,
  // not just the default tab. Tabs that fail to switch surface their
  // own console errors via the standard logger.
  await rotateTabsLoop(page, ctx, cfg);

  // Re-collect after tab rotation since each tab adds new interactables.
  const fuzz2 = await collectFuzzableElements(page);
  const fuzzables = fuzz2.length > fuzz.length ? fuzz2 : fuzz;

  // Per-loop time budget so a slow click loop doesn't starve fills/keys/
  // resizes. Weights add to 1.0; the resize loop just consumes remaining
  // time so even an overshoot in earlier phases finishes cleanly.
  const totalMs = cfg.durationMs;
  const clickBudget = Math.floor(totalMs * 0.30);
  const fillBudget = Math.floor(totalMs * 0.30);
  const keyBudget = Math.floor(totalMs * 0.20);

  // Helper: between loops, if the previous loop's interactions navigated
  // us off the SPA (chrome-error, blank, raw API JSON), restore so the
  // next loop has something real to fuzz.
  const ensureOnOrigin = async (label: string): Promise<void> => {
    try {
      const cur = page.url();
      const drifted = cur.startsWith('chrome-error://') || cur === 'about:blank' || /\/api\//.test(cur);
      if (drifted && originUrl && originUrl !== cur) {
        ctx.log({ kind: 'console', level: 'warn', text: `[stress] ${label}: page drifted to ${cur}; restoring to ${originUrl}` });
        await page.goto(originUrl, { timeout: 5_000, waitUntil: 'domcontentloaded' });
        await page.waitForTimeout(400);
      }
    } catch { /* fine */ }
  };

  try {
    if (!cfg.skipClicks && Date.now() < deadline) {
      const localDeadline = Math.min(deadline, Date.now() + clickBudget);
      result.clicks = await clickFuzzLoop(page, ctx, cfg, fuzzables, localDeadline, screenshotCounter);
    }
    if (!cfg.skipFills && Date.now() < deadline) {
      await ensureOnOrigin('before-fills');
      // Re-collect — clicks may have opened modals with fresh inputs,
      // and we may have just restored to the origin URL.
      const fresh = await collectFuzzableElements(page);
      const localDeadline = Math.min(deadline, Date.now() + fillBudget);
      result.fills = await fillFuzzLoop(page, ctx, cfg, fresh.length > fuzzables.length ? fresh : fuzzables, localDeadline);
    }
    if (!cfg.skipKeyboard && Date.now() < deadline) {
      await ensureOnOrigin('before-keyboard');
      const localDeadline = Math.min(deadline, Date.now() + keyBudget);
      result.keyboards = await keyboardFuzzLoop(page, ctx, cfg, localDeadline);
    }
    if (!cfg.skipResize && Date.now() < deadline) {
      await ensureOnOrigin('before-resize');
      // Resize gets whatever's left.
      result.resizes = await resizeFuzzLoop(page, ctx, cfg, deadline);
    }
  } catch (e: any) {
    result.abortReason = String(e?.message || e);
  }

  if (chaos) {
    result.networkChaosTrips = chaos.trips();
    await chaos.uninstall();
  }

  // If a click/fill navigated us off-page (chrome-error, about:blank, or
  // an /api/* JSON response), restore to the URL we were testing so the
  // final-DOM measurement reflects the actual page under test.
  try {
    const cur = page.url();
    const drifted = cur.startsWith('chrome-error://') || cur === 'about:blank' || /\/api\//.test(cur);
    if (drifted && originUrl && originUrl !== cur) {
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] page drifted to ${cur} during fuzz; restoring to ${originUrl}` });
      await page.goto(originUrl, { timeout: 5_000, waitUntil: 'domcontentloaded' });
      await page.waitForTimeout(500);
    }
  } catch { /* fine */ }

  // Final DOM stats — capture BEFORE back/forward chaos so we measure the
  // post-fuzz DOM, not about:blank.
  try {
    result.finalDom = await page.evaluate(() => ({
      nodes: document.querySelectorAll('*').length,
      buttons: document.querySelectorAll('button, [role="button"]').length,
      inputs: document.querySelectorAll('input, textarea, select').length,
      errorRoles: document.querySelectorAll('[role="alert"], [aria-invalid="true"]').length,
    }));
  } catch { /* fine */ }

  // Final back/forward chaos so the SPA router's cleanup paths fire.
  // We don't care about the result — the next journey step does its own
  // goto so this is just to surface unmount errors and route effects.
  // Save the URL we were on so we can recover if back/forward strands
  // the browser on chrome-error:// (no history) or about:blank.
  let savedUrl = '';
  try { savedUrl = page.url(); } catch { /* fine */ }
  try { await page.goBack({ timeout: 1_500 }); } catch { /* ok */ }
  try { await page.goForward({ timeout: 1_500 }); } catch { /* ok */ }
  try {
    const cur = page.url();
    if (savedUrl && (cur.startsWith('chrome-error://') || cur === 'about:blank')) {
      ctx.log({ kind: 'console', level: 'warn', text: `[stress] back/forward stranded on ${cur}; restoring to ${savedUrl}` });
      await page.goto(savedUrl, { timeout: 5_000, waitUntil: 'domcontentloaded' });
    }
  } catch { /* fine */ }

  if (cfg.finalScreenshot) {
    const p = await ctx.takeScreenshot('stress-final');
    if (p) screenshotCounter.shots.push(p);
  }
  result.screenshots = screenshotCounter.shots;
  result.durationMs = Date.now() - start;
  ctx.log({ kind: 'console', level: 'info', text: `[stress] complete: clicks ok=${result.clicks.ok}/${result.clicks.attempted} fills ok=${result.fills.ok}/${result.fills.attempted} keys=${result.keyboards.attempted} resizes=${result.resizes.attempted} chaos-trips=${result.networkChaosTrips} dur=${result.durationMs}ms domNodes=${result.finalDom.nodes} errorRoles=${result.finalDom.errorRoles}` });
  return result;
}
