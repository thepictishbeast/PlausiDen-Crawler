//! `origin_agent_cluster` — Origin-Agent-Cluster header detector.
//!
//! Mirror of `src/originAgentCluster.ts`. Findings:
//!
//!   * `origin-agent-cluster.missing`  warn — header absent
//!   * `origin-agent-cluster.disabled` warn — explicit `?0` opt-out
//!   * `origin-agent-cluster.invalid`  warn — non-structured-field value
//!
//! Origin-Agent-Cluster (HTML Living Standard, 2021) requests
//! process-level isolation distinct from COOP/COEP cross-origin
//! isolation. With `?1`, the origin gets its own agent cluster
//! (separate from other same-site origins), document.domain
//! mutation is disabled, and some Spectre-class side channels
//! become harder to exploit.
//!
//! Acceptable values per RFC 8941 structured-fields boolean:
//!   * `?1` — opt in (the desired state)
//!   * `?0` — explicit opt-out (matches default behaviour)
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OriginAgentClusterSnapshot {
    /// Page URL (used for localhost exemption).
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw header value (trimmed), or `None` if absent.
    pub raw: Option<String>,
}

/// Build a snapshot from a captured response-headers map.
pub fn build_origin_agent_cluster_snapshot(
    page_url: &str,
    headers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> OriginAgentClusterSnapshot {
    let page_is_localhost = is_localhost(page_url);
    let mut raw = None;
    for (k, v) in headers {
        if k.as_ref().eq_ignore_ascii_case("origin-agent-cluster") {
            raw = Some(v.as_ref().trim().to_owned());
            break;
        }
    }
    OriginAgentClusterSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        raw,
    }
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_origin_agent_cluster_issues(snap: &OriginAgentClusterSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }

    match snap.raw.as_deref() {
        None => vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "origin-agent-cluster.missing".into(),
            detail: "No Origin-Agent-Cluster header. Browser uses default (origin shares an agent cluster with other same-site origins). For sensitive-data origins, request process-level isolation via 'Origin-Agent-Cluster: ?1'. Disables document.domain mutation as a side effect — confirm no legacy code depends on it.".into(),
        }],
        Some("?1") => Vec::new(),
        Some("?0") => vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "origin-agent-cluster.disabled".into(),
            detail: "Origin-Agent-Cluster explicitly set to '?0'. Operator opted OUT of process-level isolation. Likely a legacy document.domain compatibility need; surfaced so the choice can be confirmed.".into(),
        }],
        Some(other) => vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "origin-agent-cluster.invalid".into(),
            detail: format!(
                "Origin-Agent-Cluster value '{other}' is not the structured-fields boolean form ('?1' or '?0'). Browsers silently reject — the header has no effect."
            ),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(headers: &[(&str, &str)], page_url: &str) -> OriginAgentClusterSnapshot {
        build_origin_agent_cluster_snapshot(page_url, headers.iter().map(|(k, v)| (*k, *v)))
    }

    #[test]
    fn localhost_is_skipped() {
        let s = snap(&[], "http://localhost:8000/");
        assert!(detect_origin_agent_cluster_issues(&s).is_empty());
    }

    #[test]
    fn loopback_ipv4_is_skipped() {
        let s = snap(&[], "http://127.0.0.1:3000/");
        assert!(detect_origin_agent_cluster_issues(&s).is_empty());
    }

    #[test]
    fn missing_header_warns() {
        let s = snap(&[("Content-Type", "text/html")], "https://example.com/");
        let f = detect_origin_agent_cluster_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "origin-agent-cluster.missing");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn opt_in_passes_clean() {
        let s = snap(&[("Origin-Agent-Cluster", "?1")], "https://example.com/");
        assert!(detect_origin_agent_cluster_issues(&s).is_empty());
    }

    #[test]
    fn opt_in_with_whitespace_passes() {
        let s = snap(
            &[("Origin-Agent-Cluster", "  ?1  ")],
            "https://example.com/",
        );
        assert!(detect_origin_agent_cluster_issues(&s).is_empty());
    }

    #[test]
    fn opt_out_explicit_warns() {
        let s = snap(&[("Origin-Agent-Cluster", "?0")], "https://example.com/");
        let f = detect_origin_agent_cluster_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "origin-agent-cluster.disabled");
    }

    #[test]
    fn invalid_value_warns() {
        let s = snap(&[("Origin-Agent-Cluster", "yes")], "https://example.com/");
        let f = detect_origin_agent_cluster_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "origin-agent-cluster.invalid");
        // Evidence carries the offending value for the audit trail.
        assert!(f[0].detail.contains("'yes'"));
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let s = snap(&[("ORIGIN-AGENT-CLUSTER", "?1")], "https://example.com/");
        assert!(detect_origin_agent_cluster_issues(&s).is_empty());
    }

    #[test]
    fn empty_string_value_is_invalid() {
        let s = snap(&[("Origin-Agent-Cluster", "")], "https://example.com/");
        let f = detect_origin_agent_cluster_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "origin-agent-cluster.invalid");
    }
}
