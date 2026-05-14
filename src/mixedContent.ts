/**
 * mixedContent.ts — mixed-content (HTTPS page loading HTTP resource)
 * detector. T76.
 *
 * Modern browsers BLOCK mixed-active content (script/css/iframe/
 * worker over http on an https page) and SILENTLY UPGRADE many
 * passive types (img/audio/video). The runtime signal can be
 * inconsistent — Chrome auto-upgrades, Firefox warns, Safari
 * blocks. So static analysis of the HTML attribute markup is
 * the most reliable detector.
 *
 * Even when the browser silently auto-upgrades, the source markup
 * is still wrong: it's a security misconfiguration that could
 * regress on a different client (older browser, embedded view,
 * downgraded fetcher). Per the project's state-actor threat
 * model (CLAUDE.md), defence in depth: surface the bug at the
 * source, not just at the runtime symptom.
 *
 * Findings:
 *
 *   - mixed-content.active      strict
 *     `<script>`, `<link rel=stylesheet>`, `<iframe>`,
 *     `<embed>`, `<object>` with http:// URL on an https page.
 *     These get BLOCKED by browsers; the page may render with
 *     missing functionality and zero warning past the console.
 *
 *   - mixed-content.passive     warn
 *     `<img>`, `<audio>`, `<video>`, `<source>`, `<picture>`
 *     with http:// URL on an https page. May get auto-upgraded
 *     OR rendered with a "not secure" indicator OR blocked
 *     depending on browser. Either way, the markup is wrong.
 *
 *   - mixed-content.form-action strict
 *     `<form action="http://...">` on an https page. The form
 *     submission travels in the clear — credentials / PII
 *     leaked. Browsers may warn or block; the markup is
 *     unambiguously broken.
 *
 * The detector skips http:// pages (mixed-content concept doesn't
 * apply when the page itself isn't secure).
 *
 * Mirror: crates/crawler-detectors/src/mixed_content.rs.
 */
import type { Page } from 'playwright';

export interface MixedContentFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface CapturedMixedAsset {
  selector: string;
  /** lowercased tag name */
  tag: string;
  /** HTTP-scheme URL */
  url: string;
  /** Which attribute carried the URL: src/href/action/data/srcset */
  attribute: string;
  /** Classification: active | passive | form */
  classification: 'active' | 'passive' | 'form';
}

export interface MixedContentSnapshot {
  pageUrl: string;
  /** True iff the page itself was loaded over https. */
  pageIsHttps: boolean;
  assets: CapturedMixedAsset[];
}

