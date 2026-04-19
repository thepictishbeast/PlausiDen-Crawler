/**
 * EasySpider flow importer.
 *
 * EasySpider is a visual no-code crawler builder. It exports flows as
 * `.es.json` files with a specific schema. This importer reads an ES
 * flow and converts it into our internal Journey schema so non-
 * engineers can author journeys visually (using the ES UI) and still
 * run them through the PlausiDen crawler.
 *
 * ES isn't absorbed as a runtime dep — it's a browser extension + desktop
 * app, not a library. Only the flow format is consumed. Reference:
 *   https://github.com/NaiboWang/EasySpider (AGPL-3.0 — flow JSON usage
 *   does not require AGPL compliance since we don't ship ES code.)
 *
 * Full schema support is v0.4 work. Today supports the common subset:
 * navigate / click / input / wait / extract.
 */
import type { Journey, Step } from '../../journey';

interface EasySpiderNode {
  id: number;
  option: number;           // ES action code
  title?: string;
  url?: string;              // for navigate
  target?: string;           // XPath/CSS selector
  value?: string;            // for input
  waitTime?: number;         // ms
}

interface EasySpiderFlow {
  name?: string;
  url?: string;              // starting URL
  graph?: { nodes?: EasySpiderNode[] };
}

const ES_ACTION: Record<number, Step['kind']> = {
  1: 'goto',                 // navigate
  4: 'click',
  6: 'fill',                 // input
  9: 'wait',
  13: 'screenshot',
  // extract, submit, etc. — add as needed.
};

export function importEasySpiderFlow(json: string | EasySpiderFlow): Journey {
  const flow: EasySpiderFlow = typeof json === 'string' ? JSON.parse(json) : json;
  const nodes = flow?.graph?.nodes || [];
  const steps: Step[] = [];
  for (const n of nodes) {
    const kind = ES_ACTION[n.option];
    if (!kind) continue; // unknown ES action — skip
    const step: Step = { kind, label: n.title || `node-${n.id}` };
    if (n.url) step.url = n.url;
    if (n.target) step.selector = cssFromXPath(n.target);
    if (n.value) step.text = n.value;
    if (n.waitTime) step.ms = n.waitTime;
    steps.push(step);
  }
  return {
    name: flow.name || 'imported-es-flow',
    description: 'Imported from an EasySpider flow',
    baseUrl: flow.url || '',
    steps,
  };
}

/**
 * ES emits XPath selectors by default. Playwright accepts XPath via
 * the `xpath=` prefix, but our Journey schema uses plain CSS. Convert
 * where trivial; otherwise pass through with the xpath= prefix so
 * Playwright resolves it.
 */
function cssFromXPath(xp: string): string {
  // The common XPath shapes ES emits are //*[@id='foo']
  const idMatch = xp.match(/^\/\/[*\w]+\[@id=['"]([^'"]+)['"]\]$/);
  if (idMatch) return `#${CSS.escape ? CSS.escape(idMatch[1]) : idMatch[1]}`;
  return `xpath=${xp}`;
}
