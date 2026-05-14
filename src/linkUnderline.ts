/**
 * linkUnderline.ts — link distinguishability detector. T76.
 *
 * WCAG 1.4.1 (Use of Color, Level A): "Color is not used as the
 * only visual means of conveying information... or distinguishing a
 * visual element". Links inside running text rely on this rule —
 * if the only visual cue is a different colour, ~8% of the male
 * population (red-green colour-blind) and many low-contrast
 * environments (sunlight, projection, e-ink) can't tell what's a
 * link and what's body text.
 *
 * The canonical fix is to keep `text-decoration: underline` on
 * inline links, OR add a distinguishing feature like bold weight,
 * a small icon, an outline, or a meaningfully different background.
 *
 * Findings:
 *
 *   - link.color-only-distinction      warn
 *     A visible inline anchor (link inside paragraph / list-item /
 *     definition-description text) has `text-decoration: none` AND
 *     `font-weight` matching the surrounding text AND no
 *     border / outline / background distinguishing it. Sole cue
 *     is colour.
 *
 * Out of scope (not flagged):
 *   * Block-level links (nav items, button-like CTAs, card links).
 *     The user knows they're clickable from layout context.
 *   * Links inside <header>, <nav>, <footer>, <aside> — those are
 *     conventionally button-like; visual cues from surrounding
 *     chrome are sufficient.
 *   * Links with explicit visual distinction (icon child, bold,
 *     box-shadow, border, background colour different from parent).
 *
 * The detector compares each candidate link's computed style to
 * the EFFECTIVE surrounding text colour and weight at the parent
 * element. If the link contrasts only via `color` and shares
 * `font-weight` + `text-decoration: none`, it's flagged.
 *
 * Mirror: crates/crawler-detectors/src/link_underline.rs.
 */
import type { Page } from 'playwright';

export interface LinkUnderlineFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedColorOnlyLink {
  selector: string;
  /** Link text, first 60 chars. */
  text: string;
  /** Computed text-decoration-line value. */
  textDecoration: string;
  /** Computed font-weight (resolved to a number string e.g. "400"). */
  fontWeight: string;
  /** Parent computed font-weight for comparison. */
  parentFontWeight: string;
  /** href value. */
  href: string;
}

export interface LinkUnderlineSnapshot {
  pageUrl: string;
  candidates: CapturedColorOnlyLink[];
}

export async function captureLinkUnderlineSnapshot(
  page: Page,
): Promise<LinkUnderlineSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };

    // Walk up from the link to the nearest meaningful ancestor.
    // We only flag links INSIDE running text — paragraph, list
    // item, definition description, blockquote, article body.
    // Header / nav / footer / aside contain conventionally
    // button-styled links; layout context is sufficient.
    const isInsideRunningText = function(el) {
      const runningTags = ['P', 'LI', 'DD', 'BLOCKQUOTE', 'TD', 'TH'];
      let parent = el.parentElement;
      let hops = 0;
      let foundChrome = false;
      while (parent && hops < 8) {
        const tag = parent.tagName;
        if (tag === 'NAV' || tag === 'HEADER' || tag === 'FOOTER' || tag === 'ASIDE') {
          foundChrome = true;
        }
        if (runningTags.indexOf(tag) >= 0) {
          // Found a running-text ancestor. If we ALSO found chrome
          // earlier in the walk, the running-text element is INSIDE
          // chrome (e.g. a <p> inside <footer>) — still chrome-y.
          return !foundChrome;
        }
        parent = parent.parentElement;
        hops += 1;
      }
      return false;
    };

    /**
     * Detect any visual cue beyond colour. Each returns true if
     * the link is distinguishable on something other than colour.
     */
    const hasNonColorDistinction = function(el, cs, parentCs) {
      // 1. Underline (canonical fix).
      const td = cs.textDecorationLine || cs.textDecoration || '';
      if (td.indexOf('underline') >= 0) return true;
      // 2. Weight contrast (≥ 200 unit difference).
      const lw = parseFloat(cs.fontWeight) || 400;
      const pw = parseFloat(parentCs.fontWeight) || 400;
      if (Math.abs(lw - pw) >= 200) return true;
      // 3. Border / outline.
      const bw = parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth)
              + parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
      if (bw > 0) return true;
      // outlineWidth can be a non-zero default (3px focus ring) even
      // when outline-style is "none" — browsers compute a width
      // value the focus ring WOULD use without actually drawing it.
      // Check style too.
      const ow = parseFloat(cs.outlineWidth);
      if (ow > 0 && cs.outlineStyle && cs.outlineStyle !== 'none') return true;
      // 4. Different background from parent.
      if (cs.backgroundColor && cs.backgroundColor !== 'rgba(0, 0, 0, 0)' &&
          cs.backgroundColor !== 'transparent' &&
          cs.backgroundColor !== parentCs.backgroundColor) {
        return true;
      }
      // 5. Box-shadow (gives a visual edge).
      if (cs.boxShadow && cs.boxShadow !== 'none') return true;
      // 6. Italic style (when parent is not).
      if (cs.fontStyle === 'italic' && parentCs.fontStyle !== 'italic') return true;
      // 7. An ICON child (inline svg, img, or i tag).
      if (el.querySelector('svg, img, i.icon, [class*="icon"]')) return true;
      return false;
    };

    const out = [];
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const el = anchors[i];
      if (!isVisible(el)) continue;
      if (!isInsideRunningText(el)) continue;
      const cs = window.getComputedStyle(el);
      const parentEl = el.parentElement;
      if (!parentEl) continue;
      const parentCs = window.getComputedStyle(parentEl);
      if (hasNonColorDistinction(el, cs, parentCs)) continue;

      const txt = (el.textContent || '').trim().slice(0, 60);
      const td = cs.textDecorationLine || cs.textDecoration || '';
      out.push({
        selector: selectorOf(el),
        text: txt,
        textDecoration: td,
        fontWeight: cs.fontWeight,
        parentFontWeight: parentCs.fontWeight,
        href: el.getAttribute('href') || '',
      });
    }
    return { candidates: out };
  })()`;

  const result = (await page.evaluate(evalFn)) as { candidates: CapturedColorOnlyLink[] };
  return { pageUrl, candidates: result.candidates };
}

export function detectLinkUnderlineIssues(
  snap: LinkUnderlineSnapshot,
): LinkUnderlineFinding[] {
  if (snap.candidates.length === 0) return [];
  const examples = snap.candidates.slice(0, 5).map((c) => {
    const t = c.text || '(no text)';
    return `${c.selector} '${t}' → ${c.href}`;
  });
  return [
    {
      severity: 'warn',
      kind: 'link.color-only-distinction',
      detail: `${snap.candidates.length} inline link(s) inside running text are distinguished from surrounding text ONLY by colour — text-decoration is none, font-weight matches the parent, no border / outline / background / icon. WCAG 1.4.1 (Use of Color, A): ~8% of users (red-green colourblind) and many low-contrast environments can't see the difference. Add 'text-decoration: underline' or another visual cue. Examples: ${examples.join('; ')}`,
      evidence: { count: snap.candidates.length, examples },
    },
  ];
}

export async function checkLinkUnderline(
  page: Page,
): Promise<{
  snapshot: LinkUnderlineSnapshot;
  findings: LinkUnderlineFinding[];
}> {
  const snapshot = await captureLinkUnderlineSnapshot(page);
  const findings = detectLinkUnderlineIssues(snapshot);
  return { snapshot, findings };
}
