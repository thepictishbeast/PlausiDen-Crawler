/**
 * supersocietyScore.ts — meta-aggregator that turns the
 * crawler's 43+ detection axes into a single composite
 * 0-100 score with per-category breakdown + letter grade.
 * T76 cycle 32.
 *
 * Why this exists:
 *
 * After 31 cycles of T76 expansion, the crawler emits 43
 * named detector axes plus the three legacy event kinds. An
 * operator looking at the JSON report has to mentally
 * aggregate findings across 14 security categories, 12
 * accessibility categories, etc. This is a UX gap — the
 * crawler is too detailed to act on without pre-processing.
 *
 * The supersocietyScore aggregator solves this by:
 *
 *   1. Bucketing every finding kind into ONE of 11 categories.
 *   2. Computing a 0-100 score per category (deductive: 100
 *      with no findings, -25 per strict finding, -5 per warn,
 *      clamped at 0).
 *   3. Weighting categories by importance (security > UX).
 *   4. Producing a composite 0-100 score + letter grade.
 *
 * Output shape: small JSON object that ships alongside
 * report.json. Console summary prints the composite + grade.
 *
 * Design choices:
 *
 * Deduction policy. Strict findings cost 25 points each, warns
 * cost 5. A single category with one strict + zero warns =
 * score 75, grade C. Two strict = 50, grade F. Three warns =
 * 85, grade B. This is intentionally aggressive — supersociety
 * means "no excuses". An operator complaining about a low
 * score should be told: fix the findings.
 *
 * Categorisation. Most kinds are one-to-one with categories.
 * Some kinds touch multiple concerns:
 *   * cookieSecurity → cookieHygiene (primary).
 *   * cacheControl → cacheCorrectness (primary), but the
 *     cache-control.public-with-cookie strict overlaps cookie
 *     hygiene. We attribute to cacheCorrectness only — cookie
 *     hygiene is covered by cookieSecurity from a different
 *     angle.
 *
 * Weighting. The composite is a weighted average. Security
 * categories carry 2x weight vs UX categories — a site with
 * great UX but no CSP shouldn't grade out the same as a site
 * with stringent CSP and one missing alt text.
 *
 * Unknown event kinds (future detectors not yet bucketed)
 * fall into a `misc` category that's logged but doesn't
 * affect the composite — keeps the score stable when new
 * axes ship before the bucketing is updated.
 */

export interface SupersocietyCategoryScore {
  /** Category identifier — e.g. 'transportSecurity'. */
  category: string;
  /** Weight applied to this category in the composite. */
  weight: number;
  /** 0-100 score for this category. */
  score: number;
  /** Letter grade A..F. */
  grade: string;
  /** Number of strict findings in this category. */
  strict: number;
  /** Number of warn findings in this category. */
  warn: number;
  /** Detector kinds that fed this category (for the audit trail). */
  contributingKinds: string[];
}

export interface SupersocietyScore {
  /** Composite 0-100 score, weighted average of categories. */
  composite: number;
  /** Composite letter grade A..F. */
  grade: string;
  /** Per-category breakdown. */
  categories: SupersocietyCategoryScore[];
  /** Total strict findings across all categories. */
  totalStrict: number;
  /** Total warn findings across all categories. */
  totalWarn: number;
  /** Event kinds the categoriser didn't recognise. */
  unbucketed: string[];
  /** Plain-English headline interpretation. */
  headline: string;
}

/**
 * Categorisation map. Keys are the captured event `kind`
 * field from main.ts (matches the report.events[].kind
 * literal-union types in report.ts). Values are category
 * identifiers.
 */
