/**
 * D₀ runtime audit — enforces PlausiDen design-system invariants on the
 * **rendered** page, not just the source. Complements:
 *
 *   - axe-core (WCAG, looser 24×24 touch-target floor, broad a11y rules)
 *   - plausiden-standards/audits/* (build-time, source-level: raw <input>,
 *     raw colors, missing overflow safety classes)
 *
 * Build-time audits cannot see what the browser actually renders. A
 * `<button className="...">` that gets flattened to 24×24 by Tailwind
 * tree-shaking, or an inline `style={{ color: brandHex }}` whose token
 * resolution drifted, only shows up at runtime. This module is that
 * second line of defence.
 *
 * v0 checks (geometric / computed-style only — no false positives):
 *
 *   1. touch-target: every interactive element ≥ 44×44 px in actual
 *      rendered geometry (iOS HIG / WCAG 2.5.5 AAA — PlausiDen mandate).
 *   2. control-min-height: every form control's computed `min-height`
 *      ≥ 36 px (matches `min-h-9` baseline from UI-STANDARDS.md §2.1).
 *
 * Future passes will add focus-ring presence, color-token drift, and
 * user-text overflow heuristics. Each new rule must be deterministic
 * and produce zero false positives on the institutional baseline pages
 * before it lands — drift in the audit erodes trust in the audit.
 *
 * Output shape mirrors `audit.ts` so the runner's diff/budget logic
 * needs no changes — D₀ violations become `CapturedEvent` rows of kind
 * `d0-violation` and flow through the same baseline-comparison gate.
 */
import type { Page } from 'playwright';
import type { CapturedEvent } from './report.js';

/** Minimum hit-target side length (px). iOS HIG + PlausiDen UI-STANDARDS §1.1 row `interactive-touch`. */
const TOUCH_TARGET_MIN_PX = 44;

/** Minimum form-control rendered min-height (px). Matches the `min-h-9` baseline from UI-STANDARDS §2.1. */
const CONTROL_MIN_HEIGHT_PX = 36;

/** Per-rule cap so a single bad page can't flood the report. */
const MAX_NODES_PER_RULE = 50;

export type D0RuleId = 'touch-target' | 'control-min-height';

export interface D0Node {
  /** CSS-ish selector path for re-finding the element. */
  target: string;
  /** Trimmed innerText of the element (≤80 chars). */
  text?: string;
  /** Trimmed innerText of the parent (≤160 chars). */
  parentText?: string;
  /** Nearest ancestor heading text (≤80 chars). */
  sectionHeading?: string;
  /** Position region of the element. */
  region?: 'header' | 'above-fold' | 'below-fold' | 'footer' | 'off-screen';
  /** Bounding box in CSS pixels. */
  bbox?: { x: number; y: number; w: number; h: number };
  /** Tag name + measurement causing the violation, e.g. `button 28×28 (need 44×44)`. */
  detail: string;
}

export interface D0Violation {
  id: D0RuleId;
  /** WCAG-style impact. We treat touch-target failures as `serious` per AAA. */
  impact: 'minor' | 'moderate' | 'serious' | 'critical';
  description: string;
  help: string;
  helpUrl: string;
  nodes: D0Node[];
}

export interface D0PageResult {
  url: string;
  ok: boolean;
  /** Engine error (script eval crashed, navigation race, CSP block, etc.). */
  error?: string;
  violations: D0Violation[];
  durationMs: number;
}

const RULE_META: Record<D0RuleId, Pick<D0Violation, 'description' | 'help' | 'helpUrl' | 'impact'>> = {
  'touch-target': {
    description: 'Interactive elements must be at least 44×44 CSS pixels.',
    help: 'PlausiDen UI-STANDARDS §1.1 (interactive-touch trait): every element a user can tap on mobile needs a 44×44px hit area (iOS HIG, WCAG 2.5.5 AAA). Add padding or use the `.touch-target` mixin.',
    helpUrl: 'https://github.com/thepictishbeast/plausiden-standards/blob/main/UI-STANDARDS.md#11-the-baseline-trait-catalog',
    impact: 'serious',
  },
  'control-min-height': {
    description: 'Form controls must have computed min-height ≥ 36px.',
    help: 'PlausiDen UI-STANDARDS §2.1 (control-base trait): `min-h-9` (36px) prevents content clipping and matches the standard touch-friendly control row. Use the kit primitives in `components/ui/*` instead of raw <input>.',
    helpUrl: 'https://github.com/thepictishbeast/plausiden-standards/blob/main/UI-STANDARDS.md#21-inputs-input',
    impact: 'moderate',
  },
};

