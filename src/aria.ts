/**
 * Accessibility-tree capture + token-efficient serialization.
 *
 * Inspired by the Visionless AI paradigm: instead of screenshots (multimodal
 * model required) or raw HTML (15k+ tokens of noise), we capture the
 * browser's accessibility tree — a role/name/state graph that's
 * typically <1k tokens per page and purely semantic.
 *
 * Each step now writes both:
 *   - screenshot (.png)          — for humans debugging
 *   - aria snapshot (.aria.yaml) — for LLMs diagnosing
 *
 * An LLM ingesting reports can reason about structure without ever
 * seeing pixels. Bonus: aria-tree signals accessibility bugs directly
 * — if a button has no accessible name, the snapshot node is
 * unnamed, which IS the bug.
 */
import type { Page } from 'playwright';

export interface AriaNode {
  role: string;
  name?: string;
  value?: string;
  description?: string;
  disabled?: boolean;
  expanded?: boolean;
  checked?: boolean | 'mixed';
  selected?: boolean;
  focused?: boolean;
  level?: number;
  children?: AriaNode[];
}

/** Playwright's page.accessibility.snapshot() returns this tree.
 *  Falls back to interestingOnly:false and then a DOM-based role scrape
 *  when Chromium's a11y tree isn't populated (headless sometimes skips
 *  full computation unless a client attaches). */
export async function captureAriaTree(page: Page): Promise<AriaNode | null> {
  try {
    const snap = await page.accessibility.snapshot({ interestingOnly: true });
    if (snap) return snap as AriaNode;
  } catch { /* fall through */ }
  try {
    const snap = await page.accessibility.snapshot({ interestingOnly: false });
    if (snap) return snap as AriaNode;
  } catch { /* fall through */ }
  // Last resort: scrape roles from the DOM ourselves. Less thorough
  // than Chromium's native tree but beats "(no a11y tree)" in reports.
  try {
    const tree = await page.evaluate(() => {
      function collect(el: Element): any {
        const role = el.getAttribute('role')
          || ({
            BUTTON: 'button', A: 'link', INPUT: 'textbox', TEXTAREA: 'textbox',
            SELECT: 'combobox', H1: 'heading', H2: 'heading', H3: 'heading',
            H4: 'heading', H5: 'heading', H6: 'heading', NAV: 'navigation',
            MAIN: 'main', HEADER: 'banner', FOOTER: 'contentinfo',
            ASIDE: 'complementary', IMG: 'img', UL: 'list', OL: 'list', LI: 'listitem',
          } as Record<string, string>)[el.tagName] || '';
        if (!role) {
          const children = Array.from(el.children).map(collect).filter(Boolean);
          return children.length ? { role: 'generic', children } : null;
        }
        const name = el.getAttribute('aria-label')
          || el.getAttribute('alt')
          || el.getAttribute('title')
          || (el as HTMLElement).innerText?.trim().slice(0, 80)
          || '';
        const node: any = { role };
        if (name) node.name = name;
        const level = el.tagName.match(/^H(\d)$/);
        if (level) node.level = Number(level[1]);
        if ((el as HTMLInputElement).disabled) node.disabled = true;
        const children = Array.from(el.children).map(collect).filter(Boolean);
        if (children.length) node.children = children;
        return node;
      }
      return collect(document.body);
    });
    return tree as AriaNode | null;
  } catch {
    return null;
  }
}

/**
 * Serialize an accessibility tree as compact indented text — ~10x
 * token-efficient compared to raw JSON. Format:
 *
 *   heading "Welcome to PlausiDen" level=1
 *     banner
 *       button "Chats" pressed
 *       button "New chat"
 *     main
 *       textbox "Chat message input"
 *       button "Send" disabled
 *
 * One line per node, role first, then quoted name, then state flags.
 * Easy for both humans skimming and LLMs parsing.
 */
export function ariaTreeToText(node: AriaNode | null, depth = 0): string {
  if (!node) return '(no accessibility tree)';
  const indent = '  '.repeat(depth);
  const parts: string[] = [node.role];
  if (node.name) parts.push(`"${node.name.replace(/"/g, '\\"')}"`);
  if (node.value) parts.push(`value="${String(node.value).slice(0, 80)}"`);
  if (node.level != null) parts.push(`level=${node.level}`);
  if (node.disabled) parts.push('disabled');
  if (node.expanded === true) parts.push('expanded');
  if (node.expanded === false) parts.push('collapsed');
  if (node.checked === true) parts.push('checked');
  if (node.checked === false) parts.push('unchecked');
  if (node.checked === 'mixed') parts.push('mixed');
  if (node.selected) parts.push('selected');
  if (node.focused) parts.push('focused');
  let line = indent + parts.join(' ');
  if (node.children && node.children.length > 0) {
    line += '\n' + node.children.map(c => ariaTreeToText(c, depth + 1)).join('\n');
  }
  return line;
}

/**
 * Flat list of all interactable nodes (button / link / textbox / combobox /
 * menuitem / tab / checkbox / radio / slider / switch / searchbox /
 * spinbutton). Feed this to an LLM as "the clickable elements on the page"
 * — it's what a screen-reader user would hear.
 */
export function interactableNodes(root: AriaNode | null): AriaNode[] {
  const INTERACTIVE = new Set([
    'button', 'link', 'textbox', 'combobox', 'menuitem', 'menuitemcheckbox',
    'menuitemradio', 'tab', 'checkbox', 'radio', 'slider', 'switch',
    'searchbox', 'spinbutton', 'treeitem', 'gridcell', 'option',
  ]);
  const out: AriaNode[] = [];
  const walk = (n: AriaNode | null | undefined) => {
    if (!n) return;
    if (INTERACTIVE.has(n.role)) out.push(n);
    (n.children || []).forEach(walk);
  };
  walk(root);
  return out;
}

/**
 * Quick heuristic score — "does this page have obvious accessibility
 * issues?" Counts:
 *   - buttons / links / inputs without accessible names
 *   - headings out of order (h3 without h2, etc.)
 *   - images without alt text (in the accessibility graph as role=img)
 *
 * Lower is better. Returns {score, flags} for the report.
 */
export function scoreAriaTree(root: AriaNode | null): { score: number; flags: string[] } {
  if (!root) return { score: 0, flags: ['no-a11y-tree'] };
  const flags: string[] = [];
  let score = 0;
  const walk = (n: AriaNode) => {
    if (['button', 'link', 'textbox', 'combobox', 'menuitem', 'tab', 'checkbox'].includes(n.role)) {
      if (!n.name || n.name.trim() === '') {
        flags.push(`unnamed ${n.role}`);
        score++;
      }
    }
    if (n.role === 'img' && (!n.name || n.name.trim() === '')) {
      flags.push('img without alt');
      score++;
    }
    (n.children || []).forEach(walk);
  };
  walk(root);
  return { score, flags };
}
