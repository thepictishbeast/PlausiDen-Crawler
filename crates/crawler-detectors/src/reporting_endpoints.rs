//! `reporting_endpoints` — Reporting API endpoint configuration audit.
//! T76 port of `src/reportingEndpoints.ts`.
//!
//! The Reporting API (W3C, browser-shipping since 2018, modern
//! `Reporting-Endpoints` header standardised 2023) is the modern
//! transport for browser-emitted security telemetry: CSP / COEP /
//! CORP / Document-Policy violations, crash reports, intervention
//! reports, deprecation reports. Without endpoints configured, the
//! reports are silently lost — observability is incomplete.
//!
//! Findings (mirrors TS byte-for-byte):
//!
//!   * `reporting.invalid`                       — warn
//!   * `reporting.no-endpoints`                  — warn
//!   * `reporting.report-to-only`                — warn
//!   * `reporting.csp-report-uri-no-endpoints`   — warn
//!   * `reporting.csp-group-undeclared`          — warn (cross-header
//!     consistency: CSP `report-to <name>` names a group not declared
//!     in `Reporting-Endpoints` — silent-drop trap)
//!   * `reporting.endpoint-orphan`               — warn
//!   * `reporting.endpoint-not-https`            — warn
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One parsed `Reporting-Endpoints` dictionary entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ParsedReportingEndpoint {
    /// Lowercase group name.
    pub name: String,
    /// Endpoint URL (verbatim from the header — same-origin paths
    /// like `/reports` are valid and inherit the page scheme).
    pub url: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReportingEndpointsSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw `Reporting-Endpoints` value, or `None`.
    pub raw_reporting_endpoints: Option<String>,
    /// Parsed entries (empty when unparseable or absent).
    pub endpoints: Vec<ParsedReportingEndpoint>,
    /// True iff Reporting-Endpoints was present-but-unparseable.
    pub reporting_endpoints_unparseable: bool,
    /// Raw legacy `Report-To` value, or `None`.
    pub raw_report_to: Option<String>,
    /// Raw `Content-Security-Policy` value (for cross-checking
    /// `report-uri` / `report-to` references), or `None`.
    pub raw_csp: Option<String>,
}

fn header_lookup<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// Parse the dictionary `Reporting-Endpoints` header.
/// Tolerates whitespace and optional double-quotes around the URL.
fn parse_reporting_endpoints(raw: &str) -> Vec<ParsedReportingEndpoint> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(eq) = trimmed.find('=') else {
            continue;
        };
        if eq == 0 {
            continue;
        }
        let name = trimmed[..eq].trim().to_owned();
        let mut value = trimmed[eq + 1..].trim().to_owned();
        if name.is_empty() {
            continue;
        }
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = value[1..value.len() - 1].to_owned();
        }
        if value.is_empty() {
            continue;
        }
        out.push(ParsedReportingEndpoint { name, url: value });
    }
    out
}

fn csp_mentions_reporting(raw_csp: Option<&str>) -> bool {
    let Some(csp) = raw_csp else {
        return false;
    };
    let lower = csp.to_ascii_lowercase();
    // Word-boundary equivalent — split by non-token chars; CSP is
    // semicolon + space separated, so a contains check is safe enough
    // when paired with the lowercase normalise.
    for tok in lower.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
        if tok == "report-uri" || tok == "report-to" {
            return true;
        }
    }
    false
}

/// Extract the FIRST `report-to <group>` group name from a CSP.
/// Browsers only honour the first occurrence.
fn csp_report_to_group(raw_csp: Option<&str>) -> Option<String> {
    let csp = raw_csp?;
    for seg in csp.split(';') {
        let t = seg.trim();
        let lower = t.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("report-to") {
            // ensure separator after directive name
            let rest_chars = rest.chars();
            if let Some(first) = rest_chars.clone().next() {
                if !first.is_whitespace() {
                    continue;
                }
            } else {
                continue;
            }
            // Pull the first whitespace-separated token from the
            // ORIGINAL-CASE segment so we don't lowercase the group.
            let original_after =
                t[lower.find("report-to").unwrap_or(0) + "report-to".len()..].trim_start();
            let first_tok = original_after.split_whitespace().next();
            if let Some(name) = first_tok {
                if !name.is_empty() {
                    return Some(name.to_owned());
                }
            }
        }
    }
    None
}

