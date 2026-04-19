/**
 * Puppeteer driver adapter.
 *
 * Puppeteer absorbed via npm (puppeteer@24.x). This module is the thin
 * facade per AVP-2 dependency-inversion — only this file imports
 * puppeteer; the Runner talks to this adapter.
 *
 * When to use Puppeteer over Playwright (the default):
 *   • Heavy concurrency (100+ parallel browsers)
 *   • puppeteer-extra stealth plugin for bot-detection sites
 *   • Chromium-only workloads where Playwright's multi-browser abstraction is overhead
 *
 * Not a full Driver implementation yet — that's v0.3's work. This stub
 * proves the dep chain wires cleanly.
 */
import type { Driver } from '../../drivers/types';
import type { CapturedEvent } from '../../report';

export interface PuppeteerDriverOpts {
  headless?: boolean;
  viewport?: { width: number; height: number };
}

export const PUPPETEER_VERSION = '24.41.0'; // pinned; bump deliberately

/**
 * Create a Puppeteer-backed Driver. Mirrors the Playwright driver API
 * so the Runner can swap platforms without knowing which backend is
 * driving the browser.
 */
export async function createPuppeteerDriver(opts: PuppeteerDriverOpts = {}): Promise<Driver> {
  const puppeteer = (await import('puppeteer')).default;
  const browser = await puppeteer.launch({ headless: opts.headless !== false });
  const page = await browser.newPage();
  if (opts.viewport) await page.setViewport(opts.viewport);
  const listeners: Array<(e: CapturedEvent) => void> = [];
  const emit = (e: Omit<CapturedEvent, 't'>) => {
    const ev: CapturedEvent = { ...e, t: Date.now() };
    for (const fn of listeners) { try { fn(ev); } catch { /* silent */ } }
  };
  page.on('console', (msg) => emit({
    kind: 'console', level: msg.type(), text: msg.text(),
    url: msg.location().url,
  }));
  page.on('pageerror', (err) => emit({
    kind: 'pageerror', text: err.message, stack: err.stack,
  }));
  page.on('requestfailed', (req) => emit({
    kind: 'request-failed', text: req.failure()?.errorText || 'unknown', url: req.url(),
  }));
  page.on('response', (res) => {
    if (res.status() >= 400) {
      emit({ kind: 'response-error', text: res.statusText(), url: res.url(), status: res.status() });
    }
  });
  return {
    platform: 'web',
    async start(target: string) { await page.goto(target, { waitUntil: 'networkidle0' }); },
    async goto(url: string) { await page.goto(url, { waitUntil: 'networkidle0' }); },
    async click(sel, o) { await page.click(sel, { timeout: o?.timeout ?? 10_000 } as any); },
    async fill(sel, v, o) { await page.$eval(sel, (el: any) => { el.value = ''; }); await page.type(sel, v); },
    async type(sel, v) { await page.type(sel, v, { delay: 30 }); },
    async press(key, sel) {
      if (sel) await page.focus(sel);
      await page.keyboard.press(key as any);
    },
    async waitFor(sel, o) { await page.waitForSelector(sel, { timeout: o?.timeout ?? 10_000 }); },
    async waitMs(ms: number) { await new Promise(r => setTimeout(r, ms)); },
    async scroll(dy: number) { await page.evaluate((dy: number) => window.scrollBy(0, dy), dy); },
    async screenshot(path: string) { await page.screenshot({ path: path as `${string}.png`, fullPage: true }); },
    async textOf(sel, o) {
      try {
        await page.waitForSelector(sel, { timeout: o?.timeout ?? 5_000 });
        return await page.$eval(sel, (el: any) => el.textContent);
      } catch { return null; }
    },
    onEvent(cb) { listeners.push(cb); },
    async close() { await browser.close(); },
  };
}
