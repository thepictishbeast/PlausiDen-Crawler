/**
 * UI audit primitives — axe-core injection + result shaping.
 *
 * The existing crawler already captures console/pageerror/4xx/5xx as
 * CapturedEvents. This module adds real WCAG checks on top: it injects
 * the bundled axe-core script into the live page and converts each
 * violation into the same CapturedEvent shape so the diff/budget logic
 * needs zero changes to consume it.
 */
import type { Page } from 'playwright';
import { createRequire } from 'node:module';
import type { CapturedEvent } from './report.js';

const require_ = createRequire(import.meta.url);
const AXE_PATH: string = require_.resolve('axe-core/axe.min.js');

export interface AxeNode {
  html: string;
  target: string[];
  failureSummary?: string;
  /** Trimmed innerText of the offending element. Empty string if axe
   *  flagged a non-text element (icon, image, badge with no copy). */
  text?: string;
  /** Trimmed innerText of the parent — gives the user a phrase to
   *  search for in the live page ("near 'Sign in' in the header"). */
  parentText?: string;
  /** Nearest ancestor heading text, if any — answers "what section
   *  does this live in?". Walks up to find h1-h6 / [role=heading]. */
  sectionHeading?: string;
  /** Approximate region of the page this element lives in. */
  region?: 'header' | 'above-fold' | 'below-fold' | 'footer' | 'off-screen';
  /** Bounding box in CSS pixels. Useful for cropping highlighted
   *  screenshots and for "top-left vs. middle vs. bottom-right" prose. */
  bbox?: { x: number; y: number; w: number; h: number };
}

export interface AxeViolation {
  id: string;
  impact: 'minor' | 'moderate' | 'serious' | 'critical' | null;
  description: string;
  help: string;
  helpUrl: string;
  nodes: AxeNode[];
}

export interface AxePageResult {
  url: string;
  ok: boolean;
  /** Engine error if axe failed to load/run (CSP block, navigation race, etc.). */
  error?: string;
  violations: AxeViolation[];
  /** Wall time spent in axe.run, ms. */
  durationMs: number;
}

/**
 * Inject axe-core and run a scan against the current document. Safe to
 * call on any page; if the page navigates mid-scan or CSP blocks the
 * inline script, returns ok=false with the error string.
 *
 * Skips iframes by default (resultTypes=['violations'], iframes=false)
 * because cross-origin frames either crash the engine or duplicate
 * findings already covered by the page-level scan.
 */