/// True iff the URL starts with `http://` or `https://` (case-insens).
fn is_absolute_http_url(u: &str) -> bool {
    let lower = u.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Build a snapshot from a captured headers map.
pub fn build_reporting_endpoints_snapshot(
    page_url: &str,
    headers: &BTreeMap<String, String>,
) -> ReportingEndpointsSnapshot {
    let page_is_localhost = is_localhost(page_url);
    let raw_reporting_endpoints = header_lookup(headers, "reporting-endpoints").cloned();
    let endpoints = raw_reporting_endpoints
        .as_deref()
        .map_or_else(Vec::new, parse_reporting_endpoints);
    let reporting_endpoints_unparseable = raw_reporting_endpoints
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty() && endpoints.is_empty());
    let raw_report_to = header_lookup(headers, "report-to").cloned();
    let raw_csp = header_lookup(headers, "content-security-policy").cloned();

    ReportingEndpointsSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        raw_reporting_endpoints,
        endpoints,
        reporting_endpoints_unparseable,
        raw_report_to,
        raw_csp,
    }
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_reporting_endpoints_issues(snap: &ReportingEndpointsSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let mut out = Vec::new();
    let has_modern = !snap.endpoints.is_empty();
    let has_legacy = snap
        .raw_report_to
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());

    if snap.reporting_endpoints_unparseable {
        let raw = snap
            .raw_reporting_endpoints
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "reporting.invalid".into(),
            detail: format!(
                "Reporting-Endpoints header is set but no valid 'name=URL' pair could be parsed. Browsers ignore unparseable values — the entire reporting pipeline is silently broken. Header value: '{raw}'."
            ),
        });
    }

    if !has_modern && !has_legacy {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "reporting.no-endpoints".into(),
            detail: "Neither Reporting-Endpoints nor Report-To header is present. ALL browser-emitted security reports (CSP violations, COEP violations, crash reports, intervention reports, deprecation warnings) are LOST — the page's own telemetry has nowhere to go. Add a 'Reporting-Endpoints: csp-default=\"https://your-collector.example/csp\"' header and reference it from CSP via 'report-to csp-default'.".into(),
        });
    } else if !has_modern && has_legacy {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "reporting.report-to-only".into(),
            detail: "Legacy Report-To header set but no modern Reporting-Endpoints. Modern browsers prefer Reporting-Endpoints (W3C 2023); they may emit deprecation warnings and stop honouring Report-To in future versions. Migrate.".into(),
        });
    }

    if csp_mentions_reporting(snap.raw_csp.as_deref()) && !has_modern && !has_legacy {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "reporting.csp-report-uri-no-endpoints".into(),
            detail: "Content-Security-Policy includes 'report-uri' or 'report-to' directive but no Reporting-Endpoints / Report-To header is set up to receive the reports. They go nowhere. Pair the CSP directive with a Reporting-Endpoints header.".into(),
        });
    }

    let csp_group = csp_report_to_group(snap.raw_csp.as_deref());
    if let Some(grp) = csp_group.as_deref() {
        if has_modern {
            let declared: BTreeSet<&str> = snap.endpoints.iter().map(|e| e.name.as_str()).collect();
            if !declared.contains(grp) {
                let declared_list: Vec<String> =
                    declared.iter().map(|n| format!("'{n}'")).collect();
                out.push(AxisFinding {
                    severity: AxisSeverity::Warn,
                    kind: "reporting.csp-group-undeclared".into(),
                    detail: format!(
                        "Content-Security-Policy includes 'report-to {grp}' but the Reporting-Endpoints header declares no group named '{grp}'. Browsers silently drop the violation reports. Declared groups: {}. Fix: rename the CSP group OR add 'Reporting-Endpoints: {grp}=\"<url>\"'.",
                        if declared_list.is_empty() {
                            "(none)".to_owned()
                        } else {
                            declared_list.join(", ")
                        }
                    ),
                });
            }
        }
    }

    if let Some(grp) = csp_group.as_deref() {
        if has_modern {
            let orphans: Vec<&str> = snap
                .endpoints
                .iter()
                .map(|e| e.name.as_str())
                .filter(|n| *n != grp)
                .collect();
            if !orphans.is_empty() {
                let formatted: Vec<String> = orphans.iter().map(|n| format!("'{n}'")).collect();
                out.push(AxisFinding {
                    severity: AxisSeverity::Warn,
                    kind: "reporting.endpoint-orphan".into(),
                    detail: format!(
                        "Reporting-Endpoints declares group(s) {} that no CSP 'report-to' directive references. Reports for these groups will never fire unless another policy (Document-Policy, COOP, COEP, NEL) names them. Either reference the group or remove the orphan declaration.",
                        formatted.join(", ")
                    ),
                });
            }
        }
    }

    let insecure: Vec<&ParsedReportingEndpoint> = snap
        .endpoints
        .iter()
        .filter(|e| {
            is_absolute_http_url(&e.url) && e.url.to_ascii_lowercase().starts_with("http://")
        })
        .collect();
    if !insecure.is_empty() {
        let names: Vec<String> = insecure.iter().map(|e| format!("'{}'", e.name)).collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "reporting.endpoint-not-https".into(),
            detail: format!(
                "{} Reporting-Endpoints URL(s) use plaintext http:// — reports leak in transit and can be tampered with by network-position attackers. Use https://, or a same-origin path that inherits the page origin scheme. Affected: {}.",
                insecure.len(),
                names.join(", ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> &'static str {
        "https://example.com/"
    }

    fn build(headers: &[(&str, &str)]) -> ReportingEndpointsSnapshot {
        let map: BTreeMap<String, String> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        build_reporting_endpoints_snapshot(page(), &map)
    }

    #[test]
    fn localhost_skipped() {
        let map = BTreeMap::new();
        let s = build_reporting_endpoints_snapshot("http://localhost/", &map);
        assert!(detect_reporting_endpoints_issues(&s).is_empty());
    }

    #[test]
    fn no_headers_emits_no_endpoints_finding() {
        let s = build(&[]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f.iter().any(|x| x.kind == "reporting.no-endpoints"));
    }

    #[test]
    fn unparseable_header_warns() {
        let s = build(&[("Reporting-Endpoints", "garbage value with no equals")]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f.iter().any(|x| x.kind == "reporting.invalid"));
    }

    #[test]
    fn parses_dict_endpoints() {
        let s = build(&[(
            "Reporting-Endpoints",
            "csp-default=\"https://example.com/csp\", crash-reports=\"https://example.com/crash\"",
        )]);
        assert_eq!(s.endpoints.len(), 2);
        assert_eq!(s.endpoints[0].name, "csp-default");
        assert_eq!(s.endpoints[0].url, "https://example.com/csp");
    }

    #[test]
    fn legacy_report_to_only_warns() {
        let s = build(&[("Report-To", "{\"group\":\"csp\",\"endpoints\":[]}")]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f.iter().any(|x| x.kind == "reporting.report-to-only"));
        assert!(!f.iter().any(|x| x.kind == "reporting.no-endpoints"));
    }

    #[test]
    fn csp_references_reports_without_endpoints_warns() {
        let s = build(&[(
            "Content-Security-Policy",
            "default-src 'self'; report-to csp-default",
        )]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "reporting.csp-report-uri-no-endpoints"));
    }

    #[test]
    fn csp_group_undeclared_warns() {
        let s = build(&[
            (
                "Reporting-Endpoints",
                "different-name=\"https://example.com/csp\"",
            ),
            (
                "Content-Security-Policy",
                "default-src 'self'; report-to csp-default",
            ),
        ]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f.iter().any(|x| x.kind == "reporting.csp-group-undeclared"));
    }

    #[test]
    fn endpoint_orphan_warns() {
        let s = build(&[
            (
                "Reporting-Endpoints",
                "csp-default=\"https://example.com/csp\", crash-reports=\"https://example.com/crash\"",
            ),
            (
                "Content-Security-Policy",
                "default-src 'self'; report-to csp-default",
            ),
        ]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f.iter().any(|x| x.kind == "reporting.endpoint-orphan"));
    }

    #[test]
    fn plaintext_endpoint_url_warns() {
        let s = build(&[(
            "Reporting-Endpoints",
            "csp-default=\"http://insecure.example.com/csp\"",
        )]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(f.iter().any(|x| x.kind == "reporting.endpoint-not-https"));
    }

    #[test]
    fn same_origin_relative_endpoint_is_ok() {
        let s = build(&[("Reporting-Endpoints", "csp-default=\"/reports/csp\"")]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "reporting.endpoint-not-https"));
    }

    #[test]
    fn clean_modern_setup_no_findings() {
        let s = build(&[
            (
                "Reporting-Endpoints",
                "csp-default=\"https://example.com/csp\"",
            ),
            (
                "Content-Security-Policy",
                "default-src 'self'; report-to csp-default",
            ),
        ]);
        let f = detect_reporting_endpoints_issues(&s);
        assert!(
            f.is_empty(),
            "clean modern setup should be silent, got: {f:#?}"
        );
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build(&[(
            "REPORTING-ENDPOINTS",
            "csp-default=\"https://example.com/csp\"",
        )]);
        assert_eq!(s.endpoints.len(), 1);
    }

    #[test]
    fn csp_first_report_to_group_wins() {
        // Multiple `report-to` directives — browsers only honour the
        // first. The TS source's `cspReportToGroup` returns the
        // group from the first matching segment.
        let s = build(&[
            (
                "Reporting-Endpoints",
                "first=\"https://example.com/1\", second=\"https://example.com/2\"",
            ),
            (
                "Content-Security-Policy",
                "default-src 'self'; report-to first; report-to second",
            ),
        ]);
        let f = detect_reporting_endpoints_issues(&s);
        // 'first' is declared so no csp-group-undeclared
        assert!(!f.iter().any(|x| x.kind == "reporting.csp-group-undeclared"));
        // 'second' is orphaned relative to first
        assert!(f.iter().any(|x| x.kind == "reporting.endpoint-orphan"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = build(&[
            (
                "Reporting-Endpoints",
                "csp-default=\"https://example.com/csp\"",
            ),
            ("Report-To", "{\"group\":\"x\"}"),
            (
                "Content-Security-Policy",
                "default-src 'self'; report-to csp-default",
            ),
        ]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: ReportingEndpointsSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.endpoints.len(), s.endpoints.len());
    }
}
