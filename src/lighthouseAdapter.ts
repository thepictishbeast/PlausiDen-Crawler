/**
 * lighthouseAdapter.ts — a Lighthouse result (LHR) becomes inbox records.
 *
 * This is the only place the LHR is allowed to be shaped. The runner
 * hands it whatever Lighthouse produced; the adapter decides what
 * analytics.plausiden.com is ever told about, and everything not on the
 * allowlist is dropped HERE rather than filtered later. A filter applied
 * downstream is a filter somebody eventually turns off.
 *
 * ## The wire shape is deliberately the inbox's own
 *
 * `v` / `kind` / `id` / `ts` / `source` / `data`, exactly as
 * `PlausiDen-Analytics/src/inbox.rs` already parses, so the two parser
 * families cannot drift into two different envelope conventions. The
 * `campaign` field is dropped — it means nothing here — and `data`
 * carries the audit body.
 *
 * ## What must never look like success
 *
 * On this estate a check that has never failed is assumed broken, so
 * every way this can go wrong has a NAMED state rather than an empty
 * file:
 *
 *   failed    Lighthouse threw, Chrome would not start, the target
 *             timed out, or the LHR carries a runtimeError.
 *   unusable  It ran, but the result cannot be trusted: an allowlisted
 *             audit id is missing (a version bump renamed it), or every
 *             audit came back notApplicable/null. Fourteen null scores
 *             are otherwise a fully-accepted, fully-green lie.
 *   findings  Ran, and at least one audit scored short of a pass.
 *   clean     Ran, and every scored audit passed.
 *
 * `clean` is the only one that means what it says, and it is the hardest
 * to reach on purpose.
 *
 * ## The URL is checked, not assumed
 *
 * Lighthouse follows redirects. Asking for `https://x/` and being handed
 * a login page, an error page or an apex-to-www redirect produces a
 * perfectly valid LHR describing a page nobody asked about. Both
 * `requested_url` and `final_url` are recorded on every run, and a
 * mismatch is called out in `redirected` so a trend is never built on
 * two different pages that share a row.
 */
import { ALLOWED_AUDITS, ALLOWED_ID_SET, ALLOWED_IDS } from './lighthouseAllowlist.js';

/** The four states, borrowed verbatim from analytics `scans::State`. */
export type RunState = 'clean' | 'findings' | 'failed' | 'unusable';

export interface InboxRecord {
  v: 1;
  kind: 'ux_run' | 'ux_finding';
  id: string;
  ts: number;
  source: string;
  data: Record<string, unknown>;
}

export interface AdapterInput {
  /** The URL we asked for, before any redirect. */
  requestedUrl: string;
  formFactor: 'mobile' | 'desktop';
  /** Whatever Lighthouse returned. `null` when it threw. */
  lhr: any | null;
  /** The thrown error, when it threw. */
  error?: string;
  durationMs: number;
  /** Epoch seconds. Passed in so a test is not a clock race. */
  nowSec: number;
  /** Throttling actually applied, recorded so runs are comparable. */
  throttling: Record<string, unknown>;
  /** Screen emulation actually applied. */
  emulation: Record<string, unknown>;
}

export interface AdapterOutput {
  records: InboxRecord[];
  state: RunState;
  runId: string;
  /** Human line for the journal, so a failed run says why in one place. */
  summary: string;
}

const hostOf = (url: string): string => {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
};

/** Lighthouse hands savings back in several shapes across audit types. */
const savings = (audit: any): { bytes: number; ms: number } => {
  const d = audit?.details ?? {};
  const ms = Number(d.overallSavingsMs ?? audit?.metricSavings?.LCP ?? 0);
  const bytes = Number(d.overallSavingsBytes ?? 0);
  return {
    bytes: Number.isFinite(bytes) ? Math.round(bytes) : 0,
    ms: Number.isFinite(ms) ? Math.round(ms) : 0,
  };
};

const itemCount = (audit: any): number =>
  Array.isArray(audit?.details?.items) ? audit.details.items.length : 0;

/**
 * Build the records for one target.
 *
 * Always returns at least the `ux_run` record. A run that audited
 * nothing still leaves a row saying so — an absent file and a clean file
 * are indistinguishable from the console, and only one of them is good
 * news.
 */
