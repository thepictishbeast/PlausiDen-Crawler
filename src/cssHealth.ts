/**
 * cssHealth.ts — detect missing/broken CSS the way a real visitor would.
 *
 * Operates strictly on what a visitor's browser sees:
 *   - network responses for <link rel="stylesheet"> resources
 *   - computed styles on the rendered DOM
 *   - the rendered HTML structure
 *
 * NEVER reads project source files. The Crawler is a visitor; it doesn't
 * have your file system, and bugs that only show up on the file system
 * are an audit/CI concern, not a Crawler concern.
 *
 * Owner directive 2026-05-04: "the crawler should only read the dom and
 * sources and network and elements and such thats available to someone
 * visiting a website. it wouldnt have access to those files, it needs
 * to function like a person."
 *
 * Detection heuristics (in priority order):
 *
 *   1. STRICT — page declares stylesheets but ALL of them failed network
 *      (4xx, 5xx, connection refused, timeout). Even if body has some
 *      inline-styled elements, the declared sheets being unreachable is
 *      a P0 bug.
 *   2. STRICT — page declares stylesheets, all returned 200, but at
 *      least one returned with Content-Type that's not text/css
 *      (browsers refuse to apply application/json or text/html as CSS;
 *      this is a server misconfig).
 *   3. STRICT — declared stylesheet returned 200 + correct MIME, but
 *      response body is suspiciously small (< 50 bytes) or empty.
 *   4. STRICT — page has stylesheets declared and they all loaded fine,
 *      but the COMPUTED style on <body> matches user-agent defaults
 *      across multiple signals (default white bg + default serif font +
 *      default font size). This catches the case where CSS files
 *      served fine but contain a parse error early enough that the
 *      browser drops most rules silently.
 *   5. WARN — page declares ZERO stylesheets and has no <style> blocks.
 *      This is suspicious for any non-trivial page; rare but legitimate
 *      for plain-text-only pages.
 *
 * The detector returns CSSHealthFinding[]. Each finding has:
 *   - severity: 'strict' | 'warn'
 *   - kind: machine-grepable id
 *   - detail: human-readable explanation
 *   - evidence: structured fields the user can reproduce against
 */
import type { Page } from 'playwright';

export interface CSSHealthFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface StylesheetObservation {
  url: string;
  status: number;
  contentType: string | null;
  bodyBytes: number;
  /**
   * Number of `{` characters in the response body (counted by the
   * browser via fetch() — same-origin only). A rough proxy for "how
   * many rules the sheet *intends* to define". Compared against
   * appliedRuleCountEstimate to detect parse-rejection.
   * Null if the sheet is cross-origin or the fetch failed.
   */
  declaredBraceCount: number | null;
  /**
   * Number of `}` characters. Imbalance with declaredBraceCount
   * (especially close >> open) strongly indicates a syntax bug.
   */
  declaredCloseBraceCount: number | null;
  fromNetwork: boolean;
  errorText: string | null;
}

export interface CSSHealthSnapshot {
  pageUrl: string;
  declaredSheets: StylesheetObservation[];
  inlineStyleBlockCount: number;
  computedBody: {
    backgroundColor: string;
    color: string;
    fontFamily: string;
    fontSize: string;
    margin: string;
  };
  computedHtml: {
    backgroundColor: string;
  };
  bodyVisibleTextLength: number;
  appliedRuleCountEstimate: number;
}

const USER_AGENT_DEFAULTS = {
  backgroundColor: new Set([
    'rgba(0, 0, 0, 0)',
    'rgb(255, 255, 255)',
    'transparent',
    'initial',
  ]),
  fontFamily: new Set([
    'Times',
    '"Times New Roman"',
    'Times New Roman',
    'serif',
    '"Times New Roman", Times, serif',
  ]),
  fontSize: new Set(['16px', '13.3333px']),
  margin: new Set(['8px', '0px 8px']),
};

/**
 * Capture a CSS-health snapshot of the current page state.
 *
 * Pass in the network observations the calling code has already
 * accumulated (from response/responsefailed Playwright events) — the
 * helper does not subscribe by itself, to avoid double-counting in
 * pipelines that already capture network.
 */
