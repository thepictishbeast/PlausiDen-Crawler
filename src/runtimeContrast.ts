/**
 * runtimeContrast.ts — runtime WCAG contrast check on rendered text.
 *
 * The forge build-time `phase_contrast` (T3) checks every (token,
 * token) pair declared in loom-tokens.css. That catches design-system
 * regressions early, but it can't see:
 *
 *   - text rendered against a background-image / gradient (no token)
 *   - text whose effective color is overridden by a child rule
 *   - colors injected at runtime via JS / a third-party widget
 *   - forced-colors / system-theme overrides
 *
 * Runtime contrast (this file, T29) walks every visible text node in
 * the rendered DOM and computes the EFFECTIVE color vs the EFFECTIVE
 * background — the latter accounting for translucent backgrounds
 * stacking up the parent chain. WCAG 2.1 1.4.3 / 1.4.6 thresholds:
 *
 *   - body text:  4.5:1 (AA), strict if below
 *   - large text: 3:1 (AA), warn if below
 *
 * Mirror's the cssHealth / uiOverflow pattern. Page-eval string
 * (string-eval workaround for tsx __name issue, same as uiOverflow).
 */
import type { Page } from 'playwright';

export interface RuntimeContrastFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ContrastOffender {
  selector: string;
  fg: string;
  bg: string;
  ratio: number;
  required: number;
  fontSizePx: number;
  isLarge: boolean;
  text: string;
}

export interface RuntimeContrastSnapshot {
  pageUrl: string;
  viewport: { width: number; height: number };
  textNodesScanned: number;
  totalContrastPairs: number;
  failingOffenders: ContrastOffender[];
}