export function adapt(input: AdapterInput): AdapterOutput {
  const { requestedUrl, formFactor, lhr, nowSec, durationMs } = input;
  const host = hostOf(requestedUrl);
  const iso = new Date(nowSec * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');
  const runId = `${host || 'unknown'}|${formFactor}|${iso}`;
  const source = `lighthouse-${lhr?.lighthouseVersion ?? 'unknown'}`;

  const runRecord = (
    state: RunState,
    extra: Record<string, unknown>,
  ): InboxRecord => ({
    v: 1,
    kind: 'ux_run',
    id: runId,
    ts: nowSec,
    source,
    data: {
      url: requestedUrl,
      host,
      form_factor: formFactor,
      state,
      audits_expected: ALLOWED_IDS.length,
      audits_emitted: 0,
      audits_scored: 0,
      lh_version: String(lhr?.lighthouseVersion ?? ''),
      chrome: String(lhr?.environment?.hostUserAgent ?? ''),
      throttling: input.throttling,
      emulation: input.emulation,
      requested_url: requestedUrl,
      final_url: '',
      redirected: false,
      duration_ms: Math.round(durationMs),
      error: '',
      ...extra,
    },
  });

  // --- it never ran ----------------------------------------------------
  if (!lhr) {
    const err = input.error || 'Lighthouse returned no result and no error';
    return {
      records: [runRecord('failed', { error: err })],
      state: 'failed',
      runId,
      summary: `FAILED ${requestedUrl} (${formFactor}): ${err}`,
    };
  }

  // --- it ran and told us it had failed --------------------------------
  if (lhr.runtimeError && lhr.runtimeError.code !== 'NO_ERROR') {
    const err = `${lhr.runtimeError.code}: ${lhr.runtimeError.message ?? ''}`;
    return {
      records: [runRecord('failed', { error: err, final_url: String(lhr.finalDisplayedUrl ?? '') })],
      state: 'failed',
      runId,
      summary: `FAILED ${requestedUrl} (${formFactor}): ${err}`,
    };
  }

  // Lighthouse 13 exposes both. `finalUrl` is the legacy alias and is
  // read only as a fallback, because relying on a remembered field name
  // is how an adapter ends up recording an empty string forever.
  const finalUrl = String(lhr.finalDisplayedUrl ?? lhr.finalUrl ?? '');
  const redirected = finalUrl !== '' && finalUrl !== requestedUrl;

  // --- an allowlisted id Lighthouse no longer emits ---------------------
  // This is the version-drift guard. Silently emitting 11 records where
  // 14 were expected is exactly how coverage shrinks without anyone
  // noticing, so it is a refusal, not a smaller number.
  const audits = (lhr.audits ?? {}) as Record<string, any>;
  const missing = ALLOWED_IDS.filter((id) => !(id in audits));

  const emitted: InboxRecord[] = [];
  let scored = 0;
  let failing = 0;

  for (const spec of ALLOWED_AUDITS) {
    const a = audits[spec.id];
    if (!a) continue;
    const score = a.score === null || a.score === undefined ? null : Number(a.score);
    // NULL IS A REAL VALUE. notApplicable and informative audits carry
    // it, and coercing it to 0 fabricates a failure while coercing it to
    // 1 fabricates a pass.
    if (score !== null) {
      scored += 1;
      if (score < 1) failing += 1;
    }
    const s = savings(a);
    emitted.push({
      v: 1,
      kind: 'ux_finding',
      id: `${runId}#${spec.id}`,
      ts: nowSec,
      source,
      data: {
        run_id: runId,
        host,
        form_factor: formFactor,
        audit: spec.id,
        title: String(a.title ?? spec.what),
        score,
        score_display_mode: String(a.scoreDisplayMode ?? ''),
        numeric_value:
          a.numericValue === undefined || a.numericValue === null
            ? null
            : Number(a.numericValue),
        numeric_unit: String(a.numericUnit ?? ''),
        display_value: String(a.displayValue ?? a.errorMessage ?? ''),
        items: itemCount(a),
        savings_bytes: s.bytes,
        savings_ms: s.ms,
      },
    });
  }

  let state: RunState;
  let error = '';
  if (missing.length > 0) {
    state = 'unusable';
    error =
      `Lighthouse ${lhr.lighthouseVersion} did not emit ${missing.length} ` +
      `allowlisted audit(s): ${missing.join(', ')} — the allowlist and this ` +
      `Lighthouse version disagree, so the run is not comparable`;
  } else if (scored === 0) {
    // Every audit notApplicable: a redirect to a near-empty page, an
    // error page, a challenge. Fourteen null scores are not a pass.
    state = 'unusable';
    error =
      'every allowlisted audit came back unscored — the page Lighthouse ' +
      'measured had nothing to measure';
  } else if (failing > 0) {
    state = 'findings';
  } else {
    state = 'clean';
  }

  const run = runRecord(state, {
    audits_emitted: emitted.length,
    audits_scored: scored,
    final_url: finalUrl,
    redirected,
    error,
  });

  // The run record MUST be first: analytics enforces
  // `ux_finding.run_id REFERENCES ux_run(run_id)` with
  // `PRAGMA foreign_keys=ON`, so the parent row has to exist before any
  // child insert.
  return {
    records: [run, ...emitted],
    state,
    runId,
    summary:
      `${state.toUpperCase()} ${requestedUrl} (${formFactor}) — ` +
      `${emitted.length}/${ALLOWED_IDS.length} audits, ${scored} scored, ` +
      `${failing} short of a pass` +
      (redirected ? ` — REDIRECTED to ${finalUrl}` : '') +
      (error ? ` — ${error}` : ''),
  };
}

/** Exported for the test: nothing outside the allowlist may escape. */
export const isAllowed = (id: string): boolean => ALLOWED_ID_SET.has(id);
