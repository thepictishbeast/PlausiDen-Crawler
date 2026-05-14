/**
 * htmlLang.ts — `<html lang>` attribute detector. T76.
 *
 * WCAG 3.1.1 Language of Page (Level A): the default human
 * language of the page must be programmatically determinable.
 * Screen readers use `<html lang>` to switch pronunciation,
 * inflection, and dictionary; without it, English screen readers
 * read French content phonetically as English (and vice versa).
 *
 * Findings:
 *
 *   - lang.missing             strict
 *     `<html>` has no `lang` attribute (not even an empty one).
 *
 *   - lang.empty               strict
 *     `<html lang="">` — same effect as missing, but a deliberate
 *     bug indicator (someone removed the value but kept the
 *     attribute).
 *
 *   - lang.invalid             warn
 *     `lang` value doesn't match a recognized BCP-47 / ISO-639
 *     pattern. We don't validate against the full BCP-47 grammar
 *     (overkill for a runtime check); we apply a structural
 *     filter that catches the obvious mistakes.
 *
 *   - lang.unknown-primary     warn
 *     Primary subtag isn't in our list of common ISO 639-1 codes.
 *     This catches typos (`engish`) without flagging genuinely
 *     rare languages.
 *
 * The supersociety doctrine (per CLAUDE.md) is to use ISO 639-1
 * lowercase codes. Loom's docs already enforce this for their
 * own pages; this detector lifts the same standard to every site
 * the crawler audits.
 *
 * Mirror: crates/crawler-detectors/src/html_lang.rs.
 */
import type { Page } from 'playwright';

export interface HtmlLangFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface HtmlLangSnapshot {
  pageUrl: string;
  /** True iff `<html>` has a `lang` attribute (empty or not). */
  present: boolean;
  /** Raw attribute value, '' if absent or explicitly empty. */
  value: string;
}

export async function captureHtmlLangSnapshot(
  page: Page,
): Promise<HtmlLangSnapshot> {
  const pageUrl = page.url();
  const evalFn = `(() => {
    const root = document.documentElement;
    if (!root.hasAttribute('lang')) return { present: false, value: '' };
    return { present: true, value: root.getAttribute('lang') || '' };
  })()`;
  const result = (await page.evaluate(evalFn)) as { present: boolean; value: string };
  return { pageUrl, present: result.present, value: result.value };
}

/**
 * Common ISO 639-1 two-letter codes. Not exhaustive (there are
 * ~180 ISO 639-1 codes total) — we list the long-tail-cutoff
 * codes that cover ~99% of real-world web traffic.
 *
 * BCP-47 also allows ISO 639-2 three-letter codes
 * (e.g. `cmn` for Mandarin Chinese), but they're rare on the
 * web. If a site uses one we'll currently warn — acceptable
 * trade-off for catching typos like `engish`.
 */
const COMMON_ISO_639_1 = new Set<string>([
  'aa', 'ab', 'af', 'am', 'ar', 'as', 'az',
  'ba', 'be', 'bg', 'bh', 'bm', 'bn', 'bo', 'br', 'bs',
  'ca', 'ce', 'co', 'cs', 'cy',
  'da', 'de', 'dv', 'dz',
  'el', 'en', 'eo', 'es', 'et', 'eu',
  'fa', 'fi', 'fj', 'fo', 'fr', 'fy',
  'ga', 'gd', 'gl', 'gn', 'gu', 'gv',
  'ha', 'he', 'hi', 'hr', 'ht', 'hu', 'hy',
  'ia', 'id', 'ie', 'ig', 'is', 'it', 'iu',
  'ja', 'jv',
  'ka', 'kk', 'kl', 'km', 'kn', 'ko', 'ku', 'kw', 'ky',
  'la', 'lb', 'lo', 'lt', 'lv',
  'mg', 'mk', 'ml', 'mn', 'mr', 'ms', 'mt', 'my',
  'na', 'nb', 'ne', 'nl', 'nn', 'no',
  'oc', 'or',
  'pa', 'pl', 'ps', 'pt',
  'qu',
  'rm', 'ro', 'ru', 'rw',
  'sa', 'sd', 'se', 'sg', 'si', 'sk', 'sl', 'sm', 'sn', 'so', 'sq',
  'sr', 'ss', 'st', 'su', 'sv', 'sw',
  'ta', 'te', 'tg', 'th', 'ti', 'tk', 'tl', 'tn', 'to', 'tr', 'ts', 'tt', 'tw',
  'ug', 'uk', 'ur', 'uz',
  'vi',
  'wa', 'wo',
  'xh',
  'yi', 'yo',
  'zh', 'zu',
]);