export async function captureRuntimeContrastSnapshot(page: Page): Promise<RuntimeContrastSnapshot> {
  const pageUrl = page.url();

  // String-eval workaround (named function declarations get __name
  // wrappers under tsx that don't exist in the page context). The
  // detector walks every visible non-empty text node, computes its
  // effective foreground color, walks up the parent chain to find
  // the first non-transparent background, computes WCAG ratio.
  const evalFn = `(() => {
    const parseColor = function(c) {
      // Returns [r,g,b,a] in 0-255 / 0-1.
      const m = /^rgba?\\(\\s*([0-9.]+)\\s*,\\s*([0-9.]+)\\s*,\\s*([0-9.]+)\\s*(?:,\\s*([0-9.]+)\\s*)?\\)$/i.exec(c);
      if (!m) return null;
      return [parseFloat(m[1]), parseFloat(m[2]), parseFloat(m[3]), m[4] === undefined ? 1 : parseFloat(m[4])];
    };
    const sRgb = function(c) { c = c / 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    const luminance = function(rgba) { return 0.2126 * sRgb(rgba[0]) + 0.7152 * sRgb(rgba[1]) + 0.0722 * sRgb(rgba[2]); };
    const contrast = function(a, b) {
      const l1 = luminance(a), l2 = luminance(b);
      const lo = Math.min(l1, l2), hi = Math.max(l1, l2);
      return (hi + 0.05) / (lo + 0.05);
    };
    const blendOver = function(top, under) {
      // Composite top (rgba) over under (rgba) — alpha-over operator.
      const a = top[3];
      return [
        top[0] * a + under[0] * (1 - a),
        top[1] * a + under[1] * (1 - a),
        top[2] * a + under[2] * (1 - a),
        1
      ];
    };

    // Walk up to find effective background: stack any translucent
    // backgrounds and composite them. Returns the resolved opaque
    // rgba (a=1).
    const effectiveBg = function(el) {
      let stack = [];
      let node = el;
      while (node && node.nodeType === 1) {
        const cs = window.getComputedStyle(node);
        const bg = parseColor(cs.backgroundColor);
        if (bg && bg[3] > 0) {
          // background-image with gradients defeats this — bail and
          // let the caller skip this offender (we can't compute
          // contrast against an arbitrary gradient).
          if (cs.backgroundImage && cs.backgroundImage !== 'none') {
            return null;
          }
          stack.unshift(bg);
          if (bg[3] === 1) break;
        }
        node = node.parentElement;
      }
      // Default html background = white per spec.
      let cur = [255, 255, 255, 1];
      for (const layer of stack) cur = blendOver(layer, cur);
      return cur;
    };

    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    let textNodesScanned = 0;
    let totalContrastPairs = 0;
    const failingOffenders = [];

    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
    let n;
    while ((n = walker.nextNode())) {
      const t = (n.nodeValue || '').trim();
      if (t.length < 2) continue;
      const parent = n.parentElement;
      if (!parent) continue;
      const cs = window.getComputedStyle(parent);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') continue;
      // Hidden elements (offscreen, sr-only, etc.) — skip if width/height 0.
      const rect = parent.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      textNodesScanned += 1;

      const fg = parseColor(cs.color);
      if (!fg || fg[3] === 0) continue;
      const bg = effectiveBg(parent);
      if (!bg) continue; // Hit a gradient — can't compute.

      const fgOpaque = bg ? blendOver(fg, bg) : fg;
      const ratio = contrast(fgOpaque, bg);
      const fontSizePx = parseFloat(cs.fontSize) || 16;
      const fontWeight = parseInt(cs.fontWeight, 10) || 400;
      // WCAG large text = >= 18pt (24px) OR >= 14pt (18.66px) bold.
      const isLarge = fontSizePx >= 24 || (fontSizePx >= 18.66 && fontWeight >= 700);
      const required = isLarge ? 3 : 4.5;
      totalContrastPairs += 1;
      if (ratio < required) {
        failingOffenders.push({
          selector: selectorOf(parent),
          fg: cs.color,
          bg: 'rgb(' + Math.round(bg[0]) + ',' + Math.round(bg[1]) + ',' + Math.round(bg[2]) + ')',
          ratio: Math.round(ratio * 100) / 100,
          required: required,
          fontSizePx: Math.round(fontSizePx * 10) / 10,
          isLarge: isLarge,
          text: t.slice(0, 60)
        });
      }
    }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      textNodesScanned: textNodesScanned,
      totalContrastPairs: totalContrastPairs,
      failingOffenders: failingOffenders
    };
  })()`;

  const result = await page.evaluate(evalFn) as {
    vpW: number;
    vpH: number;
    textNodesScanned: number;
    totalContrastPairs: number;
    failingOffenders: ContrastOffender[];
  };

  return {
    pageUrl,
    viewport: { width: result.vpW, height: result.vpH },
    textNodesScanned: result.textNodesScanned,
    totalContrastPairs: result.totalContrastPairs,
    failingOffenders: result.failingOffenders,
  };
}

export function detectRuntimeContrastIssues(snap: RuntimeContrastSnapshot): RuntimeContrastFinding[] {
  const out: RuntimeContrastFinding[] = [];
  const strict = snap.failingOffenders.filter((o) => !o.isLarge);
  const warn = snap.failingOffenders.filter((o) => o.isLarge);
  if (strict.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'contrast.body-text-below-aa',
      detail: `${strict.length} body-text element(s) below WCAG AA 4.5:1 contrast.`,
      evidence: { offenderCount: strict.length, offenders: strict.slice(0, 10) },
    });
  }
  if (warn.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'contrast.large-text-below-aa',
      detail: `${warn.length} large-text element(s) below WCAG AA 3:1 contrast.`,
      evidence: { offenderCount: warn.length, offenders: warn.slice(0, 10) },
    });
  }
  return out;
}

export async function checkRuntimeContrast(
  page: Page,
): Promise<{ snapshot: RuntimeContrastSnapshot; findings: RuntimeContrastFinding[] }> {
  const snapshot = await captureRuntimeContrastSnapshot(page);
  const findings = detectRuntimeContrastIssues(snapshot);
  return { snapshot, findings };
}
