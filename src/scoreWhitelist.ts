/**
 * scoreWhitelist.ts — accepted-risk filter for the Supersociety
 * Score. T76 cycle 35.
 *
 * Problem: real-world journeys have findings the operator has
 * intentionally accepted. SkillShots' 7 baseline-frozen
 * tap-targets, a legacy CSP that an enterprise contract
 * mandates stays at `unsafe-inline`, a vendor stylesheet that
 * lacks SRI because the vendor refuses to publish a hash.
 * Each of these legitimately deducts from the cycle-32
 * Supersociety Score, but the score then no longer reflects
 * "is there anything NEW to worry about" — it reflects "the
 * pile of accepted risks plus anything new".
 *
 * The whitelist file (per journey, sibling to the journey
 * JSON) lets the operator declare accepted risks. Whitelisted
 * findings are FILTERED OUT of the score calculation but
 * remain in the report.events stream for full transparency.
 * The HTML report surfaces the whitelist in a separate
 * "Accepted risks" section so reviewers can see what's been
 * suppressed.
 *
 * File location: `<journey-base>.whitelist.json` —
 * e.g. `journeys/skillshots-poc.whitelist.json`.
 *
 * File shape (array of entries):
 *
 *     [
 *       {
 *         "kind": "tap-targets",
 *         "ruleId": "tap.too-small",
 *         "reason": "SkillShots PoC layout has 7 deliberate small targets; redesign queued.",
 *         "until": "2026-12-31"
 *       },
 *       {
 *         "kind": "csp-policy",
 *         "ruleId": "csp.script-unsafe-inline",
 *         "reason": "Enterprise contract X requires legacy unsafe-inline; migration in 2027.",
 *         "until": "2027-06-30"
 *       },
 *       {
 *         "kind": "info-leak"
 *         // No ruleId → wildcard, all info-leak findings suppressed.
 *       }
 *     ]
 *
 * Matching semantics:
 *   * `kind` MUST match (case-sensitive, exact).
 *   * `ruleId` if present MUST match (case-sensitive, exact).
 *   * `ruleId` if absent matches any ruleId of that kind.
 *   * `until` if present must be in the future (ISO 8601
 *     date or datetime); expired entries DON'T match — score
 *     deduction resumes. The HTML report shows expired entries
 *     in a separate "Expired whitelist" sub-section so the
 *     operator knows to renew.
 *
 * Out of scope:
 *   * URL-pattern matching (suppress on /admin/* but not /).
 *     Could be a future extension.
 *   * Justification linking — the `reason` is free-text; we
 *     could later require a ticket URL but the operator can
 *     write whatever they want now.
 *   * Hash-of-finding granularity. The kind+ruleId combo is
 *     coarse — if a CSP detector emits the SAME ruleId for
 *     two different pages, the whitelist suppresses both.
 *     For SkillShots' use case this is fine; revisit if it
 *     causes problems.
 *
 * REGRESSION-GUARD: the whitelist FILTERS events from the
 * score, not from the report.events stream. Operators
 * inspecting the JSON dump still see every finding. The HTML
 * report's "Accepted risks" section is the audit trail.
 */

import { existsSync, readFileSync } from 'fs';
import { dirname, basename, join } from 'path';

export interface WhitelistEntry {
  /** Captured-event kind. Required. */
  kind: string;
  /** Captured-event ruleId. Optional — absent matches any. */
  ruleId?: string;
  /** Free-text accepted-risk justification. Optional but
   *  strongly recommended — surfaces in the HTML report. */
  reason?: string;
  /** ISO 8601 date or datetime. If present and in the past,
   *  this entry is EXPIRED and doesn't match. */
  until?: string;
}

export interface WhitelistedFinding {
  kind: string;
  ruleId?: string;
  url?: string;
  text?: string;
  matchedEntry: WhitelistEntry;
}

export interface WhitelistApplyResult {
  /** Events kept (not whitelisted, scored as normal). */
  kept: Array<{ kind: string; ruleId?: string; severity?: string; level?: string; url?: string; text?: string }>;
  /** Events suppressed from the score (audit trail). */
  whitelisted: WhitelistedFinding[];
  /** Whitelist entries that didn't match anything (operator
   *  may have stale entries to remove). */
  unused: WhitelistEntry[];
  /** Whitelist entries past their `until` date. */
  expired: WhitelistEntry[];
}

/**
 * Resolve the whitelist path for a journey. The journey arg
 * can be either the bare name (`skillshots-poc`) or the full
 * journey JSON path (`journeys/skillshots-poc.json`). Both
 * resolve to the same `*.whitelist.json` sibling.
 */
export function whitelistPathFor(journey: string): string {
  // Strip .json suffix if present.
  const withoutSuffix = journey.replace(/\.json$/i, '');
  // If it looks like a path, keep the dirname. Otherwise put
  // it in journeys/.
  if (withoutSuffix.includes('/') || withoutSuffix.includes('\\')) {
    return `${withoutSuffix}.whitelist.json`;
  }
  return join('journeys', `${withoutSuffix}.whitelist.json`);
}

/**
 * Read + parse the whitelist file. Tolerates missing file
 * (returns empty array) and malformed JSON (logs warning
 * via the optional logFn, returns empty array).
 */
