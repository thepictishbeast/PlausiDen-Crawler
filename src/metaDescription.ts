/**
 * metaDescription.ts — page `<meta name="description">` detector. T76.
 *
 * The meta-description is what search engines show under the page
 * title in results, what social-share previews fall back to when
 * no `og:description` is present, and what some assistive tech
 * announces as page summary. It's high-leverage for SEO/UX yet
 * commonly forgotten on internal pages or auto-generated routes.
 *
 * Findings:
 *
 *   - meta-description.missing       warn
 *     No `<meta name="description">` element in head. Search
 *     engines synthesize one from page text — usually poorly.
 *
 *   - meta-description.empty         warn
 *     Tag present but `content=""` or whitespace-only.
 *
 *   - meta-description.too-short     warn
 *     Trimmed content < 50 chars. Real previews truncate at
 *     ~155-160 chars; under 50 means there's no useful preview
 *     to show.
 *
 *   - meta-description.too-long      warn
 *     Trimmed content > 160 chars. Google + most engines truncate
 *     at this length, so the tail of the description never gets
 *     shown.
 *
 * All warn (not strict) — a missing meta description doesn't break
 * the page; it just suboptimizes discovery + previews.
 *
 * Mirror: crates/crawler-detectors/src/meta_description.rs.
 */
import type { Page } from 'playwright';

export interface MetaDescriptionFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface MetaDescriptionSnapshot {
  pageUrl: string;
  /** True iff at least one `<meta name="description">` exists in head. */
  present: boolean;
  /** Raw content attribute of the first such meta, untrimmed. */
  raw: string;
}

const TOO_SHORT_MAX = 50;
const TOO_LONG_MIN = 160;

export async function captureMetaDescriptionSnapshot(
  page: Page,
): Promise<MetaDescriptionSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const m = document.querySelector('head meta[name="description"]');
    if (!m) return { present: false, raw: '' };
    return { present: true, raw: m.getAttribute('content') || '' };
  })()`;
  const result = (await page.evaluate(evalFn)) as { present: boolean; raw: string };
  return { pageUrl, present: result.present, raw: result.raw };
}

export function detectMetaDescriptionIssues(
  snap: MetaDescriptionSnapshot,
): MetaDescriptionFinding[] {
  const out: MetaDescriptionFinding[] = [];

  if (!snap.present) {
    out.push({
      severity: 'warn',
      kind: 'meta-description.missing',
      detail: `Page has no <meta name="description"> in head. Search engines synthesize one from page text (usually poorly); social-share previews lose a useful summary. Add a 50-160 char description that reflects the page's actual content.`,
      evidence: { present: false },
    });
    return out;
  }

  const trimmed = snap.raw.trim();
  if (trimmed === '') {
    out.push({
      severity: 'warn',
      kind: 'meta-description.empty',
      detail: `<meta name="description"> is present but content is empty or whitespace-only. Same effect as missing.`,
      evidence: { rawLength: snap.raw.length },
    });
    return out;
  }

  const length = trimmed.length;
  if (length < TOO_SHORT_MAX) {
    out.push({
      severity: 'warn',
      kind: 'meta-description.too-short',
      detail: `Meta description is only ${length} characters ('${trimmed}'). Search-result snippets and social-share previews need ~50-160 chars of useful summary.`,
      evidence: { description: trimmed, length },
    });
  }
  if (length > TOO_LONG_MIN) {
    out.push({
      severity: 'warn',
      kind: 'meta-description.too-long',
      detail: `Meta description is ${length} characters — Google + most engines truncate around 155-160 chars in results, so the tail is invisible. Trim to <= 160.`,
      evidence: { descriptionHead: trimmed.slice(0, 100) + '…', length },
    });
  }

  return out;
}

export async function checkMetaDescription(
  page: Page,
): Promise<{ snapshot: MetaDescriptionSnapshot; findings: MetaDescriptionFinding[] }> {
  const snapshot = await captureMetaDescriptionSnapshot(page);
  const findings = detectMetaDescriptionIssues(snapshot);
  return { snapshot, findings };
}