export async function captureCSSHealthSnapshot(
  page: Page,
  networkResponses: Map<string, { status: number; contentType: string | null; bodyBytes: number; errorText: string | null }>,
): Promise<CSSHealthSnapshot> {
  const pageUrl = page.url();

  // Pull declared <link rel="stylesheet"> hrefs as a real visitor would
  // see them: from the rendered DOM, with their resolved absolute URLs.
  const declaredHrefs = await page.evaluate(() => {
    const out: string[] = [];
    document.querySelectorAll('link[rel="stylesheet"]').forEach((el) => {
      const href = (el as HTMLLinkElement).href;
      if (href) out.push(href);
    });
    return out;
  });

  // Same-origin fetch each sheet from inside the page so we can count
  // declared braces (proxy for intended rule count). This is what a
  // real visitor's browser would see — no FS access.
  const braceCounts = await page.evaluate(async (urls: string[]) => {
    const out: Record<string, number | null> = {};
    for (const u of urls) {
      try {
        const r = await fetch(u, { cache: 'no-store' });
        if (!r.ok) {
          out[u] = null;
          continue;
        }
        const text = await r.text();
        out[u] = (text.match(/\{/g) || []).length;
        // Sneak the close-brace count + body length into a parallel
        // map (the function returns both via the captured outer var).
        (out as unknown as { _close?: Record<string, number> })._close ??= {};
        (out as unknown as { _close?: Record<string, number> })._close![u] =
          (text.match(/\}/g) || []).length;
      } catch {
        out[u] = null;
      }
    }
    return out;
  }, declaredHrefs);
  const closeBraceCounts: Record<string, number> =
    ((braceCounts as unknown) as { _close?: Record<string, number> })._close ?? {};

  const declaredSheets: StylesheetObservation[] = declaredHrefs.map((url) => {
    const obs = networkResponses.get(url);
    return {
      url,
      status: obs?.status ?? 0,
      contentType: obs?.contentType ?? null,
      bodyBytes: obs?.bodyBytes ?? 0,
      declaredBraceCount: braceCounts[url] ?? null,
      declaredCloseBraceCount: closeBraceCounts[url] ?? null,
      fromNetwork: !!obs,
      errorText: obs?.errorText ?? null,
    };
  });

  const inlineStyleBlockCount = await page.evaluate(
    () => document.querySelectorAll('style').length,
  );

  const computed = await page.evaluate(() => {
    const bs = window.getComputedStyle(document.body);
    const hs = window.getComputedStyle(document.documentElement);
    return {
      body: {
        backgroundColor: bs.backgroundColor,
        color: bs.color,
        fontFamily: bs.fontFamily,
        fontSize: bs.fontSize,
        margin: bs.margin,
      },
      html: {
        backgroundColor: hs.backgroundColor,
      },
    };
  });

  const bodyVisibleTextLength = await page.evaluate(
    () => (document.body.innerText || '').replace(/\s+/g, ' ').trim().length,
  );

  // Best-effort estimate of how many CSS rules ACTUALLY got applied
  // by walking document.styleSheets and counting rules RECURSIVELY
  // — every CSSMediaRule / CSSSupportsRule has its own cssRules
  // collection, and each of those should count against the brace
  // budget too. Without recursion, a sheet with many @media blocks
  // looks (falsely) like the parser rejected most of the file.
  // Walk via a stack instead of a named recursive function — tsx
  // rewrites named functions with __name() wrappers that don't exist
  // in the page-evaluation context.
  const appliedRuleCountEstimate = await page.evaluate(() => {
    let total = 0;
    const stack: CSSRuleList[] = [];
    for (const sheet of Array.from(document.styleSheets)) {
      try {
        const rules = (sheet as CSSStyleSheet).cssRules;
        if (rules) stack.push(rules);
      } catch {
        /* cross-origin or otherwise unreadable; count as 0 */
      }
    }
    while (stack.length > 0) {
      const rules = stack.pop()!;
      for (let i = 0; i < rules.length; i++) {
        total += 1;
        const inner = (rules[i] as { cssRules?: CSSRuleList }).cssRules;
        if (inner && inner.length) stack.push(inner);
      }
    }
    return total;
  });

  return {
    pageUrl,
    declaredSheets,
    inlineStyleBlockCount,
    computedBody: computed.body,
    computedHtml: computed.html,
    bodyVisibleTextLength,
    appliedRuleCountEstimate,
  };
}

/**
 * Apply detection heuristics to a snapshot. Returns 0+ findings.
 *
 * Pure function — does not perform I/O. Test it with hand-crafted
 * snapshots without spinning up a browser.
 */