export function readWhitelist(
  journey: string,
  logFn?: (msg: string) => void,
): WhitelistEntry[] {
  const path = whitelistPathFor(journey);
  if (!existsSync(path)) return [];
  let raw: string;
  try {
    raw = readFileSync(path, 'utf8');
  } catch (e) {
    logFn?.(`[whitelist] couldn't read ${path}: ${(e as Error).message}`);
    return [];
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (e) {
    logFn?.(`[whitelist] couldn't parse ${path}: ${(e as Error).message}`);
    return [];
  }
  if (!Array.isArray(parsed)) {
    logFn?.(`[whitelist] ${path} is not a JSON array; ignoring`);
    return [];
  }
  const out: WhitelistEntry[] = [];
  for (let i = 0; i < parsed.length; i++) {
    const entry = parsed[i] as Record<string, unknown> | null;
    if (!entry || typeof entry !== 'object') {
      logFn?.(`[whitelist] ${path}[${i}] is not an object; skipped`);
      continue;
    }
    const kind = entry.kind;
    if (typeof kind !== 'string' || kind.length === 0) {
      logFn?.(`[whitelist] ${path}[${i}] missing required 'kind'; skipped`);
      continue;
    }
    const ruleId = typeof entry.ruleId === 'string' ? entry.ruleId : undefined;
    const reason = typeof entry.reason === 'string' ? entry.reason : undefined;
    const until = typeof entry.until === 'string' ? entry.until : undefined;
    out.push({ kind, ruleId, reason, until });
  }
  return out;
}

function isExpired(entry: WhitelistEntry, now: Date): boolean {
  if (entry.until === undefined) return false;
  const t = Date.parse(entry.until);
  if (Number.isNaN(t)) return false;
  return t < now.getTime();
}

function entryMatches(
  event: { kind: string; ruleId?: string },
  entry: WhitelistEntry,
): boolean {
  if (event.kind !== entry.kind) return false;
  if (entry.ruleId === undefined) return true;
  return event.ruleId === entry.ruleId;
}

export interface ApplyEvent {
  kind: string;
  ruleId?: string;
  severity?: string;
  level?: string;
  url?: string;
  text?: string;
}

/**
 * Filter `events` through the whitelist. Returns the kept +
 * suppressed lists, the unused entries (operator should
 * probably remove them), and the expired entries (operator
 * should renew or remove).
 *
 * `now` is injectable for deterministic tests.
 */
export function applyWhitelist(
  events: ApplyEvent[],
  whitelist: WhitelistEntry[],
  now: Date = new Date(),
): WhitelistApplyResult {
  const active: WhitelistEntry[] = [];
  const expired: WhitelistEntry[] = [];
  for (const e of whitelist) {
    if (isExpired(e, now)) expired.push(e);
    else active.push(e);
  }
  const matchCount = new Map<WhitelistEntry, number>();
  for (const e of active) matchCount.set(e, 0);

  const kept: ApplyEvent[] = [];
  const whitelisted: WhitelistedFinding[] = [];

  for (const ev of events) {
    let matchedEntry: WhitelistEntry | undefined;
    for (const entry of active) {
      if (entryMatches(ev, entry)) {
        matchedEntry = entry;
        break;
      }
    }
    if (matchedEntry) {
      matchCount.set(matchedEntry, (matchCount.get(matchedEntry) ?? 0) + 1);
      whitelisted.push({
        kind: ev.kind,
        ruleId: ev.ruleId,
        url: ev.url,
        text: ev.text,
        matchedEntry,
      });
    } else {
      kept.push(ev);
    }
  }

  const unused: WhitelistEntry[] = [];
  for (const [entry, count] of matchCount.entries()) {
    if (count === 0) unused.push(entry);
  }

  return { kept, whitelisted, unused, expired };
}

/**
 * Render a terminal-friendly summary block. Suitable for the
 * console summary after the Supersociety Score render. Plain
 * ASCII, no colour codes.
 */
export function renderWhitelistSummary(r: WhitelistApplyResult): string {
  const lines: string[] = [];
  lines.push('');
  lines.push('=== Whitelist (accepted risks) ===');
  if (r.whitelisted.length === 0 && r.expired.length === 0 && r.unused.length === 0) {
    lines.push('  No whitelist file or no entries — score unmodified.');
    return lines.join('\n');
  }
  lines.push(`  suppressed: ${r.whitelisted.length} finding(s) excluded from score.`);
  if (r.expired.length > 0) {
    lines.push(`  EXPIRED:    ${r.expired.length} entry/entries past their 'until' date — renew or remove:`);
    for (const e of r.expired) {
      lines.push(`              · kind=${e.kind} ruleId=${e.ruleId ?? '*'} until=${e.until ?? '?'}`);
    }
  }
  if (r.unused.length > 0) {
    lines.push(`  unused:     ${r.unused.length} active entry/entries didn't match any finding — remove if no longer needed:`);
    for (const e of r.unused) {
      lines.push(`              · kind=${e.kind} ruleId=${e.ruleId ?? '*'}`);
    }
  }
  return lines.join('\n');
}