export async function captureMixedContentSnapshot(
  page: Page,
): Promise<MixedContentSnapshot> {
  const pageUrl = page.url();
  const pageIsHttps = pageUrl.startsWith('https://');
  if (!pageIsHttps) {
    // Mixed-content doesn't apply to http pages.
    return { pageUrl, pageIsHttps: false, assets: [] };
  }
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

    const isHttp = function(u) {
      if (!u) return false;
      const s = u.trim().toLowerCase();
      return s.startsWith('http://');
    };

    // Each entry: { sel, attr, class }
    const targets = [
      { sel: 'script[src]',                attr: 'src',    cls: 'active'  },
      { sel: 'link[rel="stylesheet"][href]', attr: 'href', cls: 'active'  },
      { sel: 'link[rel="preload"][href]',  attr: 'href',   cls: 'active'  },
      { sel: 'iframe[src]',                attr: 'src',    cls: 'active'  },
      { sel: 'embed[src]',                 attr: 'src',    cls: 'active'  },
      { sel: 'object[data]',               attr: 'data',   cls: 'active'  },
      { sel: 'img[src]',                   attr: 'src',    cls: 'passive' },
      { sel: 'audio[src]',                 attr: 'src',    cls: 'passive' },
      { sel: 'video[src]',                 attr: 'src',    cls: 'passive' },
      { sel: 'source[src]',                attr: 'src',    cls: 'passive' },
      { sel: 'video[poster]',              attr: 'poster', cls: 'passive' },
      { sel: 'form[action]',               attr: 'action', cls: 'form'    },
    ];

    const out = [];
    for (let t = 0; t < targets.length; t++) {
      const cfg = targets[t];
      const els = document.querySelectorAll(cfg.sel);
      for (let i = 0; i < els.length; i++) {
        const el = els[i];
        const url = el.getAttribute(cfg.attr) || '';
        if (!isHttp(url)) continue;
        out.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          url: url.slice(0, 200),
          attribute: cfg.attr,
          classification: cfg.cls,
        });
      }
    }

    // srcset is a comma-separated list of "url density" pairs;
    // walk each url separately so a single img/source srcset with
    // mixed urls flags accurately.
    const srcsetEls = document.querySelectorAll('img[srcset], source[srcset]');
    for (let i = 0; i < srcsetEls.length; i++) {
      const el = srcsetEls[i];
      const raw = el.getAttribute('srcset') || '';
      const parts = raw.split(',').map(function(p) { return p.trim().split(/\\s+/)[0] || ''; });
      for (let p = 0; p < parts.length; p++) {
        if (!isHttp(parts[p])) continue;
        out.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          url: parts[p].slice(0, 200),
          attribute: 'srcset',
          classification: 'passive',
        });
      }
    }

    return { assets: out };
  })()`;

  const result = (await page.evaluate(evalFn)) as { assets: CapturedMixedAsset[] };
  return { pageUrl, pageIsHttps: true, assets: result.assets };
}

export function detectMixedContentIssues(
  snap: MixedContentSnapshot,
): MixedContentFinding[] {
  if (!snap.pageIsHttps) return [];

  const active: CapturedMixedAsset[] = [];
  const passive: CapturedMixedAsset[] = [];
  const form: CapturedMixedAsset[] = [];

  for (const a of snap.assets) {
    if (a.classification === 'active') active.push(a);
    else if (a.classification === 'passive') passive.push(a);
    else if (a.classification === 'form') form.push(a);
  }

  const out: MixedContentFinding[] = [];
  const renderEx = (a: CapturedMixedAsset) =>
    `${a.selector} ${a.tag}[${a.attribute}=${a.url}]`;

  if (active.length > 0) {
    const examples = active.slice(0, 5).map(renderEx);
    out.push({
      severity: 'strict',
      kind: 'mixed-content.active',
      detail: `${active.length} active mixed-content asset(s): script/stylesheet/iframe/embed/object loaded over http on this https page. Browsers BLOCK these — the page renders with missing functionality. Switch to https:// or use protocol-relative URLs ('//host/path'). Examples: ${examples.join('; ')}`,
      evidence: { count: active.length, examples },
    });
  }
  if (passive.length > 0) {
    const examples = passive.slice(0, 5).map(renderEx);
    out.push({
      severity: 'warn',
      kind: 'mixed-content.passive',
      detail: `${passive.length} passive mixed-content asset(s): img/audio/video/srcset over http on this https page. Some browsers auto-upgrade, others render with a 'not secure' indicator, others block. Switch to https://. Examples: ${examples.join('; ')}`,
      evidence: { count: passive.length, examples },
    });
  }
  if (form.length > 0) {
    const examples = form.slice(0, 5).map(renderEx);
    out.push({
      severity: 'strict',
      kind: 'mixed-content.form-action',
      detail: `${form.length} form(s) submit to http:// on this https page. Form contents (credentials, PII, payment data) travel IN THE CLEAR. Switch action="https://...". Examples: ${examples.join('; ')}`,
      evidence: { count: form.length, examples },
    });
  }

  return out;
}

export async function checkMixedContent(
  page: Page,
): Promise<{
  snapshot: MixedContentSnapshot;
  findings: MixedContentFinding[];
}> {
  const snapshot = await captureMixedContentSnapshot(page);
  const findings = detectMixedContentIssues(snapshot);
  return { snapshot, findings };
}