const KIND_TO_CATEGORY: Record<string, string> = {
  // Transport security — TLS / mixed content / HSTS.
  'hsts': 'transportSecurity',
  'mixed-content': 'transportSecurity',

  // Origin isolation — Spectre mitigation, embed control.
  'coop': 'originIsolation',
  'coep': 'originIsolation',
  'corp': 'originIsolation',
  'x-frame-options': 'originIsolation',
  'permissions-policy': 'originIsolation',

  // Content security — CSP, SRI, DOM XSS surface.
  'csp-policy': 'contentSecurity',
  'sri': 'contentSecurity',
  'inline-script': 'contentSecurity',

  // Cookie hygiene.
  'cookie-security': 'cookieHygiene',

  // Cache correctness.
  'cache-control': 'cacheCorrectness',
  'vary': 'cacheCorrectness',

  // Information disclosure.
  'info-leak': 'infoDisclosure',
  'referrer-policy': 'infoDisclosure',

  // Observability — reporting + telemetry.
  'reporting-endpoints': 'observability',

  // Accessibility — a11y axe-core + WCAG-flavoured checks.
  'a11y-violation': 'accessibility',
  'heading-order': 'accessibility',
  'runtime-landmarks': 'accessibility',
  'runtime-contrast': 'accessibility',
  'runtime-focus': 'accessibility',
  'link-text': 'accessibility',
  'placeholder-text': 'accessibility',
  'tap-targets': 'accessibility',
  'form-labels': 'accessibility',
  'doc-title': 'accessibility',
  'html-lang': 'accessibility',
  'skip-link': 'accessibility',
  'autocomplete': 'accessibility',
  'aria-drift': 'accessibility',
  'link-underline': 'accessibility',
  'runtime-images': 'accessibility',

  // UX hygiene — non-a11y user-facing polish.
  'viewport-meta': 'uxHygiene',
  'meta-description': 'uxHygiene',
  'favicon': 'uxHygiene',
  'outbound-links': 'uxHygiene',
  'cross-page-title': 'uxHygiene',
  'cross-page-meta-description': 'uxHygiene',
  'font-loading': 'uxHygiene',
  'web-vitals': 'uxHygiene',
  'ui-overflow': 'uxHygiene',
  'css-health': 'uxHygiene',

  // Reliability — JS errors, network failures.
  'console': 'reliability',
  'pageerror': 'reliability',
  'request-failed': 'reliability',
  'response-error': 'reliability',
  'csp-violation': 'reliability',
};

/**
 * Category weights. Security categories (transport, origin,
 * content, cookie) carry 2x weight vs UX. Reliability +
 * accessibility carry 1.5x — they're security-adjacent and
 * legally-mandated respectively.
 */
const CATEGORY_WEIGHTS: Record<string, number> = {
  transportSecurity: 2.0,
  originIsolation: 2.0,
  contentSecurity: 2.0,
  cookieHygiene: 2.0,
  cacheCorrectness: 1.5,
  infoDisclosure: 1.0,
  observability: 1.0,
  reliability: 1.5,
  accessibility: 1.5,
  uxHygiene: 1.0,
};

const STRICT_PENALTY = 25;
const WARN_PENALTY = 5;

function gradeFromScore(score: number): string {
  if (score >= 90) return 'A';
  if (score >= 80) return 'B';
  if (score >= 70) return 'C';
  if (score >= 60) return 'D';
  return 'F';
}

function headlineFor(score: number, totalStrict: number, totalWarn: number): string {
  const grade = gradeFromScore(score);
  if (totalStrict === 0 && totalWarn === 0) {
    return `Grade ${grade} (${score}/100). Supersociety baseline met — zero findings across all categories.`;
  }
  if (grade === 'F') {
    return `Grade F (${score}/100). ${totalStrict} strict + ${totalWarn} warn finding(s). The site has serious defects across multiple categories.`;
  }
  if (grade === 'A') {
    return `Grade A (${score}/100). ${totalStrict} strict + ${totalWarn} warn finding(s). Near-supersociety; close out the warns.`;
  }
  return `Grade ${grade} (${score}/100). ${totalStrict} strict + ${totalWarn} warn finding(s). Clear path to A: fix the strict findings first.`;
}

/**
 * Captured event shape — a minimal subset of the report.ts
 * CapturedEvent type. Kept loose so this module compiles
 * standalone without importing the full report type
 * (avoids circular import risk).
 */
export interface ScoreInputEvent {
  kind: string;
  severity?: string;
  level?: string;
}

