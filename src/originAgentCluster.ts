/**
 * originAgentCluster.ts — Origin-Agent-Cluster response-header
 * audit. T76 cycle 45.
 *
 * Origin-Agent-Cluster is a modern HTML Living Standard header
 * (W3C, 2021) that lets an origin request its own dedicated
 * agent cluster — a process-level isolation primitive distinct
 * from COOP/COEP cross-origin isolation. With the header set
 * to `?1`, the browser:
 *
 *   * Puts the origin in its OWN agent cluster (separate from
 *     other same-site origins, not just cross-site ones).
 *   * Disables document.domain mutation (which was a vector
 *     for relaxing same-origin policy).
 *   * Improves memory + performance isolation; some Spectre-
 *     class attacks become harder because the attacker's
 *     same-origin page lives in a different process.
 *   * Enables window.crossOriginIsolated to return true even
 *     for same-site embeds, as part of the broader COOP/COEP
 *     story.
 *
 * Without the header, sub-origins on the same site share an
 * agent cluster by default — `https://accounts.example.com`
 * and `https://files.example.com` end up in the same process,
 * so a Spectre exploit against one can leak data from the
 * other.
 *
 * Acceptable values (HTML Living Standard, structured-fields
 * boolean per RFC 8941):
 *   * `?1` — request own agent cluster.
 *   * `?0` — explicit opt-out (default behaviour, same as
 *     header absent).
 *
 * Findings:
 *
 *   - origin-agent-cluster.missing      warn
 *     No header. Browser uses default (shared agent cluster
 *     with same-site origins). Lower-priority than COOP/COEP
 *     since it's a defense-in-depth measure, but the
 *     supersociety baseline sets it for any origin handling
 *     sensitive data.
 *
 *   - origin-agent-cluster.disabled     warn
 *     Header set to `?0` — explicit opt-out. Operator may
 *     have a reason (legacy document.domain code) but it
 *     should be confirmed.
 *
 *   - origin-agent-cluster.invalid      warn
 *     Header value not `?1` or `?0`. Browsers reject the
 *     header silently.
 *
 * Out of scope:
 *   * Localhost (consistent with the response-header detector
 *     family).
 *   * http pages — the header applies but the page already has
 *     bigger problems.
 *
 * Reads from the same `topLevelResponseHeaders` Map as the
 * other 12 response-header detectors. THIRTEENTH consumer of
 * the shared capture path. Uses the cycle-24
 * `responseHeaderDetector` helper.
 */

export interface OriginAgentClusterFinding {
  severity: 'strict' | 'warn';
  kind: string;
  detail: string;
  evidence: Record<string, unknown>;
}

export interface OriginAgentClusterSnapshot {
  pageUrl: string;
  pageIsLocalhost: boolean;
  /** Raw header value (trimmed), or null. */
  raw: string | null;
}

function isLocalhost(url: string): boolean {
  try {
    const u = new URL(url);
    const h = u.hostname;
    return h === 'localhost' || h === '127.0.0.1' || h === '::1' || h.endsWith('.localhost');
  } catch {
    return false;
  }
}

function getHeader(headers: Record<string, string> | undefined, name: string): string | null {
  if (!headers) return null;
  for (const [k, v] of Object.entries(headers)) {
    if (k.toLowerCase() === name) return v;
  }
  return null;
}

export function buildOriginAgentClusterSnapshot(
  pageUrl: string,
  headers: Record<string, string> | undefined,
): OriginAgentClusterSnapshot {
  const pageIsLocalhost = isLocalhost(pageUrl);
  const rawHeader = getHeader(headers, 'origin-agent-cluster');
  return {
    pageUrl,
    pageIsLocalhost,
    raw: rawHeader === null ? null : rawHeader.trim(),
  };
}

export function detectOriginAgentClusterIssues(
  snap: OriginAgentClusterSnapshot,
): OriginAgentClusterFinding[] {
  if (snap.pageIsLocalhost) return [];

  if (snap.raw === null) {
    return [
      {
        severity: 'warn',
        kind: 'origin-agent-cluster.missing',
        detail: `No Origin-Agent-Cluster header. Browser uses default (origin shares an agent cluster with other same-site origins). For sensitive-data origins, request process-level isolation via 'Origin-Agent-Cluster: ?1'. Disables document.domain mutation as a side effect — confirm no legacy code depends on it.`,
        evidence: {},
      },
    ];
  }

  if (snap.raw === '?1') return [];

  if (snap.raw === '?0') {
    return [
      {
        severity: 'warn',
        kind: 'origin-agent-cluster.disabled',
        detail: `Origin-Agent-Cluster explicitly set to '?0'. Operator opted OUT of process-level isolation. Likely a legacy document.domain compatibility need; surfaced so the choice can be confirmed.`,
        evidence: { value: snap.raw },
      },
    ];
  }

  return [
    {
      severity: 'warn',
      kind: 'origin-agent-cluster.invalid',
      detail: `Origin-Agent-Cluster value '${snap.raw}' is not the structured-fields boolean form ('?1' or '?0'). Browsers silently reject — the header has no effect.`,
      evidence: { value: snap.raw },
    },
  ];
}