export async function runAxe(page: Page): Promise<AxePageResult> {
  const startedAt = Date.now();
  const url = page.url();
  try {
    await page.addScriptTag({ path: AXE_PATH });
  } catch (e: any) {
    return {
      url, ok: false,
      error: `inject: ${e?.message || e}`,
      violations: [],
      durationMs: Date.now() - startedAt,
    };
  }
  try {
    const raw = await page.evaluate(async () => {
      const w = window as any;
      if (!w.axe || typeof w.axe.run !== 'function') {
        return { __error: 'axe global not present after inject' };
      }

      // Localize a single offending element so the report can tell the
      // user *where on the page* to look. Runs entirely in-page so we
      // don't pay a round-trip per node.
      const localize = (target: any): {
        text?: string;
        parentText?: string;
        sectionHeading?: string;
        region?: string;
        bbox?: { x: number; y: number; w: number; h: number };
      } => {
        try {
          // Resolve the axe target (which may be a CSS selector string
          // or an array of strings for shadow DOM) to a live Element.
          let sel: string | null = null;
          if (typeof target === 'string') sel = target;
          else if (Array.isArray(target) && typeof target[0] === 'string') sel = target[0];
          if (!sel) return {};
          const el = document.querySelector(sel) as HTMLElement | null;
          if (!el) return {};

          const trim = (s: string, n: number) =>
            s.replace(/\s+/g, ' ').trim().slice(0, n);

          const text = trim(el.innerText || el.textContent || '', 80);
          const parent = el.parentElement;
          const parentText = parent ? trim(parent.innerText || parent.textContent || '', 160) : undefined;

          // Walk up to nearest heading-like ancestor so we can say
          // "this is in the section titled 'Verify identity'".
          let sectionHeading: string | undefined;
          let cur: HTMLElement | null = el;
          for (let i = 0; i < 12 && cur; i++) {
            const heading = cur.querySelector?.('h1, h2, h3, h4, h5, h6, [role="heading"]') as HTMLElement | null;
            if (heading && heading !== el && cur.contains(heading)) {
              sectionHeading = trim(heading.innerText || heading.textContent || '', 80);
              if (sectionHeading) break;
            }
            cur = cur.parentElement;
          }

          const r = el.getBoundingClientRect();
          const scrollY = window.scrollY || 0;
          const bbox = {
            x: Math.round(r.left),
            y: Math.round(r.top + scrollY),
            w: Math.round(r.width),
            h: Math.round(r.height),
          };

          // Region heuristic. Page-relative y position: top 80px → header,
          // viewport-height first screen → above-fold, beyond → below-fold,
          // last 200px of document → footer.
          const pageH = Math.max(
            document.documentElement.scrollHeight,
            document.body?.scrollHeight || 0,
          );
          const vh = window.innerHeight || 900;
          let region: string;
          if (bbox.w === 0 && bbox.h === 0) region = 'off-screen';
          else if (bbox.y < 80) region = 'header';
          else if (bbox.y < vh) region = 'above-fold';
          else if (bbox.y > pageH - 240) region = 'footer';
          else region = 'below-fold';

          return { text, parentText, sectionHeading, region, bbox };
        } catch {
          return {};
        }
      };

      try {
        const r = await w.axe.run(document, {
          resultTypes: ['violations'],
          iframes: false,
        });
        // Strip giant fields we never consume to bound report size.
        return {
          violations: (r.violations || []).map((v: any) => ({
            id: v.id,
            impact: v.impact ?? null,
            description: v.description,
            help: v.help,
            helpUrl: v.helpUrl,
            nodes: (v.nodes || []).slice(0, 5).map((n: any) => {
              const target = Array.isArray(n.target) ? n.target.map((t: any) => String(t)).slice(0, 4) : [];
              const loc = localize(target[0]);
              return {
                html: String(n.html || '').slice(0, 400),
                target,
                failureSummary: n.failureSummary ? String(n.failureSummary).slice(0, 400) : undefined,
                text: loc.text,
                parentText: loc.parentText,
                sectionHeading: loc.sectionHeading,
                region: loc.region,
                bbox: loc.bbox,
              };
            }),
          })),
        };
      } catch (e: any) {
        return { __error: `axe.run: ${e?.message || e}` };
      }
    });
    if (raw && (raw as any).__error) {
      return {
        url, ok: false,
        error: (raw as any).__error,
        violations: [],
        durationMs: Date.now() - startedAt,
      };
    }
    return {
      url, ok: true,
      violations: ((raw as any)?.violations || []) as AxeViolation[],
      durationMs: Date.now() - startedAt,
    };
  } catch (e: any) {
    return {
      url, ok: false,
      error: `evaluate: ${e?.message || e}`,
      violations: [],
      durationMs: Date.now() - startedAt,
    };
  }
}

/**
 * Convert an axe result into CapturedEvents. One event per (rule, node)
 * pair so the diff algorithm can pinpoint *which* selector regressed,
 * not just "this rule fires more often now".
 *
 * Caps at 5 nodes per rule (already trimmed in runAxe) — for any rule
 * that fires on dozens of nodes, the user wants to fix the rule, not
 * walk every offending element individually.
 */
export function axeEventsFor(
  result: AxePageResult,
  startEpoch: number,
): CapturedEvent[] {
  const out: CapturedEvent[] = [];
  if (!result.ok) {
    out.push({
      kind: 'pageerror',
      text: `axe failed on ${result.url}: ${result.error}`,
      url: result.url,
      t: Date.now() - startEpoch,
    });
    return out;
  }
  for (const v of result.violations) {
    for (const n of v.nodes) {
      const target = n.target.join(' ');
      out.push({
        kind: 'a11y-violation',
        ruleId: v.id,
        impact: v.impact || 'moderate',
        text: `${v.id} [${v.impact || 'moderate'}] ${v.help} — ${target}`,
        url: result.url,
        t: Date.now() - startEpoch,
      });
    }
  }
  return out;
}

