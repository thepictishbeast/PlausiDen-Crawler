/**
 * linkText.ts — link-purpose detector. T106 (TS port).
 *
 * WCAG 2.4.4 (Link Purpose, AA): every link's purpose must be
 * determinable from the link text alone. Catches:
 *
 *   - link.empty-text     strict   visible link with no text +
 *                                  no aria-label / aria-labelledby /
 *                                  title. Screen readers announce
 *                                  "link" with no destination.
 *   - link.generic-text   warn     phrases like "click here" / "more"
 *                                  / "read more" — vague text that
 *                                  doesn't convey destination.
 *
 * Mirrors crates/crawler-detectors/src/link_text.rs — keep the
 * kind+severity strings in sync.
 */
import type { Page } from 'playwright';

export interface LinkTextFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedLink {
  selector: string;
  href: string;
  name: string;
}

export interface LinkTextSnapshot {
  pageUrl: string;
  links: CapturedLink[];
}

const GENERIC_PHRASES = [
  'click here',
  'click',
  'here',
  'read more',
  'more',
  'learn more',
  'details',
  'see more',
  'see details',
  'this',
  'this link',
  'link',
  'go',
  'next',
  'previous',
  'continue',
  '>',
  '>>',
  '..',
  '...',
];

export async function captureLinkTextSnapshot(
  page: Page,
): Promise<LinkTextSnapshot> {
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

    const accessibleName = function(el) {
      const labelledby = el.getAttribute('aria-labelledby');
      if (labelledby) {
        const ids = labelledby.split(/\\s+/).filter(Boolean);
        const parts = [];
        for (const id of ids) {
          const ref = document.getElementById(id);
          if (ref) parts.push((ref.textContent || '').trim());
        }
        const joined = parts.join(' ').trim();
        if (joined) return joined;
      }
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim();
      const text = (el.textContent || '').trim();
      if (text) return text;
      const title = el.getAttribute('title');
      if (title && title.trim()) return title.trim();
      return '';
    };

    const links = [];
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const el = anchors[i];
      if (!isVisible(el)) continue;
      const name = accessibleName(el);
      const href = el.getAttribute('href') || '';
      links.push({ selector: selectorOf(el), href: href, name: name.slice(0, 120) });
    }
    return { links: links };
  })()`;

  const result = (await page.evaluate(evalFn)) as { links: CapturedLink[] };
  return { pageUrl, links: result.links };
}

export function detectLinkTextIssues(snap: LinkTextSnapshot): LinkTextFinding[] {
  const out: LinkTextFinding[] = [];
  const emptyExamples: string[] = [];
  const genericExamples: string[] = [];
  let emptyCount = 0;
  let genericCount = 0;

  for (const link of snap.links) {
    const name = link.name.trim();
    if (name === '') {
      emptyCount++;
      if (emptyExamples.length < 5) {
        emptyExamples.push(`${link.selector} → href=${link.href}`);
      }
      continue;
    }
    const lower = name.toLowerCase();
    if (GENERIC_PHRASES.includes(lower)) {
      genericCount++;
      if (genericExamples.length < 5) {
        genericExamples.push(`'${name}' → href=${link.href}`);
      }
    }
  }

  if (emptyCount > 0) {
    out.push({
      severity: 'strict',
      kind: 'link.empty-text',
      detail: `${emptyCount} visible link(s) have no accessible name (no textContent, aria-label, aria-labelledby, or title). Screen-reader users hear 'link' with no destination context. Examples: ${emptyExamples.join('; ')}`,
      evidence: { emptyCount, emptyExamples },
    });
  }
  if (genericCount > 0) {
    out.push({
      severity: 'warn',
      kind: 'link.generic-text',
      detail: `${genericCount} link(s) have generic text (e.g. 'click here', 'read more') that doesn't convey destination. WCAG 2.4.4 — link purpose must be determinable from the link text. Examples: ${genericExamples.join('; ')}`,
      evidence: { genericCount, genericExamples },
    });
  }

  return out;
}

export async function checkLinkText(
  page: Page,
): Promise<{ snapshot: LinkTextSnapshot; findings: LinkTextFinding[] }> {
  const snapshot = await captureLinkTextSnapshot(page);
  const findings = detectLinkTextIssues(snapshot);
  return { snapshot, findings };
}
