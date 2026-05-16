//! `hsts` — Strict-Transport-Security header detector.
//!
//! Mirror of `src/hstsHeader.ts`. Findings:
//!
//!   * `hsts.missing`              strict
//!   * `hsts.max-age-too-short`    warn  (< 6 months)
//!   * `hsts.no-subdomains`        warn  (adequate max-age but no includeSubDomains)
//!
//! HSTS pins TLS so a subsequent visit can't be MITM'd via http
//! before the server's redirect lands. http pages + localhost
//! exempt (loopback browsers don't honour HSTS).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// 6 months in seconds — de-facto minimum effective lifetime.
const HSTS_SIX_MONTHS_SECONDS: u64 = 6 * 30 * 24 * 60 * 60;

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HstsSnapshot {
    /// Page URL (for evidence + http/localhost gates).
    pub page_url: String,
    /// True iff the page itself was loaded over https.
    pub page_is_https: bool,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw header value, empty string if absent.
    pub hsts_value: String,
}

/// Build a snapshot from a page URL + a header map.
pub fn build_hsts_snapshot<I, K, V>(page_url: &str, headers: I) -> HstsSnapshot
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut hsts_value = String::new();
    for (k, v) in headers {
        if k.as_ref().to_ascii_lowercase() == "strict-transport-security" {
            hsts_value = v.as_ref().to_owned();
            break;
        }
    }
    HstsSnapshot {
        page_url: page_url.to_owned(),
        page_is_https: page_url.starts_with("https://"),
        page_is_localhost: is_localhost(page_url),
        hsts_value,
    }
}

/// Parse `max-age=N` (case-insensitive). Returns `None` on parse
/// failure / missing directive. Quoted form `max-age="N"` accepted.
fn parse_max_age(header_value: &str) -> Option<u64> {
    for part in header_value.split(';') {
        let t = part.trim();
        let lower = t.to_ascii_lowercase();
        let Some(rest) = lower.strip_prefix("max-age") else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let mut digits = rest.trim();
        digits = digits.trim_matches('"').trim();
        if digits.is_empty() {
            continue;
        }
        if let Ok(n) = digits.parse::<u64>() {
            return Some(n);
        }
    }
    None
}

/// Case-insensitive directive presence check.
fn has_directive(header_value: &str, directive: &str) -> bool {
    let target = directive.to_ascii_lowercase();
    for part in header_value.split(';') {
        if part.trim().to_ascii_lowercase() == target {
            return true;
        }
    }
    false
}

/// Run the detector. Returns zero findings on http / localhost.
pub fn detect_hsts_issues(snap: &HstsSnapshot) -> Vec<AxisFinding> {
    if !snap.page_is_https {
        return Vec::new();
    }
    if snap.page_is_localhost {
        return Vec::new();
    }

    let mut out = Vec::new();
    let value = snap.hsts_value.trim();

    if value.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "hsts.missing".into(),
            detail: format!(
                "Page is served over https but the response carries no \
                 Strict-Transport-Security header. First-hit users on a clean \
                 browser hit http:// (whatever they typed) and can be MITM'd \
                 before the server's redirect to https. Add 'Strict-Transport-Security: \
                 max-age=15768000; includeSubDomains' (6 months) as a minimum. \
                 (pageUrl={url})",
                url = snap.page_url
            ),
        });
        return out;
    }

    let Some(max_age) = parse_max_age(value) else {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "hsts.missing".into(),
            detail: format!(
                "Strict-Transport-Security header present ('{value}') but no max-age \
                 directive parsed. Browsers will ignore the policy. Fix the header syntax."
            ),
        });
        return out;
    };

    if max_age < HSTS_SIX_MONTHS_SECONDS {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "hsts.max-age-too-short".into(),
            detail: format!(
                "Strict-Transport-Security max-age is {max_age} seconds — less than \
                 6 months ({HSTS_SIX_MONTHS_SECONDS}). Protection lapses if the user \
                 doesn't return within the window. Use 'max-age=31536000' (1 year) \
                 or longer for production sites."
            ),
        });
    } else if !has_directive(value, "includeSubDomains") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "hsts.no-subdomains".into(),
            detail: format!(
                "Strict-Transport-Security has adequate max-age ({max_age}) but missing \
                 'includeSubDomains'. Subdomain takeovers can serve \
                 http://attacker.example.com — the apex site's HSTS won't protect them. \
                 If subdomains are under your control, add includeSubDomains."
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, hsts: &str) -> HstsSnapshot {
        if hsts.is_empty() {
            build_hsts_snapshot(url, Vec::<(&str, &str)>::new())
        } else {
            build_hsts_snapshot(url, [("strict-transport-security", hsts)])
        }
    }

    #[test]
    fn http_page_exempt() {
        assert!(detect_hsts_issues(&snap("http://example.com/", "")).is_empty());
    }

    #[test]
    fn localhost_exempt() {
        assert!(detect_hsts_issues(&snap("https://localhost/", "")).is_empty());
        assert!(detect_hsts_issues(&snap("https://127.0.0.1/", "")).is_empty());
        assert!(detect_hsts_issues(&snap("https://app.localhost/", "")).is_empty());
    }

    #[test]
    fn missing_header_strict() {
        let f = detect_hsts_issues(&snap("https://example.com/", ""));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "hsts.missing");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn one_year_max_age_with_subdomains_clean() {
        let f = detect_hsts_issues(&snap(
            "https://example.com/",
            "max-age=31536000; includeSubDomains",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn six_months_exact_boundary_with_subdomains_clean() {
        let s = format!("max-age={HSTS_SIX_MONTHS_SECONDS}; includeSubDomains");
        let f = detect_hsts_issues(&snap("https://example.com/", &s));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn short_max_age_warn() {
        let f = detect_hsts_issues(&snap("https://example.com/", "max-age=3600"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "hsts.max-age-too-short");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn no_subdomains_with_long_max_age_warn() {
        let f = detect_hsts_issues(&snap("https://example.com/", "max-age=31536000"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "hsts.no-subdomains");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn unparseable_value_treated_as_missing() {
        let f = detect_hsts_issues(&snap("https://example.com/", "garbage"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "hsts.missing");
    }

    #[test]
    fn case_insensitive_directive_names() {
        let f = detect_hsts_issues(&snap(
            "https://example.com/",
            "MAX-AGE=31536000; INCLUDESUBDOMAINS",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn quoted_max_age_value_accepted() {
        let f = detect_hsts_issues(&snap("https://example.com/", "max-age=\"31536000\"; includeSubDomains"));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn whitespace_around_equals_accepted() {
        let f = detect_hsts_issues(&snap(
            "https://example.com/",
            "max-age = 31536000; includeSubDomains",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build_hsts_snapshot(
            "https://example.com/",
            [("Strict-Transport-Security", "max-age=31536000; includeSubDomains")],
        );
        assert!(detect_hsts_issues(&s).is_empty());
    }

    #[test]
    fn preload_directive_does_not_change_outcome() {
        // preload presence is out of scope for this detector.
        let f = detect_hsts_issues(&snap(
            "https://example.com/",
            "max-age=31536000; includeSubDomains; preload",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn detail_text_includes_url_for_missing() {
        let f = detect_hsts_issues(&snap("https://acme.example.com/foo", ""));
        assert!(f[0].detail.contains("acme.example.com"));
    }
}