export function calculateSupersocietyScore(events: ScoreInputEvent[]): SupersocietyScore {
  // Tally per-category strict + warn counts.
  const tallies = new Map<string, { strict: number; warn: number; kinds: Set<string> }>();
  const unbucketed = new Set<string>();

  const seenKinds = new Set<string>();
  for (const e of events) {
    seenKinds.add(e.kind);
    // Map console-error to reliability (severity-aware).
    let category: string | undefined;
    if (e.kind === 'console' && e.level === 'error') {
      category = 'reliability';
    } else {
      category = KIND_TO_CATEGORY[e.kind];
    }
    if (category === undefined) {
      // Skip unknown kinds (future detectors not yet bucketed).
      // Log them so the categoriser can be updated.
      unbucketed.add(e.kind);
      continue;
    }
    let bucket = tallies.get(category);
    if (!bucket) {
      bucket = { strict: 0, warn: 0, kinds: new Set() };
      tallies.set(category, bucket);
    }
    bucket.kinds.add(e.kind);
    // strict / warn from severity, with console-warn → warn,
    // console-error → strict. Other kinds use severity field
    // directly; default to warn if absent (consistent with
    // historical detector behaviour).
    const isStrict = e.severity === 'strict' || (e.kind === 'console' && e.level === 'error');
    if (isStrict) bucket.strict += 1;
    else bucket.warn += 1;
  }

  // Build per-category scores. Initialise every weighted
  // category so missing-from-the-stream doesn't penalise.
  const categories: SupersocietyCategoryScore[] = [];
  for (const [cat, weight] of Object.entries(CATEGORY_WEIGHTS)) {
    const t = tallies.get(cat);
    const strict = t?.strict ?? 0;
    const warn = t?.warn ?? 0;
    const rawScore = 100 - (strict * STRICT_PENALTY) - (warn * WARN_PENALTY);
    const score = Math.max(0, Math.min(100, rawScore));
    categories.push({
      category: cat,
      weight,
      score,
      grade: gradeFromScore(score),
      strict,
      warn,
      contributingKinds: t ? [...t.kinds].sort() : [],
    });
  }

  // Composite weighted average.
  let weightedSum = 0;
  let weightTotal = 0;
  for (const c of categories) {
    weightedSum += c.score * c.weight;
    weightTotal += c.weight;
  }
  const composite = weightTotal > 0 ? Math.round(weightedSum / weightTotal) : 100;
  const grade = gradeFromScore(composite);

  let totalStrict = 0;
  let totalWarn = 0;
  for (const c of categories) {
    totalStrict += c.strict;
    totalWarn += c.warn;
  }

  return {
    composite,
    grade,
    categories,
    totalStrict,
    totalWarn,
    unbucketed: [...unbucketed].sort(),
    headline: headlineFor(composite, totalStrict, totalWarn),
  };
}

/**
 * Render the score in a human-readable terminal format. Used
 * in main.ts's console summary. Plain ASCII, no colour codes
 * (CI logs strip them; the user reads JSON for detail).
 */
export function renderSupersocietyScore(s: SupersocietyScore): string {
  const lines: string[] = [];
  lines.push('');
  lines.push('=== Supersociety Score ===');
  lines.push(`  ${s.headline}`);
  lines.push('');
  lines.push('  category              score  grade  strict  warn  weight');
  lines.push('  --------------------  -----  -----  ------  ----  ------');
  for (const c of s.categories) {
    const cat = c.category.padEnd(20);
    const sc = String(c.score).padStart(5);
    const gr = c.grade.padEnd(5);
    const st = String(c.strict).padStart(6);
    const wn = String(c.warn).padStart(4);
    const wt = c.weight.toFixed(1).padStart(6);
    lines.push(`  ${cat}  ${sc}  ${gr}  ${st}  ${wn}  ${wt}`);
  }
  if (s.unbucketed.length > 0) {
    lines.push('');
    lines.push(`  unbucketed event kinds (categoriser update needed): ${s.unbucketed.join(', ')}`);
  }
  return lines.join('\n');
}
