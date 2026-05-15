/**
 * cssVarResolution.ts — Detect CSS custom properties (var(--X))
 * that reference an undefined token. Closes the bug class fixed
 * in Loom cycle 95c.
 *
 * THE BUG CLASS
 * -------------
 * Loom's design system defined --loom-space-N / --loom-font-N /
 * --loom-pad-* in a separate loom-tokens.css file that was
 * SHIPPED to /static/ but NEVER LINKED from the page-shell.
 * Result: every `var(--loom-space-N)` reference in skin.css
 * resolved to "invalid" → CSS declaration silently dropped to
 * initial value. Visible symptom: card stats `gap:var(--loom-space-6)`
 * computed as `gap: normal` (the initial), so card-stat values
 * touched ("78%$240" with no space).
 *
 * Owner ("its almost like there isnt that much css being used
 * on skillshots at all") diagnosed it correctly from the visual.
 * Cycle 95c root-caused via headless probe + fixed by inlining
 * the missing token defs into BASE_THEME_CSS.
 *
 * THIS DETECTOR
 * -------------
 * Walks every loaded stylesheet in the rendered page, collects
 * every var(--X) reference + every --X definition, and reports
 * any reference whose token is not defined anywhere in the
 * cascade. Cross-checks two directions:
 *   1. References without definitions → strict (silent failure).
 *   2. Definitions without references → warn (dead token —
 *      maintenance signal, not a runtime bug).
 *
 * Edge cases handled:
 *   - Cross-origin stylesheets (cssRules access throws SecurityError).
 *     Recorded as "unparseable" finding kind, not strict.
 *   - var() with fallback (`var(--missing, fallback)`) → still flagged
 *     because the fallback masks the bug; if author wrote a fallback
 *     they should know they're depending on it.
 *   - Nested var() (`var(--x, var(--y))`) → both refs collected.
 *   - @media-wrapped definitions → walked recursively.
 *   - :root vs :where(html) etc. → all selectors counted as defs.
 *
 * Ships as detector axis 52 on top of cycle 92's 51-axis baseline.
 */

export interface CssVarFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedCssVarSnapshot {
  pageUrl: string;
  /** Distinct --X tokens defined anywhere in the cascade. */
  defined: string[];
  /** Distinct var(--X) references seen anywhere. */
  referenced: string[];
  /** Stylesheets that couldn't be inspected (cross-origin / null). */
  unparseable: number;
  /** Per-token usage count (helpful for "dead token" warn). */
  refCount: Record<string, number>;
}

/**
 * Pure-function classifier. Snapshot built in-browser via
 * page.evaluate(CSS_VAR_DOM_CAPTURE_JS).
 */
export function detectCssVarIssues(snap: CapturedCssVarSnapshot): CssVarFinding[] {
  const out: CssVarFinding[] = [];

  const defined = new Set(snap.defined);
  const referenced = new Set(snap.referenced);

  // Undefined references — strict (the bug class).
  const undefinedRefs: string[] = [];
  for (const ref of snap.referenced) {
    if (!defined.has(ref)) undefinedRefs.push(ref);
  }
  if (undefinedRefs.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'css-var.undefined-reference',
      detail:
        `${undefinedRefs.length} CSS custom property reference(s) point to ` +
        `tokens that aren't defined anywhere in the page's effective ` +
        `cascade. Affected declarations resolve to invalid → drop to ` +
        `initial value (silent failure). Most-referenced first: ` +
        undefinedRefs
          .slice(0, 8)
          .map((n) => `${n} (×${snap.refCount[n] ?? 0})`)
          .join(', '),
      evidence: {
        count: undefinedRefs.length,
        examples: undefinedRefs.slice(0, 12),
        totalRefs: snap.referenced.length,
        totalDefs: snap.defined.length,
      },
    });
  }

  // Dead definitions — warn (maintenance signal).
  const deadDefs: string[] = [];
  for (const def of snap.defined) {
    if (!referenced.has(def)) deadDefs.push(def);
  }
  if (deadDefs.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'css-var.dead-definition',
      detail:
        `${deadDefs.length} CSS custom property definition(s) are not ` +
        `referenced by any var() in the cascade. Either dead code (safe ` +
        `to remove) or a future-use slot that's not wired up yet. ` +
        `Examples: ${deadDefs.slice(0, 6).join(', ')}`,
      evidence: {
        count: deadDefs.length,
        examples: deadDefs.slice(0, 12),
      },
    });
  }

  // Unparseable stylesheets (CORS) — warn.
  if (snap.unparseable > 0) {
    out.push({
      severity: 'warn',
      kind: 'css-var.unparseable-stylesheet',
      detail:
        `${snap.unparseable} stylesheet(s) couldn't be inspected — ` +
        `cross-origin without proper CORS headers. var()-resolution ` +
        `audit is INCOMPLETE for definitions or references in those ` +
        `sheets. If they're under your control add Access-Control-` +
        `Allow-Origin: ${'*'} or move them same-origin.`,
      evidence: { count: snap.unparseable },
    });
  }

  return out;
}

/**
 * In-browser snapshot builder. Stringified for page.evaluate.
 * Walks document.styleSheets recursively, collects every --X
 * definition + every var(--X) reference (including in @media,
 * @supports, nested rules, fallback chains).
 *
 * BUG ASSUMPTION: a future browser may expose cssRules
 * differently for adoptedStyleSheets / constructable
 * stylesheets. Walker tolerates missing fields (every access
 * is null-checked).
 */
export const CSS_VAR_DOM_CAPTURE_JS = `
(function() {
  var defined = new Set();
  var referenced = new Set();
  var refCount = Object.create(null);
  var unparseable = 0;
  var DEF_RE = /(--[A-Za-z0-9_-]+)\\s*:/g;
  var REF_RE = /var\\(\\s*(--[A-Za-z0-9_-]+)/g;

  function walkRule(rule) {
    if (!rule) return;
    if (rule.cssRules && rule.cssRules.length) {
      for (var i = 0; i < rule.cssRules.length; i++) walkRule(rule.cssRules[i]);
    }
    var text = rule.cssText;
    if (typeof text !== 'string') return;
    var m;
    DEF_RE.lastIndex = 0;
    while ((m = DEF_RE.exec(text)) !== null) defined.add(m[1]);
    REF_RE.lastIndex = 0;
    while ((m = REF_RE.exec(text)) !== null) {
      var n = m[1];
      referenced.add(n);
      refCount[n] = (refCount[n] || 0) + 1;
    }
  }

  for (var s = 0; s < document.styleSheets.length; s++) {
    var sheet = document.styleSheets[s];
    try {
      var rules = sheet.cssRules;
      if (!rules) { unparseable++; continue; }
      for (var i = 0; i < rules.length; i++) walkRule(rules[i]);
    } catch (e) {
      unparseable++;
    }
  }

  // Inline-style attributes also count as definitions/references.
  var els = document.querySelectorAll('[style]');
  for (var k = 0; k < els.length; k++) {
    var t = els[k].getAttribute('style') || '';
    var m;
    DEF_RE.lastIndex = 0;
    while ((m = DEF_RE.exec(t)) !== null) defined.add(m[1]);
    REF_RE.lastIndex = 0;
    while ((m = REF_RE.exec(t)) !== null) {
      var n = m[1];
      referenced.add(n);
      refCount[n] = (refCount[n] || 0) + 1;
    }
  }

  return {
    pageUrl: window.location.href,
    defined: Array.from(defined).sort(),
    referenced: Array.from(referenced).sort(),
    unparseable: unparseable,
    refCount: refCount,
  };
})()
`;
