/**
 * fontLoading.ts — `@font-face` font-display detector. T76.
 *
 * When a page declares a custom web font without specifying a
 * `font-display` strategy, the browser hides text rendered in
 * that font UNTIL the font file finishes loading. On a slow
 * connection or a font-server outage, the page renders blank
 * rectangles where text should be — Flash of Invisible Text
 * (FOIT). Even on fast connections, FOIT delays first paint by
 * the font-load time.
 *
 * The fix is a single CSS line:
 *   @font-face { ... font-display: swap; }
 *
 * `swap` shows fallback text immediately, then swaps to the web
 * font when ready. The other acceptable values:
 *   * `fallback` — brief invisible window then fallback
 *   * `optional` — UA decides; never swaps if slow
 * `block` is the (bad) browser default; `auto` resolves to
 * `block` in most browsers.
 *
 * Findings:
 *
 *   - font-loading.no-display     warn
 *     `@font-face` rule with NO `font-display` declaration. The
 *     browser default is `block` → FOIT.
 *
 *   - font-loading.display-block  warn
 *     `@font-face` rule explicitly sets `font-display: block` or
 *     `auto`. Same effect as missing — invisible text until load.
 *
 * Out of scope:
 *   * Cross-origin stylesheets the browser refuses to expose
 *     (`SecurityError` on `cssRules` access). The capture path
 *     swallows these silently — they're invisible to the
 *     detector.
 *   * Pages with NO `@font-face` rules at all. Trivially clean.
 *
 * Per-page DOM-walk pattern (mirrors cssHealth, runtimeImages,
 * etc.). No Rust mirror in v1 — straightforward to add via the
 * crawler-detectors crate when chromiumoxide port T75 lands.
 */
import type { Page } from 'playwright';

export interface FontLoadingFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedFontFace {
  /** family name from `font-family: "..."` */
  family: string;
  /** raw `font-display` value (lowercase trimmed), '' if absent */
  fontDisplay: string;
  /** Stylesheet href the rule came from, '' for inline. */
  sheetHref: string;
}

export interface FontLoadingSnapshot {
  pageUrl: string;
  /** Number of stylesheets the browser refused to expose
   *  (cross-origin SecurityError). Carry the count so the
   *  detector can avoid false-clean reports when most fonts
   *  live in a sheet we can't see. */
  inaccessibleSheetCount: number;
  /** Every @font-face rule the browser DID expose to us. */
  faces: CapturedFontFace[];
}

export async function captureFontLoadingSnapshot(
  page: Page,
): Promise<FontLoadingSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const out = [];
    let inaccessible = 0;
    const sheets = document.styleSheets;
    for (let i = 0; i < sheets.length; i++) {
      const sheet = sheets[i];
      let rules = null;
      try {
        rules = sheet.cssRules;
      } catch (e) {
        // Cross-origin sheet without CORS — browsers refuse to
        // expose .cssRules. We can't audit these.
        inaccessible += 1;
        continue;
      }
      if (!rules) continue;
      for (let r = 0; r < rules.length; r++) {
        const rule = rules[r];
        // CSSRule.FONT_FACE_RULE === 5
        if (rule.type !== 5) continue;
        const style = rule.style;
        if (!style) continue;
        const family = (style.getPropertyValue('font-family') || '').trim().replace(/^["']|["']$/g, '');
        const fontDisplay = (style.getPropertyValue('font-display') || '').trim().toLowerCase();
        out.push({
          family: family.slice(0, 80),
          fontDisplay: fontDisplay,
          sheetHref: sheet.href || '',
        });
      }
    }
    return { faces: out, inaccessibleSheetCount: inaccessible };
  })()`;

  const result = (await page.evaluate(evalFn)) as {
    faces: CapturedFontFace[];
    inaccessibleSheetCount: number;
  };
  return {
    pageUrl,
    inaccessibleSheetCount: result.inaccessibleSheetCount,
    faces: result.faces,
  };
}

/**
 * Acceptable font-display values per spec. `swap` is the
 * recommended default; `fallback` and `optional` are also
 * acceptable. `block` and `auto` cause FOIT.
 */
const SAFE_FONT_DISPLAY = new Set<string>(['swap', 'fallback', 'optional']);
/** Values that explicitly produce FOIT. */
const FOIT_FONT_DISPLAY = new Set<string>(['block', 'auto']);

export function detectFontLoadingIssues(
  snap: FontLoadingSnapshot,
): FontLoadingFinding[] {
  const noDisplay: CapturedFontFace[] = [];
  const blockDisplay: CapturedFontFace[] = [];

  for (const f of snap.faces) {
    const d = f.fontDisplay;
    if (d === '') {
      noDisplay.push(f);
    } else if (FOIT_FONT_DISPLAY.has(d)) {
      blockDisplay.push(f);
    } else if (!SAFE_FONT_DISPLAY.has(d)) {
      // Unknown / typo — treat as no-display (effectively block).
      noDisplay.push(f);
    }
  }

  const out: FontLoadingFinding[] = [];

  if (noDisplay.length > 0) {
    const examples = noDisplay.slice(0, 5).map((f) => {
      const fam = f.family || '(unnamed)';
      const sheet = f.sheetHref ? new URL(f.sheetHref, snap.pageUrl).pathname : '(inline)';
      return `font-family='${fam}' from ${sheet}`;
    });
    out.push({
      severity: 'warn',
      kind: 'font-loading.no-display',
      detail: `${noDisplay.length} @font-face rule(s) have no 'font-display' declaration. Browser default is 'block' → text rendered in this font is INVISIBLE until the font file loads (Flash of Invisible Text / FOIT). On slow connections this can hide content for several seconds. Add 'font-display: swap' to each rule. Examples: ${examples.join('; ')}`,
      evidence: { count: noDisplay.length, examples, inaccessibleSheetCount: snap.inaccessibleSheetCount },
    });
  }

  if (blockDisplay.length > 0) {
    const examples = blockDisplay.slice(0, 5).map((f) => {
      const fam = f.family || '(unnamed)';
      return `font-family='${fam}' font-display='${f.fontDisplay}'`;
    });
    out.push({
      severity: 'warn',
      kind: 'font-loading.display-block',
      detail: `${blockDisplay.length} @font-face rule(s) explicitly set 'font-display: block' or 'auto' — text is INVISIBLE until the font loads (FOIT). 'auto' resolves to 'block' in most browsers. Switch to 'swap'. Examples: ${examples.join('; ')}`,
      evidence: { count: blockDisplay.length, examples, inaccessibleSheetCount: snap.inaccessibleSheetCount },
    });
  }

  return out;
}

export async function checkFontLoading(
  page: Page,
): Promise<{
  snapshot: FontLoadingSnapshot;
  findings: FontLoadingFinding[];
}> {
  const snapshot = await captureFontLoadingSnapshot(page);
  const findings = detectFontLoadingIssues(snapshot);
  return { snapshot, findings };
}
