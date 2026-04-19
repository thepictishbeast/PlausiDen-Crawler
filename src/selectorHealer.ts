/**
 * 10-tier priority selector resolver ("self-healing" selectors).
 *
 * From the Visionless AI Paradigm: brittle CSS selectors break when
 * the DOM shifts. A durable selector falls back through a hierarchy of
 * signals ordered by stability. Given a TargetSpec, try each in order,
 * return the first that resolves.
 *
 * Priority (most stable → least stable):
 *   1. role + accessible name        (W3C standard, language-independent)
 *   2. data-testid                    (engineer-authored, stable)
 *   3. id                             (scoped to page)
 *   4. aria-label                     (screen-reader-visible name)
 *   5. aria-describedby target text   (richer context)
 *   6. name attribute (forms)         (stable for inputs)
 *   7. placeholder text
 *   8. visible text (for buttons/links)
 *   9. CSS class fragment             (brittle but sometimes the only hook)
 *  10. explicit CSS / XPath fallback  (last resort)
 *
 * Caller provides whichever subset of fields they know.
 */
import type { Page, Locator } from 'playwright';

export interface TargetSpec {
  role?: string;
  name?: string;
  testid?: string;
  id?: string;
  ariaLabel?: string;
  ariaDescribedByText?: string;
  nameAttr?: string;
  placeholder?: string;
  visibleText?: string;
  classFragment?: string;
  css?: string;
  xpath?: string;
}

export interface ResolvedLocator {
  strategy: string;
  locator: Locator;
}

/**
 * Return the first strategy that resolves to a non-empty locator. Each
 * strategy is short-circuit; the caller can then .click / .fill / etc.
 *
 * Throws only if every strategy yielded zero matches — in that case
 * surfacing the list of tried strategies aids debugging (and is what
 * the self-healing loop logs).
 */
export async function resolve(page: Page, spec: TargetSpec): Promise<ResolvedLocator> {
  const tried: string[] = [];
  const tryIt = async (strategy: string, build: () => Locator): Promise<ResolvedLocator | null> => {
    tried.push(strategy);
    try {
      const loc = build();
      const n = await loc.count();
      if (n >= 1) return { strategy, locator: loc.first() };
    } catch { /* try next */ }
    return null;
  };

  let r: ResolvedLocator | null;

  // 1. role + name
  if (spec.role && spec.name) {
    r = await tryIt(`getByRole('${spec.role}',name='${spec.name}')`,
      () => page.getByRole(spec.role as any, { name: spec.name }));
    if (r) return r;
  }

  // 2. data-testid
  if (spec.testid) {
    r = await tryIt(`data-testid='${spec.testid}'`,
      () => page.locator(`[data-testid="${CSSesc(spec.testid!)}"]`));
    if (r) return r;
  }

  // 3. id
  if (spec.id) {
    r = await tryIt(`#${spec.id}`,
      () => page.locator(`#${CSSesc(spec.id!)}`));
    if (r) return r;
  }

  // 4. aria-label
  if (spec.ariaLabel) {
    r = await tryIt(`aria-label='${spec.ariaLabel}'`,
      () => page.locator(`[aria-label="${CSSesc(spec.ariaLabel!)}"]`));
    if (r) return r;
  }

  // 5. aria-describedby's target text
  if (spec.ariaDescribedByText) {
    r = await tryIt(`aria-describedby text match`,
      () => page.locator(`[aria-describedby]`).filter({ hasText: spec.ariaDescribedByText! }));
    if (r) return r;
  }

  // 6. name attribute
  if (spec.nameAttr) {
    r = await tryIt(`name='${spec.nameAttr}'`,
      () => page.locator(`[name="${CSSesc(spec.nameAttr!)}"]`));
    if (r) return r;
  }

  // 7. placeholder
  if (spec.placeholder) {
    r = await tryIt(`getByPlaceholder='${spec.placeholder}'`,
      () => page.getByPlaceholder(spec.placeholder!));
    if (r) return r;
  }

  // 8. visible text
  if (spec.visibleText) {
    r = await tryIt(`getByText='${spec.visibleText}'`,
      () => page.getByText(spec.visibleText!, { exact: false }));
    if (r) return r;
  }

  // 9. class fragment
  if (spec.classFragment) {
    r = await tryIt(`.${spec.classFragment}`,
      () => page.locator(`[class*="${CSSesc(spec.classFragment!)}"]`));
    if (r) return r;
  }

  // 10. explicit CSS / XPath
  if (spec.css) {
    r = await tryIt(`css:${spec.css}`, () => page.locator(spec.css!));
    if (r) return r;
  }
  if (spec.xpath) {
    r = await tryIt(`xpath:${spec.xpath}`, () => page.locator(`xpath=${spec.xpath}`));
    if (r) return r;
  }

  throw new Error(`SelectorHealer: every strategy missed. Tried: ${tried.join(' → ')}`);
}

const CSSesc = (s: string): string => s.replace(/[\\"]/g, '\\$&');
