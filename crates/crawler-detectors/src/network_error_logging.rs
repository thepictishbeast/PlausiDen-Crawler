//! `network_error_logging` — NEL response-header detector.
//!
//! Mirror of `src/networkErrorLogging.ts`. Findings:
//!
//!   * `nel.missing`               warn — no NEL header
//!   * `nel.invalid`               warn — header set but not JSON object
//!   * `nel.report-to-missing`     warn — parses but no `report_to`
//!   * `nel.max-age-zero`          warn — explicit opt-out via max_age=0
//!   * `nel.failure-fraction-zero` warn — failure_fraction=0 disables
//!                                          the primary purpose of NEL
//!
//! NEL (W3C, Chromium since 2018) extends Reporting-API to network-
//! level failures: TLS handshake, DNS resolution, TCP RST. Without
//! it those failures are invisible — the user sees "site can't be
//! reached" and the operator sees nothing.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Parsed NEL policy. All fields optional; absent = browser default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct NelPolicy {
    /// Group name in the Reporting-Endpoints header to POST to.
    pub report_to: Option<String>,
    /// Seconds the policy applies for. `0` = explicit immediate drop.
    pub max_age: Option<u64>,
    /// Apply to subdomains too. Default: false.
    pub include_subdomains: Option<bool>,
    /// Sampling rate for HTTP-success reports. `0.0..=1.0`.
    pub success_fraction: Option<f64>,
    /// Sampling rate for HTTP-failure / transport-failure reports.
    pub failure_fraction: Option<f64>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct NelSnapshot {
    /// Page URL (for localhost exemption).
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw NEL header value (trimmed), or `None` if absent.
    pub raw: Option<String>,
    /// Parsed policy. Default when header is absent or unparseable.
    pub parsed: NelPolicy,
    /// True iff the header was present but JSON-parse failed.
    pub unparseable: bool,
}

/// Build a snapshot from a captured response-headers map.
pub fn build_nel_snapshot(
    page_url: &str,
    headers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> NelSnapshot {
    let page_is_localhost = is_localhost(page_url);
    let mut raw: Option<String> = None;
    for (k, v) in headers {
        if k.as_ref().eq_ignore_ascii_case("nel") {
            raw = Some(v.as_ref().trim().to_owned());
            break;
        }
    }
    let (parsed, unparseable) = match raw.as_deref() {
        Some(s) if !s.is_empty() => match serde_json::from_str::<NelPolicy>(s) {
            Ok(p) => (p, false),
            Err(_) => (NelPolicy::default(), true),
        },
        _ => (NelPolicy::default(), false),
    };
    NelSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        raw,
        parsed,
        unparseable,
    }
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_nel_issues(snap: &NelSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }

    if snap.raw.is_none() {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "nel.missing".into(),
            detail: "No NEL (Network-Error-Logging) header. TLS handshake failures, DNS errors, TCP RSTs, and other pre-HTTP failures are unreportable. Pairs with Reporting-Endpoints — add 'NEL: {\"report_to\":\"default\",\"max_age\":2592000,\"failure_fraction\":1.0}' so the browser POSTs network-level failures to your collector.".into(),
        }];
    }

    if snap.unparseable {
        let raw = snap.raw.as_deref().unwrap_or("");
        let preview: String = raw.chars().take(200).collect();
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "nel.invalid".into(),
            detail: format!(
                "NEL header present but not a valid JSON object: '{preview}'. Browsers silently ignore unparseable values. Expected form: '{{\"report_to\":\"<group>\",\"max_age\":<seconds>,\"failure_fraction\":<0..1>}}'."
            ),
        }];
    }

    let mut out = Vec::new();

    if snap.parsed.report_to.as_deref().map_or(true, str::is_empty) {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "nel.report-to-missing".into(),
            detail: "NEL header parses but contains no 'report_to' field (or it's empty). The browser has nowhere to send network-error reports. Add 'report_to': '<Reporting-Endpoints group name>'.".into(),
        });
    }

    if snap.parsed.max_age == Some(0) {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "nel.max-age-zero".into(),
            detail: "NEL 'max_age' is 0 — explicit opt-out. The browser drops the policy immediately on receipt. If this is intentional (decommissioning the collector), confirm; otherwise set 'max_age' to a positive seconds value (recommended: 2592000 = 30 days).".into(),
        });
    }

    if snap.parsed.failure_fraction == Some(0.0) {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "nel.failure-fraction-zero".into(),
            detail: "NEL 'failure_fraction' is 0 — failure reports are explicitly disabled. The primary purpose of NEL (catching TLS / DNS / TCP failures) is defeated. If privacy / data volume is the concern, sample at 0.01-0.1 instead.".into(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(headers: &[(&str, &str)], url: &str) -> NelSnapshot {
        build_nel_snapshot(url, headers.iter().map(|(k, v)| (*k, *v)))
    }

    #[test]
    fn localhost_skipped() {
        let s = snap(&[], "http://localhost:8000/");
        assert!(detect_nel_issues(&s).is_empty());
    }

    #[test]
    fn missing_header_warns() {
        let s = snap(&[], "https://example.com/");
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.missing");
    }

    #[test]
    fn well_formed_policy_passes() {
        let s = snap(
            &[(
                "NEL",
                r#"{"report_to":"default","max_age":2592000,"failure_fraction":1.0}"#,
            )],
            "https://example.com/",
        );
        assert!(detect_nel_issues(&s).is_empty());
    }

    #[test]
    fn non_json_warns_invalid() {
        let s = snap(&[("NEL", "not-json")], "https://example.com/");
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.invalid");
    }

    #[test]
    fn json_array_warns_invalid() {
        // Arrays don't deserialize into NelPolicy struct → unparseable.
        let s = snap(&[("NEL", "[]")], "https://example.com/");
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.invalid");
    }

    #[test]
    fn missing_report_to_warns() {
        let s = snap(
            &[("NEL", r#"{"max_age":2592000,"failure_fraction":1.0}"#)],
            "https://example.com/",
        );
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.report-to-missing");
    }

    #[test]
    fn empty_report_to_warns() {
        let s = snap(
            &[(
                "NEL",
                r#"{"report_to":"","max_age":2592000,"failure_fraction":1.0}"#,
            )],
            "https://example.com/",
        );
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.report-to-missing");
    }

    #[test]
    fn max_age_zero_warns() {
        let s = snap(
            &[(
                "NEL",
                r#"{"report_to":"default","max_age":0,"failure_fraction":1.0}"#,
            )],
            "https://example.com/",
        );
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.max-age-zero");
    }

    #[test]
    fn failure_fraction_zero_warns() {
        let s = snap(
            &[(
                "NEL",
                r#"{"report_to":"default","max_age":2592000,"failure_fraction":0}"#,
            )],
            "https://example.com/",
        );
        let f = detect_nel_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "nel.failure-fraction-zero");
    }

    #[test]
    fn multiple_findings_combined() {
        let s = snap(
            &[("NEL", r#"{"max_age":0,"failure_fraction":0}"#)],
            "https://example.com/",
        );
        let f = detect_nel_issues(&s);
        // missing report_to + max_age_zero + failure_fraction_zero
        assert_eq!(f.len(), 3);
        let kinds: Vec<&str> = f.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"nel.report-to-missing"));
        assert!(kinds.contains(&"nel.max-age-zero"));
        assert!(kinds.contains(&"nel.failure-fraction-zero"));
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let s = snap(
            &[(
                "nel",
                r#"{"report_to":"default","max_age":2592000,"failure_fraction":1.0}"#,
            )],
            "https://example.com/",
        );
        assert!(detect_nel_issues(&s).is_empty());
    }
}
