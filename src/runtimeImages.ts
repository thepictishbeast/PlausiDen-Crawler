/**
 * runtimeImages.ts — runtime image-health detector. T75.
 *
 * Walks every <img> in the rendered DOM and emits findings for:
 *
 *   images.broken              — img.complete=true but naturalWidth=0
 *                                (404, corrupt, MIME mismatch, blocked)
 *   images.empty-src           — src attribute is empty or missing
 *   images.missing-alt-attr    — no alt attribute at all (axe also
 *                                catches this; we re-verify at runtime
 *                                to catch DOM-injected imgs)
 *   images.cls-risk            — visible img with no width/height +
 *                                no aspect-ratio CSS (causes layout
 *                                shift while loading)
 *
 * Runs strictly on what the visitor's browser sees — DOM + computed
 * styles + per-image load state. No file-system reads.
 *
 * Mirrors cssHealth + uiOverflow + runtimeContrast pattern: capture
 * a snapshot via page.evaluate, run pure-function detection on it.
 */
import type { Page } from 'playwright';

export interface RuntimeImageFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface ImageOffender {
  selector: string;
  src: string;
  alt: string | null;
  naturalWidth: number;
  naturalHeight: number;
  complete: boolean;
  width: number;
  height: number;
  hasExplicitDims: boolean;
  hasAspectRatio: boolean;
  isVisible: boolean;
  isDecorative: boolean;
}

export interface RuntimeImagesSnapshot {
  pageUrl: string;
  viewport: { width: number; height: number };
  totalImages: number;
  broken: ImageOffender[];
  emptySrc: ImageOffender[];
  missingAlt: ImageOffender[];
  clsRisk: ImageOffender[];
}

export async function captureRuntimeImagesSnapshot(page: Page): Promise<RuntimeImagesSnapshot> {
  const pageUrl = page.url();

  // String-eval (tsx __name workaround consistent with uiOverflow).
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
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return false;
      return true;
    };

    const broken = [];
    const emptySrc = [];
    const missingAlt = [];
    const clsRisk = [];
    let totalImages = 0;

    const imgs = document.querySelectorAll('img');
    for (let i = 0; i < imgs.length; i++) {
      const el = imgs[i];
      totalImages += 1;
      const visible = isVisible(el);
      const cs = window.getComputedStyle(el);
      const rect = el.getBoundingClientRect();
      const src = el.getAttribute('src') || '';
      const alt = el.getAttribute('alt');
      const widthAttr = el.getAttribute('width');
      const heightAttr = el.getAttribute('height');
      const hasExplicitDims = !!(widthAttr && heightAttr);
      const hasAspectRatio = !!(cs.aspectRatio && cs.aspectRatio !== 'auto');
      // alt='' marks decorative; alt missing entirely is a bug.
      const isDecorative = alt === '';

      const offender = {
        selector: selectorOf(el),
        src: src,
        alt: alt,
        naturalWidth: el.naturalWidth,
        naturalHeight: el.naturalHeight,
        complete: el.complete,
        width: Math.round(rect.width),
        height: Math.round(rect.height),
        hasExplicitDims: hasExplicitDims,
        hasAspectRatio: hasAspectRatio,
        isVisible: visible,
        isDecorative: isDecorative
      };

      // 1. Empty / missing src.
      if (!src || src.trim() === '') {
        emptySrc.push(offender);
        continue;
      }
      // 2. Broken: complete=true but naturalWidth=0.
      if (el.complete && el.naturalWidth === 0) {
        broken.push(offender);
      }
      // 3. Missing alt attribute. Decorative images need alt='', not
      //    a missing attribute.
      if (alt === null) {
        missingAlt.push(offender);
      }
      // 4. CLS risk: visible AND lacks both explicit dims AND
      //    aspect-ratio. Decorative + invisible images don't count.
      if (visible && !hasExplicitDims && !hasAspectRatio) {
        clsRisk.push(offender);
      }
    }

    return {
      vpW: window.innerWidth,
      vpH: window.innerHeight,
      totalImages: totalImages,
      broken: broken,
      emptySrc: emptySrc,
      missingAlt: missingAlt,
      clsRisk: clsRisk
    };
  })()`;

  const result = await page.evaluate(evalFn) as {
    vpW: number;
    vpH: number;
    totalImages: number;
    broken: ImageOffender[];
    emptySrc: ImageOffender[];
    missingAlt: ImageOffender[];
    clsRisk: ImageOffender[];
  };

  return {
    pageUrl,
    viewport: { width: result.vpW, height: result.vpH },
    totalImages: result.totalImages,
    broken: result.broken,
    emptySrc: result.emptySrc,
    missingAlt: result.missingAlt,
    clsRisk: result.clsRisk,
  };
}

export function detectRuntimeImageIssues(snap: RuntimeImagesSnapshot): RuntimeImageFinding[] {
  const out: RuntimeImageFinding[] = [];
  if (snap.broken.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'images.broken',
      detail: `${snap.broken.length} <img> tag(s) failed to load (naturalWidth=0 with complete=true). Visitors see broken-image icons.`,
      evidence: { offenderCount: snap.broken.length, offenders: snap.broken.slice(0, 10) },
    });
  }
  if (snap.emptySrc.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'images.empty-src',
      detail: `${snap.emptySrc.length} <img> tag(s) have empty or missing src attribute.`,
      evidence: { offenderCount: snap.emptySrc.length, offenders: snap.emptySrc.slice(0, 10) },
    });
  }
  if (snap.missingAlt.length > 0) {
    out.push({
      severity: 'strict',
      kind: 'images.missing-alt-attr',
      detail: `${snap.missingAlt.length} <img> tag(s) have no alt attribute. Screen readers announce filename instead. (Decorative images need alt="", not missing attribute.)`,
      evidence: { offenderCount: snap.missingAlt.length, offenders: snap.missingAlt.slice(0, 10) },
    });
  }
  if (snap.clsRisk.length > 0) {
    out.push({
      severity: 'warn',
      kind: 'images.cls-risk',
      detail: `${snap.clsRisk.length} visible <img> tag(s) lack both explicit width+height attributes AND CSS aspect-ratio. Page will shift as images load (CLS).`,
      evidence: { offenderCount: snap.clsRisk.length, offenders: snap.clsRisk.slice(0, 10) },
    });
  }
  return out;
}

export async function checkRuntimeImages(
  page: Page,
): Promise<{ snapshot: RuntimeImagesSnapshot; findings: RuntimeImageFinding[] }> {
  const snapshot = await captureRuntimeImagesSnapshot(page);
  const findings = detectRuntimeImageIssues(snapshot);
  return { snapshot, findings };
}