/**
 * Loose BCP-47 structural validator. Checks the SHAPE, not the
 * registry — we accept anything that looks like a valid tag.
 * Rejects: contains spaces, contains `_` (BCP-47 uses `-`), starts
 * with a digit, contains chars outside [A-Za-z0-9-], has empty
 * subtags from doubled hyphens.
 */
function looksLikeBcp47(value: string): boolean {
  if (!value) return false;
  if (/\s/.test(value)) return false;
  if (value.includes('_')) return false;
  if (/^[0-9]/.test(value)) return false;
  if (!/^[A-Za-z0-9-]+$/.test(value)) return false;
  if (value.startsWith('-') || value.endsWith('-')) return false;
  if (value.includes('--')) return false;
  // Primary subtag must be 2 or 3 letters (ISO 639-1 / 639-2/3).
  const primary = value.split('-')[0];
  if (!/^[A-Za-z]{2,3}$/.test(primary)) return false;
  return true;
}

export function detectHtmlLangIssues(snap: HtmlLangSnapshot): HtmlLangFinding[] {
  const out: HtmlLangFinding[] = [];

  if (!snap.present) {
    out.push({
      severity: 'strict',
      kind: 'lang.missing',
      detail: `<html> element has no 'lang' attribute. WCAG 3.1.1 (Language of Page, A) — screen readers can't pick the right voice and pronounce the page in the user's UA default language. Add e.g. <html lang="en">.`,
      evidence: { present: false },
    });
    return out;
  }

  if (snap.value.trim() === '') {
    out.push({
      severity: 'strict',
      kind: 'lang.empty',
      detail: `<html lang=""> — attribute present but empty. Same effect as missing. Either remove the attribute or set it to a valid BCP-47 language tag (e.g. 'en', 'en-US', 'fr-CA').`,
      evidence: { present: true, value: snap.value },
    });
    return out;
  }

  const value = snap.value.trim();

  if (!looksLikeBcp47(value)) {
    out.push({
      severity: 'warn',
      kind: 'lang.invalid',
      detail: `<html lang="${snap.value}"> doesn't look like a valid BCP-47 tag (no underscores, no whitespace, primary subtag must be 2-3 letters). Examples: 'en', 'en-US', 'pt-BR', 'zh-Hans'.`,
      evidence: { value: snap.value },
    });
    // Don't try to validate the primary subtag if the structure is
    // already broken — would be noise on top of noise.
    return out;
  }

  const primary = value.split('-')[0].toLowerCase();
  if (!COMMON_ISO_639_1.has(primary)) {
    out.push({
      severity: 'warn',
      kind: 'lang.unknown-primary',
      detail: `<html lang="${value}"> primary subtag '${primary}' isn't in the common ISO 639-1 set. This may be a typo (e.g. 'engish' for 'en') or a rare valid language. Verify against the IANA Language Subtag Registry.`,
      evidence: { value, primary },
    });
  }

  return out;
}

export async function checkHtmlLang(
  page: Page,
): Promise<{ snapshot: HtmlLangSnapshot; findings: HtmlLangFinding[] }> {
  const snapshot = await captureHtmlLangSnapshot(page);
  const findings = detectHtmlLangIssues(snapshot);
  return { snapshot, findings };
}