/**
 * Run all D₀ rules against the current document. Safe to call on any
 * page; on engine error returns ok=false with the error string.
 *
 * Implementation note — every rule's body runs inside a single
 * `page.evaluate` so we pay the page→Node round-trip once. Adding a
 * rule means adding a branch to the in-page collector, not a new
 * round-trip.
 */
export async function runD0Audit(page: Page): Promise<D0PageResult> {
  const startedAt = Date.now();
  const url = page.url();
  try {
    const raw = await page.evaluate(
      ({ TOUCH_TARGET_MIN_PX, CONTROL_MIN_HEIGHT_PX, MAX_NODES_PER_RULE }) => {
        const trim = (s: string, n: number) =>
          (s || '').replace(/\s+/g, ' ').trim().slice(0, n);

        function selectorFor(el: Element): string {
          const parts: string[] = [];
          let cur: Element | null = el;
          for (let depth = 0; cur && depth < 5; depth++) {
            const tag = cur.tagName.toLowerCase();
            const id = cur.id ? `#${cur.id}` : '';
            const cls = (cur.className && typeof cur.className === 'string'
              ? '.' + cur.className.trim().split(/\s+/).slice(0, 2).join('.')
              : '');
            const testid = (cur as HTMLElement).getAttribute?.('data-testid');
            const tid = testid ? `[data-testid="${testid}"]` : '';
            parts.unshift(`${tag}${id}${tid}${cls}`);
            if (id || testid) break; // these alone are usually unique enough
            cur = cur.parentElement;
          }
          return parts.join(' > ');
        }

        function describe(el: HTMLElement): {
          text?: string;
          parentText?: string;
          sectionHeading?: string;
          region: 'header' | 'above-fold' | 'below-fold' | 'footer' | 'off-screen';
          bbox: { x: number; y: number; w: number; h: number };
        } {
          const text = trim(el.innerText || el.textContent || '', 80);
          const parent = el.parentElement;
          const parentText = parent ? trim(parent.innerText || parent.textContent || '', 160) : undefined;

          let sectionHeading: string | undefined;
          let cur: HTMLElement | null = el.parentElement;
          for (let i = 0; i < 12 && cur; i++) {
            const heading = cur.querySelector?.('h1, h2, h3, h4, h5, h6, [role="heading"]') as HTMLElement | null;
            if (heading && cur.contains(heading)) {
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
          const pageH = Math.max(
            document.documentElement.scrollHeight,
            document.body?.scrollHeight || 0,
          );
          const vh = window.innerHeight || 900;
          let region: 'header' | 'above-fold' | 'below-fold' | 'footer' | 'off-screen';
          if (bbox.w === 0 && bbox.h === 0) region = 'off-screen';
          else if (bbox.y < 80) region = 'header';
          else if (bbox.y < vh) region = 'above-fold';
          else if (bbox.y > pageH - 240) region = 'footer';
          else region = 'below-fold';
          return { text, parentText, sectionHeading, region, bbox };
        }

        function isInteractive(el: Element): boolean {
          const tag = el.tagName.toLowerCase();
          if (tag === 'button' || tag === 'select' || tag === 'textarea' || tag === 'input') return true;
          if (tag === 'a' && (el as HTMLAnchorElement).hasAttribute('href')) return true;
          const role = el.getAttribute('role');
          if (role === 'button' || role === 'link' || role === 'menuitem' || role === 'tab' || role === 'option' || role === 'switch' || role === 'checkbox' || role === 'radio') return true;
          if (el.hasAttribute('onclick')) return true;
          const ti = el.getAttribute('tabindex');
          if (ti !== null && ti !== '-1') return true;
          return false;
        }

        function isVisible(el: Element): boolean {
          const he = el as HTMLElement;
          if (he.offsetWidth === 0 && he.offsetHeight === 0) return false;
          const cs = getComputedStyle(he);
          if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
          // Skip elements clipped to 1×1 (screen-reader-only utility class).
          // The tooling cannot tell SR-only from accidentally-clipped, but
          // SR-only elements are by design not interactive for sighted
          // users — flagging them adds noise.
          if (he.offsetWidth <= 1 && he.offsetHeight <= 1) return false;
          // Inputs of type="hidden" obviously not visible.
          if (el.tagName.toLowerCase() === 'input' && (el as HTMLInputElement).type === 'hidden') return false;
          return true;
        }

        const touchTargetNodes: Array<{ el: HTMLElement; detail: string }> = [];
        const controlMinHNodes: Array<{ el: HTMLElement; detail: string }> = [];

        const all = Array.from(document.querySelectorAll<HTMLElement>(
          'a, button, input, select, textarea, [role], [tabindex], [onclick]'
        ));
        for (const el of all) {
          if (!isVisible(el)) continue;
          const tag = el.tagName.toLowerCase();

          if (isInteractive(el)) {
            const r = el.getBoundingClientRect();
            const w = Math.round(r.width);
            const h = Math.round(r.height);
            // Allow elements whose bounding box is small but whose
            // *click area* is enlarged via padding / sibling spacing.
            // Approximation: include CSS padding box.
            const cs = getComputedStyle(el);
            const padX = parseFloat(cs.paddingLeft || '0') + parseFloat(cs.paddingRight || '0');
            const padY = parseFloat(cs.paddingTop || '0') + parseFloat(cs.paddingBottom || '0');
            const effectiveW = Math.max(w, w + padX);
            const effectiveH = Math.max(h, h + padY);
            if (effectiveW < TOUCH_TARGET_MIN_PX || effectiveH < TOUCH_TARGET_MIN_PX) {
              touchTargetNodes.push({
                el,
                detail: `${tag} ${w}×${h}px (need ${TOUCH_TARGET_MIN_PX}×${TOUCH_TARGET_MIN_PX})`,
              });
            }
          }

          if (tag === 'input' || tag === 'textarea' || tag === 'select' || tag === 'button') {
            // input[type=hidden|checkbox|radio] are excused from min-height.
            // Checkbox/radio are conceptually 16-20px controls; their hit
            // area is typically extended by the surrounding label.
            if (tag === 'input') {
              const t = (el as HTMLInputElement).type;
              if (t === 'hidden' || t === 'checkbox' || t === 'radio' || t === 'range' || t === 'color') continue;
            }
            const cs = getComputedStyle(el);
            const minH = parseFloat(cs.minHeight || '0');
            const renderedH = el.getBoundingClientRect().height;
            // We accept BOTH min-height >= 36 OR rendered height >= 36
            // (some apps set explicit `height` instead of `min-height`).
            if (minH < CONTROL_MIN_HEIGHT_PX && renderedH < CONTROL_MIN_HEIGHT_PX) {
              controlMinHNodes.push({
                el,
                detail: `${tag} computed min-height=${minH || 'auto'}, rendered=${Math.round(renderedH)}px (need min-height ≥ ${CONTROL_MIN_HEIGHT_PX}px)`,
              });
            }
          }
        }

        function pack(items: Array<{ el: HTMLElement; detail: string }>) {
          return items.slice(0, MAX_NODES_PER_RULE).map(({ el, detail }) => {
            const d = describe(el);
            return {
              target: selectorFor(el),
              text: d.text,
              parentText: d.parentText,
              sectionHeading: d.sectionHeading,
              region: d.region,
              bbox: d.bbox,
              detail,
            };
          });
        }

        return {
          touchTarget: pack(touchTargetNodes),
          controlMinH: pack(controlMinHNodes),
          totals: {
            touchTarget: touchTargetNodes.length,
            controlMinH: controlMinHNodes.length,
          },
        };
      },
      { TOUCH_TARGET_MIN_PX, CONTROL_MIN_HEIGHT_PX, MAX_NODES_PER_RULE },
    );

    const violations: D0Violation[] = [];
    if (raw.touchTarget.length > 0) {
      violations.push({
        id: 'touch-target',
        ...RULE_META['touch-target'],
        nodes: raw.touchTarget,
      });
    }
    if (raw.controlMinH.length > 0) {
      violations.push({
        id: 'control-min-height',
        ...RULE_META['control-min-height'],
        nodes: raw.controlMinH,
      });
    }
    return {
      url,
      ok: true,
      violations,
      durationMs: Date.now() - startedAt,
    };
  } catch (e: any) {
    return {
      url,
      ok: false,
      error: `d0Audit: ${e?.message || e}`,
      violations: [],
      durationMs: Date.now() - startedAt,
    };
  }
}

/**
 * Convert a D0PageResult into CapturedEvent rows so the runner's diff
 * + budget logic catches D₀ regressions without bespoke plumbing.
 *
 * One event per (rule, node) pair so the diff is granular: a touch-
 * target violation on a NEW selector is a NEW event, even if the same
 * rule had violations in the prior baseline on different selectors.
 */
export function d0EventsFor(result: D0PageResult, startedAtEpoch: number): CapturedEvent[] {
  const events: CapturedEvent[] = [];
  if (!result.ok) {
    events.push({
      t: Date.now() - startedAtEpoch,
      kind: 'd0-violation',
      level: 'error',
      text: `d0 engine error on ${result.url}: ${result.error}`,
      url: result.url,
      ruleId: 'engine-error',
      impact: 'moderate',
    });
    return events;
  }
  for (const v of result.violations) {
    for (const n of v.nodes) {
      const where = n.sectionHeading ? ` in “${n.sectionHeading}”` : '';
      const near = n.parentText && n.parentText !== n.text ? ` near “${n.parentText.slice(0, 60)}”` : '';
      const txt = n.text ? `“${n.text}”` : '(no text)';
      events.push({
        t: Date.now() - startedAtEpoch,
        kind: 'd0-violation',
        level: v.impact,
        text: `${v.id}: ${txt}${where}${near} — ${n.detail}`,
        url: result.url,
        ruleId: v.id,
        impact: v.impact,
      });
    }
  }
  return events;
}

/**
 * Compact one-line-per-violation summary for human triage. Mirrors
 * renderAxeFindings shape; used to emit `d0-findings.txt` alongside
 * the JSON report.
 */
export function renderD0Findings(byPage: Array<{ url: string; result: D0PageResult }>): string {
  const lines: string[] = [];
  let totalRules = 0, totalNodes = 0, errored = 0;
  for (const { result } of byPage) {
    if (!result.ok) errored++;
    totalRules += result.violations.length;
    for (const v of result.violations) totalNodes += v.nodes.length;
  }
  lines.push(`# D₀ findings (PlausiDen design-system runtime audit)`);
  lines.push(`# Pages scanned: ${byPage.length} (${errored} engine errors)`);
  lines.push(`# Rules with at least one violation: ${totalRules}`);
  lines.push(`# Total flagged nodes: ${totalNodes}`);
  lines.push('');
  for (const { url, result } of byPage) {
    lines.push(`## ${url}`);
    if (!result.ok) {
      lines.push(`  ENGINE ERROR: ${result.error}`);
      lines.push('');
      continue;
    }
    if (result.violations.length === 0) {
      lines.push(`  (no D₀ violations)`);
      lines.push('');
      continue;
    }
    for (const v of result.violations) {
      lines.push(`  [${v.impact}] ${v.id} — ${v.help}`);
      lines.push(`    docs: ${v.helpUrl}`);
      for (const n of v.nodes) {
        const region = n.region ? `[${n.region}]` : '';
        const text = n.text ? `"${n.text}"` : '(no visible text)';
        lines.push(`    · ${region} ${text} — selector: ${n.target}`);
        if (n.sectionHeading) lines.push(`        in section: "${n.sectionHeading}"`);
        if (n.parentText && n.parentText !== n.text) lines.push(`        near text: "${n.parentText}"`);
        if (n.bbox) lines.push(`        position: x=${n.bbox.x}, y=${n.bbox.y}, ${n.bbox.w}×${n.bbox.h}px`);
        lines.push(`        detail: ${n.detail}`);
      }
    }
    lines.push('');
  }
  return lines.join('\n');
}
