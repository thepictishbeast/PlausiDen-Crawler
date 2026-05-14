/**
 * outboundLinks.ts — outbound-link safety detector. T76.
 *
 * SECURITY: a `<a target="_blank" href="https://other-site/">` link
 * without `rel="noopener"` opens the destination with a back-pointer
 * (`window.opener`) that lets the destination page navigate the
 * ORIGINAL tab to anywhere it wants. This is "tabnabbing" — the
 * canonical phishing escalation. The destination doesn't need a
 * 0day; it just needs JS.
 *
 * Modern browsers (Chrome 88+ / Firefox 79+ / Safari 12.1+) default
 * target=_blank to noopener implicitly, BUT:
 *   * older browsers (LTS Linux distros, embedded views, in-app
 *     browsers) don't.
 *   * `<a target="_blank" rel="opener">` explicitly opts BACK IN to
 *     the vulnerable behaviour and would not be caught by browser
 *     defaults.
 *   * the project's threat model assumes adversaries can downgrade
 *     the user's browser (state-actor doctrine in CLAUDE.md).
 *
 * So a defence-in-depth audit must surface every outbound _blank
 * link that doesn't have `rel="noopener"` (and the related
 * `noreferrer`).
 *
 * Findings:
 *
 *   - link.tabnab-vulnerable           strict
 *     `<a target="_blank" href="//other-origin/">` without
 *     `rel="noopener"`. Tabnabbing risk.
 *
 *   - link.opener-explicit             strict
 *     `<a target="_blank" rel="opener">` — explicitly opts back in
 *     to the vulnerable behaviour.
 *
 *   - link.outbound-no-noreferrer      warn
 *     Outbound link doesn't set `rel="noreferrer"`. Leaks the
 *     current URL (and thus session-token query params, if any) to
 *     the destination's analytics. Less severe than tabnabbing —
 *     warn only because legitimate use cases exist (intentional
 *     attribution links).
 *
 * "Outbound" = different origin from the current page. Same-origin
 * `target="_blank"` is fine (no tabnab risk).
 *
 * Mirror: crates/crawler-detectors/src/outbound_links.rs.
 */
import type { Page } from 'playwright';

export interface OutboundLinkFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedOutboundLink {
  /** Best-effort CSS selector. */
  selector: string;
  /** href attribute value. */
  href: string;
  /** target attribute value. */
  target: string;
  /** Lowercased rel tokens. */
  rel: string[];
  /** True iff the destination origin differs from the page origin. */
  outbound: boolean;
}

export interface OutboundLinksSnapshot {
  pageUrl: string;
  pageOrigin: string;
  links: CapturedOutboundLink[];
}

export async function captureOutboundLinksSnapshot(
  page: Page,
): Promise<OutboundLinksSnapshot> {
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

    const pageOrigin = window.location.origin;
    const out = [];
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const a = anchors[i];
      const href = a.getAttribute('href') || '';
      // Resolve href against page baseURI to determine origin.
      let absoluteUrl;
      try {
        absoluteUrl = new URL(href, document.baseURI);
      } catch (e) {
        // Skip malformed URLs (mailto:, tel:, javascript:, data:).
        continue;
      }
      // Only http(s) outbound links matter for tabnab.
      if (absoluteUrl.protocol !== 'http:' && absoluteUrl.protocol !== 'https:') continue;
      const outbound = absoluteUrl.origin !== pageOrigin;
      if (!outbound) continue;

      const target = (a.getAttribute('target') || '').toLowerCase();
      const relRaw = (a.getAttribute('rel') || '').toLowerCase();
      const rel = relRaw ? relRaw.split(/\\s+/).filter(Boolean) : [];

      out.push({
        selector: selectorOf(a),
        href: absoluteUrl.href,
        target: target,
        rel: rel,
        outbound: true,
      });
    }
    return { pageOrigin: pageOrigin, links: out };
  })()`;

  const result = (await page.evaluate(evalFn)) as {
    pageOrigin: string;
    links: CapturedOutboundLink[];
  };
  return {
    pageUrl,
    pageOrigin: result.pageOrigin,
    links: result.links,
  };
}

export function detectOutboundLinkIssues(
  snap: OutboundLinksSnapshot,
): OutboundLinkFinding[] {
  const tabnabVulnerable: CapturedOutboundLink[] = [];
  const openerExplicit: CapturedOutboundLink[] = [];
  const noNoreferrer: CapturedOutboundLink[] = [];

  for (const l of snap.links) {
    // Only outbound links matter — captured filter already enforced
    // this, but defence-in-depth.
    if (!l.outbound) continue;

    const isBlank = l.target === '_blank';
    const hasNoopener = l.rel.includes('noopener');
    const hasOpenerExplicit = l.rel.includes('opener');
    const hasNoreferrer = l.rel.includes('noreferrer');

    // Explicit rel=opener opts BACK IN to vulnerable behaviour even
    // if target isn't _blank — flag separately and unconditionally.
    if (hasOpenerExplicit) {
      openerExplicit.push(l);
    }

    if (isBlank && !hasNoopener && !hasOpenerExplicit) {
      // _blank without noopener AND not already flagged as opener-
      // explicit. Catches the silent default-vulnerable case.
      tabnabVulnerable.push(l);
    }

    // Noreferrer check applies to ALL outbound (target=_blank or
    // not). Same-tab outbound links also leak Referer.
    if (!hasNoreferrer) {
      noNoreferrer.push(l);
    }
  }

  const out: OutboundLinkFinding[] = [];

  if (tabnabVulnerable.length > 0) {
    const examples = tabnabVulnerable.slice(0, 5).map((l) => `${l.selector} → ${l.href}`);
    out.push({
      severity: 'strict',
      kind: 'link.tabnab-vulnerable',
      detail: `${tabnabVulnerable.length} outbound link(s) with target="_blank" and no rel="noopener". The destination page can navigate this tab to a phishing URL via window.opener (tabnabbing). Modern browsers default to noopener but older / embedded / downgraded clients do not. Add rel="noopener noreferrer". Examples: ${examples.join('; ')}`,
      evidence: { count: tabnabVulnerable.length, examples },
    });
  }

  if (openerExplicit.length > 0) {
    const examples = openerExplicit.slice(0, 5).map((l) => `${l.selector} → ${l.href}`);
    out.push({
      severity: 'strict',
      kind: 'link.opener-explicit',
      detail: `${openerExplicit.length} outbound link(s) explicitly set rel="opener" — this OPTS BACK IN to the tabnabbing vulnerability that browser defaults are designed to prevent. Remove the 'opener' token. Examples: ${examples.join('; ')}`,
      evidence: { count: openerExplicit.length, examples },
    });
  }

  if (noNoreferrer.length > 0) {
    const examples = noNoreferrer.slice(0, 5).map((l) => `${l.selector} → ${l.href}`);
    out.push({
      severity: 'warn',
      kind: 'link.outbound-no-noreferrer',
      detail: `${noNoreferrer.length} outbound link(s) don't set rel="noreferrer". The destination's analytics will see the current URL — leaking session tokens, internal paths, and traffic patterns to a third party. Add rel="noreferrer" unless attribution is intentional. Examples: ${examples.join('; ')}`,
      evidence: { count: noNoreferrer.length, examples },
    });
  }

  return out;
}

export async function checkOutboundLinks(
  page: Page,
): Promise<{
  snapshot: OutboundLinksSnapshot;
  findings: OutboundLinkFinding[];
}> {
  const snapshot = await captureOutboundLinksSnapshot(page);
  const findings = detectOutboundLinkIssues(snapshot);
  return { snapshot, findings };
}