/**
 * Highlight every flagged element on the live page with a red outline
 * and write a single full-page screenshot showing all of them at once.
 * Returns the absolute path of the saved PNG, or undefined on failure.
 *
 * The injected style is removed before returning so subsequent runs of
 * axe (or any other tooling) see the page in its natural state.
 */
export async function annotateViolations(
  page: Page,
  result: AxePageResult,
  outPath: string,
): Promise<string | undefined> {
  if (!result.ok || result.violations.length === 0) return undefined;
  const selectors: string[] = [];
  for (const v of result.violations) {
    for (const n of v.nodes) {
      if (n.target && n.target[0]) selectors.push(n.target[0]);
    }
  }
  if (selectors.length === 0) return undefined;

  try {
    await page.evaluate((sels) => {
      const STYLE_ID = '__axe_highlight_style__';
      const old = document.getElementById(STYLE_ID);
      if (old) old.remove();
      const style = document.createElement('style');
      style.id = STYLE_ID;
      // High-contrast red box + slight halo so it stands out on light
      // AND dark themes. !important survives utility-CSS specificity.
      style.textContent = `
        [data-axe-flag="1"] {
          outline: 3px solid #ff2d2d !important;
          outline-offset: 2px !important;
          box-shadow: 0 0 0 2px rgba(255,45,45,0.25) !important;
        }
      `;
      document.head.appendChild(style);
      for (const s of sels) {
        try {
          document.querySelectorAll(s).forEach((el) => {
            (el as HTMLElement).setAttribute('data-axe-flag', '1');
          });
        } catch { /* malformed selector — skip */ }
      }
    }, selectors);

    await page.screenshot({ path: outPath, fullPage: true });

    await page.evaluate(() => {
      const STYLE_ID = '__axe_highlight_style__';
      document.getElementById(STYLE_ID)?.remove();
      document.querySelectorAll('[data-axe-flag]').forEach((el) => {
        el.removeAttribute('data-axe-flag');
      });
    });
    return outPath;
  } catch {
    return undefined;
  }
}

/**
 * Compact one-line-per-violation summary for human triage. Used to
 * write findings.txt next to the JSON report.
 */
export function renderAxeFindings(byPage: Array<{ url: string; result: AxePageResult; annotated?: string }>): string {
  const lines: string[] = [];
  let totalRules = 0, totalNodes = 0, errored = 0;
  for (const { url, result } of byPage) {
    if (!result.ok) errored++;
    totalRules += result.violations.length;
    for (const v of result.violations) totalNodes += v.nodes.length;
  }
  lines.push(`# Axe findings`);
  lines.push(`# Pages scanned: ${byPage.length} (${errored} engine errors)`);
  lines.push(`# Rules with at least one violation: ${totalRules}`);
  lines.push(`# Total flagged nodes: ${totalNodes}`);
  lines.push('');
  for (const { url, result, annotated } of byPage) {
    lines.push(`## ${url}`);
    lines.push(`   How to view: open this URL in your browser (anonymous / logged out)`);
    if (annotated) {
      lines.push(`   Annotated screenshot (red outlines on every flagged element): ${annotated}`);
    }
    if (!result.ok) {
      lines.push(`  ENGINE ERROR: ${result.error}`);
      lines.push('');
      continue;
    }
    if (result.violations.length === 0) {
      lines.push(`  (no violations)`);
      lines.push('');
      continue;
    }
    for (const v of result.violations) {
      lines.push(`  [${v.impact || 'moderate'}] ${v.id} — ${v.help}`);
      lines.push(`    docs: ${v.helpUrl}`);
      for (const n of v.nodes) {
        const region = n.region ? `[${n.region}]` : '';
        const text = n.text ? `"${n.text}"` : '(no visible text)';
        lines.push(`    · ${region} ${text} — selector: ${n.target.join(' ') || '(none)'}`);
        if (n.sectionHeading) lines.push(`        in section: "${n.sectionHeading}"`);
        if (n.parentText && n.parentText !== n.text) lines.push(`        near text: "${n.parentText}"`);
        if (n.bbox) lines.push(`        position: x=${n.bbox.x}, y=${n.bbox.y}, ${n.bbox.w}×${n.bbox.h}px`);
        if (n.failureSummary) {
          lines.push(`        why: ${n.failureSummary.replace(/\n/g, ' / ')}`);
        }
      }
    }
    lines.push('');
  }
  return lines.join('\n');
}