export function detectCSSHealthIssues(snap: CSSHealthSnapshot): CSSHealthFinding[] {
  const out: CSSHealthFinding[] = [];

  // Heuristic 5: zero stylesheets at all.
  if (snap.declaredSheets.length === 0 && snap.inlineStyleBlockCount === 0) {
    out.push({
      severity: 'warn',
      kind: 'css.no-stylesheets-declared',
      detail:
        'Page has zero <link rel="stylesheet"> tags AND zero <style> blocks. This is unusual; a real visitor will see browser-default styling.',
      evidence: {
        declaredSheets: 0,
        inlineStyleBlocks: 0,
        bodyVisibleTextLength: snap.bodyVisibleTextLength,
      },
    });
    return out; // No sheets means later heuristics don't apply.
  }

  // Heuristic 1: all declared sheets failed network.
  if (snap.declaredSheets.length > 0) {
    const failedSheets = snap.declaredSheets.filter(
      (s) => s.errorText !== null || (s.fromNetwork && (s.status === 0 || s.status >= 400)),
    );
    if (failedSheets.length === snap.declaredSheets.length) {
      out.push({
        severity: 'strict',
        kind: 'css.all-sheets-failed-network',
        detail: `All ${snap.declaredSheets.length} declared stylesheet(s) failed to load. The page renders with no CSS.`,
        evidence: { failedSheets },
      });
    } else if (failedSheets.length > 0) {
      out.push({
        severity: 'strict',
        kind: 'css.some-sheets-failed-network',
        detail: `${failedSheets.length} of ${snap.declaredSheets.length} declared stylesheet(s) failed to load.`,
        evidence: { failedSheets },
      });
    }

    // Heuristic 2: wrong MIME on a stylesheet.
    for (const s of snap.declaredSheets) {
      if (s.fromNetwork && s.status >= 200 && s.status < 400) {
        const ct = (s.contentType || '').toLowerCase();
        if (ct && !ct.startsWith('text/css')) {
          out.push({
            severity: 'strict',
            kind: 'css.wrong-mime',
            detail: `Stylesheet served with Content-Type "${s.contentType}" — browsers refuse to apply non-text/css responses as CSS.`,
            evidence: { url: s.url, contentType: s.contentType, status: s.status },
          });
        }
      }
    }

    // Heuristic 3: stylesheet body is suspiciously empty.
    for (const s of snap.declaredSheets) {
      if (s.fromNetwork && s.status >= 200 && s.status < 400 && s.bodyBytes < 50) {
        out.push({
          severity: 'strict',
          kind: 'css.empty-or-tiny-body',
          detail: `Stylesheet returned ${s.bodyBytes} byte(s) — almost certainly empty or truncated.`,
          evidence: { url: s.url, bodyBytes: s.bodyBytes },
        });
      }
    }
  }

  // Heuristic 4: sheets loaded fine but computed body matches UA defaults.
  // Only fire when at least one sheet declared AND it loaded fine.
  const anyUsableSheet = snap.declaredSheets.some(
    (s) => s.fromNetwork && s.status >= 200 && s.status < 400 && s.bodyBytes >= 50,
  );
  if (anyUsableSheet) {
    const bodyLooksLikeUaDefault =
      USER_AGENT_DEFAULTS.backgroundColor.has(snap.computedBody.backgroundColor) &&
      USER_AGENT_DEFAULTS.fontFamily.has(snap.computedBody.fontFamily) &&
      USER_AGENT_DEFAULTS.margin.has(snap.computedBody.margin);
    const htmlLooksLikeUaDefault = USER_AGENT_DEFAULTS.backgroundColor.has(
      snap.computedHtml.backgroundColor,
    );
    if (bodyLooksLikeUaDefault && htmlLooksLikeUaDefault) {
      out.push({
        severity: 'strict',
        kind: 'css.served-but-not-applied',
        detail:
          'CSS file(s) loaded successfully but the body still has user-agent default styling (white background, serif font, 8px margin). Likely a CSS parse error early in the file caused the browser to silently drop the rest.',
        evidence: {
          computedBody: snap.computedBody,
          computedHtml: snap.computedHtml,
          appliedRuleCountEstimate: snap.appliedRuleCountEstimate,
          declaredSheets: snap.declaredSheets.length,
        },
      });
    }
    // Rule-density heuristic A: real CSS files yield ~1 rule per
    // 30-200 bytes. If we see < 1 rule per 500 bytes of stylesheet
    // body, the parser is rejecting most of the file.
    const totalSheetBytes = snap.declaredSheets.reduce(
      (acc, s) => acc + (s.fromNetwork && s.status >= 200 && s.status < 400 ? s.bodyBytes : 0),
      0,
    );
    if (totalSheetBytes >= 100) {
      const bytesPerRule = snap.appliedRuleCountEstimate > 0
        ? totalSheetBytes / snap.appliedRuleCountEstimate
        : Infinity;
      if (bytesPerRule > 500) {
        out.push({
          severity: 'strict',
          kind: 'css.applied-rule-count-anomaly',
          detail: `Browser reports only ${snap.appliedRuleCountEstimate} CSS rule(s) applied across ${totalSheetBytes} byte(s) of stylesheet (~${Math.round(bytesPerRule)} bytes/rule, healthy is < 200). Parse error likely dropped most rules.`,
          evidence: {
            appliedRuleCountEstimate: snap.appliedRuleCountEstimate,
            totalSheetBytes,
            bytesPerRule: Math.round(bytesPerRule),
            sheetSizes: snap.declaredSheets.map((s) => ({ url: s.url, bytes: s.bodyBytes })),
          },
        });
      }
    }

    // Rule-density heuristic B: catches small broken sheets. If the
    // stylesheet *body* contains N opening braces but the browser
    // only applied M rules with M much smaller than N, the parser
    // rejected most of the sheet. Small enough threshold (delta >= 2)
    // to fire on tiny test sheets where heuristic A's byte-budget
    // doesn't trigger.
    const totalDeclaredBraces = snap.declaredSheets.reduce(
      (acc, s) => acc + (s.declaredBraceCount ?? 0),
      0,
    );
    // Ratio-based: real CSS routinely has a small gap between
    // declared braces and applied rules (comments containing `{`,
    // string literals, parser quirks). But if the parser applied
    // < 70 % of what was declared, that's a serious rejection.
    const applyRatio = totalDeclaredBraces > 0
      ? snap.appliedRuleCountEstimate / totalDeclaredBraces
      : 1;
    if (totalDeclaredBraces >= 5 && applyRatio < 0.7) {
      out.push({
        severity: 'strict',
        kind: 'css.brace-vs-applied-mismatch',
        detail: `Stylesheet declares ${totalDeclaredBraces} opening brace(s) but the browser applied only ${snap.appliedRuleCountEstimate} rule(s) (${Math.round(applyRatio * 100)}% applied; healthy is > 70%). Likely an unmatched brace, bad selector, or invalid at-rule early in the sheet.`,
        evidence: {
          totalDeclaredBraces,
          appliedRuleCountEstimate: snap.appliedRuleCountEstimate,
          applyRatioPct: Math.round(applyRatio * 100),
          perSheet: snap.declaredSheets.map((s) => ({
            url: s.url,
            braces: s.declaredBraceCount,
          })),
        },
      });
    }

    // Brace-imbalance heuristic: open vs close braces should be equal
    // in a well-formed CSS file. A delta > 1 in either direction
    // indicates an unmatched brace — the parser will misinterpret
    // every rule after the imbalance point.
    for (const s of snap.declaredSheets) {
      if (s.declaredBraceCount === null || s.declaredCloseBraceCount === null) continue;
      const delta = s.declaredCloseBraceCount - s.declaredBraceCount;
      if (Math.abs(delta) > 1) {
        out.push({
          severity: 'strict',
          kind: 'css.brace-imbalance',
          detail: `Stylesheet has ${s.declaredBraceCount} open brace(s) and ${s.declaredCloseBraceCount} close brace(s) — a delta of ${delta}. CSS is malformed; parser will mis-anchor and drop subsequent rules.`,
          evidence: {
            url: s.url,
            openBraces: s.declaredBraceCount,
            closeBraces: s.declaredCloseBraceCount,
            delta,
          },
        });
      }
    }
  }

  return out;
}

/**
 * Convenience wrapper: snapshot + detect in one call.
 */
export async function checkCSSHealth(
  page: Page,
  networkResponses: Map<string, { status: number; contentType: string | null; bodyBytes: number; errorText: string | null }>,
): Promise<{ snapshot: CSSHealthSnapshot; findings: CSSHealthFinding[] }> {
  const snapshot = await captureCSSHealthSnapshot(page, networkResponses);
  const findings = detectCSSHealthIssues(snapshot);
  return { snapshot, findings };
}
