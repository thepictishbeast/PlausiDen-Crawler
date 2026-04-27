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
// A Playwright snapshot that says {role:"WebArea", children:[]} is
// useless — the tree is present but empty. Descend one level and
// confirm at least one non-generic role exists before accepting.
function hasRealContent(n: AriaNode | null | undefined): boolean {
  if (!n) return false;
  const walk = (x: AriaNode): boolean => {
    if (x.role && x.role !== 'WebArea' && x.role !== 'generic' && x.role !== 'document') return true;
    if (x.name && x.name.length > 0 && x.role !== 'WebArea') return true;
    for (const c of x.children || []) if (walk(c)) return true;
    return false;
  };
  return walk(n);
}

export async function captureAriaTree(page: Page): Promise<AriaNode | null> {
  try {
    const snap = await page.accessibility.snapshot({ interestingOnly: true });
    if (hasRealContent(snap as AriaNode)) return snap as AriaNode;
  } catch { /* fall through */ }
  try {
    const snap = await page.accessibility.snapshot({ interestingOnly: false });
    if (hasRealContent(snap as AriaNode)) return snap as AriaNode;
  } catch { /* fall through */ }
  // Last resort: scrape roles from the DOM ourselves. Less thorough
  // than Chromium's native tree but beats "(no a11y tree)" in reports.
  //
  // Design notes (2026-04-19):
  //  - Never return null at the root. An SPA's document.body almost
  //    always has SOME structured content worth logging; returning null
  //    here was why the smoke-journey was emitting "(no accessibility
  //    tree)" for every step.
  //  - Expand tag→role mapping to include: FORM/SECTION/ARTICLE/DIALOG
  //    and treat any tag carrying aria-role / aria-label / tabindex as
  //    interactable-ish.
  //  - Preserve element visibility: skip display:none + hidden + zero
  //    client-rect so off-screen menus don't pollute the tree.
  //  - Cap tree size to 5000 nodes to avoid an OOM on pathological
  //    pages. Truncate depth-first, remember count via a shared counter.
  try {
    // IMPORTANT: pass body as a raw STRING, not a TS arrow function.
    // tsx/esbuild keepNames=true wraps const-assigned arrows + function
    // decls with __name(fn, "name") for stack traces. When Playwright
    // serializes a compiled arrow via toString() that wrapping is
    // already baked in — the browser sees __name / __name2 / __name3
    // references with no shim possible (esbuild renames the shim too).
    // The only reliable way to run arbitrary JS in the page without
    // going through esbuild's transform is to build the source as a
    // string. page.evaluate(str) is explicitly supported by Playwright.
    const domFallbackSrc = `(() => {
      const TAG_ROLE = {
        BUTTON:"button", A:"link", INPUT:"textbox", TEXTAREA:"textbox",
        SELECT:"combobox", H1:"heading", H2:"heading", H3:"heading",
        H4:"heading", H5:"heading", H6:"heading", NAV:"navigation",
        MAIN:"main", HEADER:"banner", FOOTER:"contentinfo",
        ASIDE:"complementary", IMG:"img", UL:"list", OL:"list",
        LI:"listitem", FORM:"form", SECTION:"region", ARTICLE:"article",
        DIALOG:"dialog", LABEL:"label", SUMMARY:"button",
        DETAILS:"group", FIGURE:"figure", TABLE:"table"
      };
      const MAX_NODES = 5000;
      const nodeCountBox = { n: 0 };
      const isVisible = function(el) {
        try {
          if (el.hidden) return false;
          const rect = el.getBoundingClientRect && el.getBoundingClientRect();
          if (rect && rect.width === 0 && rect.height === 0) {
            if (!el.offsetParent && el.children.length === 0) return false;
          }
          const style = typeof getComputedStyle === "function" ? getComputedStyle(el) : null;
          if (style && (style.display === "none" || style.visibility === "hidden")) return false;
        } catch(e) { /* ignore */ }
        return true;
      };
      const textOf = function(el, cap) {
        if (cap === undefined) cap = 120;
        // Order matches the W3C HTML Accessible Name & Description
        // computation: aria-labelledby, aria-label, host-language label
        // associations (label[for=], wrapping <label>, alt, title), then
        // text content as a last resort.
        let raw = "";
        const tag = el.tagName;
        const isFormControl = tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";

        const aliBy = el.getAttribute && el.getAttribute("aria-labelledby");
        if (aliBy) {
          const ids = aliBy.split(/\\s+/).filter(Boolean);
          const parts = [];
          for (let i = 0; i < ids.length; i++) {
            const ref = document.getElementById(ids[i]);
            if (ref) parts.push((ref.innerText || ref.textContent || "").trim());
          }
          if (parts.length) raw = parts.join(" ");
        }

        if (!raw) raw = el.getAttribute("aria-label") || "";

        if (!raw && isFormControl) {
          // <label for="x"> association — the spec way to name a form
          // control. Also handle <label><input/></label> wrapping.
          const id = el.getAttribute("id");
          if (id) {
            try {
              const sel = 'label[for="' + id.replace(/"/g, '\\\\"') + '"]';
              const lbl = document.querySelector(sel);
              if (lbl) raw = (lbl.innerText || lbl.textContent || "").toString();
            } catch (e) { /* invalid selector — fall through */ }
          }
          if (!raw) {
            // Wrapping label: walk up looking for an ancestor <label>.
            let p = el.parentElement;
            for (let depth = 0; p && depth < 4; depth++) {
              if (p.tagName === "LABEL") {
                raw = (p.innerText || p.textContent || "").toString();
                break;
              }
              p = p.parentElement;
            }
          }
          if (!raw) raw = el.getAttribute("placeholder") || "";
        }

        if (!raw) {
          raw = (el.getAttribute("alt")
            || el.getAttribute("title")
            || el.innerText
            || el.textContent || "").toString();
        }

        return raw.replace(/\\s+/g, " ").trim().slice(0, cap);
      };
      const collect = function(el) {
        if (nodeCountBox.n >= MAX_NODES) return null;
        if (!isVisible(el)) return null;
        const tag = el.tagName;
        if (tag === "SCRIPT" || tag === "STYLE" || tag === "NOSCRIPT") return null;
        const attrRole = el.getAttribute("role");
        const role = attrRole || TAG_ROLE[tag] || "";
        const ariaLabel = el.getAttribute("aria-label") || "";
        const hasInteractableHint = !!ariaLabel
          || el.hasAttribute("aria-labelledby")
          || el.hasAttribute("tabindex")
          || el.hasAttribute("data-testid");
        const children = [];
        const kids = Array.from(el.children);
        for (let i = 0; i < kids.length; i++) {
          if (nodeCountBox.n >= MAX_NODES) break;
          const r = collect(kids[i]);
          if (r) children.push(r);
        }
        if (!role && !hasInteractableHint) {
          if (children.length === 1) return children[0];
          if (children.length > 1) {
            nodeCountBox.n += 1;
            return { role: "generic", children: children };
          }
          return null;
        }
        const node = { role: role || "generic" };
        const name = textOf(el);
        if (name) node.name = name;
        const levelMatch = tag.match(/^H(\\d)$/);
        if (levelMatch) node.level = Number(levelMatch[1]);
        if (el.disabled) node.disabled = true;
        const ariaExpanded = el.getAttribute("aria-expanded");
        if (ariaExpanded != null) node.expanded = ariaExpanded === "true";
        const ariaChecked = el.getAttribute("aria-checked");
        if (ariaChecked != null) {
          node.checked = ariaChecked === "mixed" ? "mixed" : ariaChecked === "true";
        }
        const ariaSelected = el.getAttribute("aria-selected");
        if (ariaSelected === "true") node.selected = true;
        if (children.length) node.children = children;
        nodeCountBox.n += 1;
        return node;
      };
      const body = document.body;
      if (!body) return null;
      const rootChildren = [];
      const topKids = Array.from(body.children);
      for (let i = 0; i < topKids.length; i++) {
        const r = collect(topKids[i]);
        if (r) rootChildren.push(r);
      }
      return {
        role: "document",
        name: document.title || undefined,
        children: rootChildren.length ? rootChildren : undefined
      };
    })()`;
    const tree = await page.evaluate(domFallbackSrc);
    return tree as AriaNode | null;
  } catch (e: any) {
    // Don't swallow silently — report to stderr so the operator knows
    // why reports show "(no accessibility tree)". 2026-04-19: the first
    // diagnostic run showed that Playwright was returning a valid but
    // empty {role:"WebArea"} tree which the caller accepted; hasRealContent
    // now forces us into this branch, but if page.evaluate fails (navigation
    // in progress, CSP block, etc.) we want a visible signal.
    try { console.warn('[aria] DOM fallback failed:', (e?.message || e).toString().slice(0, 200)); } catch { /* ignore */ }
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
