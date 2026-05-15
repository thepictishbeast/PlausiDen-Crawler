//! `coop` — Cross-Origin-Opener-Policy header detector.
//!
//! Mirror of `src/coop.ts`. Findings:
//!
//!   * `coop.missing`     warn  — header absent → defaults to unsafe-none
//!   * `coop.unsafe-none` warn  — explicit opt-out of isolation
//!   * `coop.invalid`     warn  — unrecognised value → effective unsafe-none
//!
//! COOP gates `window.opener` access from cross-origin tabs and is
//! one of the two headers (with COEP) that enable cross-origin
//! isolation, required for `SharedArrayBuffer` + Spectre mitigations.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CoopSnapshot {
    /// Page URL (used for localhost exemption).
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw header value, lowercased + trimmed, or `None` if absent.
    pub raw: Option<String>,
}

const ACCEPTABLE: &[&str] = &[
    "same-origin",
    "same-origin-allow-popups",
    "same-origin-plus-coep",
    "noopener-allow-popups",
];

const RECOGNISED: &[&str] = &[
    "same-origin",
    "same-origin-allow-popups",
    "same-origin-plus-coep",
    "noopener-allow-popups",
    "unsafe-none",
];

use crate::url_helpers::is_localhost;

fn lookup_header<I, K, V>(headers: I, name_lower: &str) -> Option<String>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    for (k, v) in headers {
        if k.as_ref().to_ascii_lowercase() == name_lower {
            return Some(v.as_ref().to_owned());
        }
    }
    None
}

/// Build a snapshot from a page URL + a header map.
pub fn build_coop_snapshot<I, K, V>(page_url: &str, headers: I) -> CoopSnapshot
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let raw = lookup_header(headers, "cross-origin-opener-policy")
        .map(|s| s.trim().to_ascii_lowercase());
    CoopSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost: is_localhost(page_url),
        raw,
    }
}

/// Run the detector. Returns zero findings on localhost / loopback.
pub fn detect_coop_issues(snap: &CoopSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let Some(value) = snap.raw.as_deref() else {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "coop.missing".into(),
            detail: "No Cross-Origin-Opener-Policy header. Defaults to 'unsafe-none' — \
                cross-origin openers can read window.opener and run tab-nabbing or timing attacks. \
                Cross-origin isolation (required for SharedArrayBuffer + Spectre mitigation) cannot be enabled. \
                Set to 'same-origin' for the strictest protection or 'same-origin-allow-popups' if you open popups."
                .into(),
        }];
    };
    if value == "unsafe-none" {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "coop.unsafe-none".into(),
            detail: format!(
                "Cross-Origin-Opener-Policy explicitly set to 'unsafe-none'. \
                 The opener relationship remains scriptable across origins; cross-origin isolation is disabled. \
                 Confirm this is intentional — the supersociety baseline is 'same-origin'. (value={value})"
            ),
        }];
    }
    if !RECOGNISED.iter().any(|r| *r == value) {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "coop.invalid".into(),
            detail: format!(
                "Cross-Origin-Opener-Policy value '{value}' is not in the W3C-recognised set \
                 ('same-origin', 'same-origin-allow-popups', 'same-origin-plus-coep', 'noopener-allow-popups', 'unsafe-none'). \
                 Browsers ignore unknown values and fall back to 'unsafe-none'."
            ),
        }];
    }
    debug_assert!(ACCEPTABLE.iter().any(|a| *a == value));
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, raw: Option<&str>) -> CoopSnapshot {
        let headers: Vec<(&str, &str)> = match raw {
            Some(v) => vec![("cross-origin-opener-policy", v)],
            None => vec![],
        };
        build_coop_snapshot(url, headers)
    }

    #[test]
    fn localhost_exempt() {
        assert!(detect_coop_issues(&snap("https://localhost/", None)).is_empty());
        assert!(detect_coop_issues(&snap("http://127.0.0.1:8080/", None)).is_empty());
    }

    #[test]
    fn missing_header_warn() {
        let f = detect_coop_issues(&snap("https://example.com/", None));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "coop.missing");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn unsafe_none_explicit_warn() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("unsafe-none")));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "coop.unsafe-none");
    }

    #[test]
    fn same_origin_no_finding() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("same-origin")));
        assert!(f.is_empty());
    }

    #[test]
    fn same_origin_allow_popups_no_finding() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("same-origin-allow-popups")));
        assert!(f.is_empty());
    }

    #[test]
    fn same_origin_plus_coep_no_finding() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("same-origin-plus-coep")));
        assert!(f.is_empty());
    }

    #[test]
    fn noopener_allow_popups_no_finding() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("noopener-allow-popups")));
        assert!(f.is_empty());
    }

    #[test]
    fn unrecognised_value_warn() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("banana")));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "coop.invalid");
    }

    #[test]
    fn case_insensitive_value() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("SAME-ORIGIN")));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn whitespace_around_value_trimmed() {
        let f = detect_coop_issues(&snap("https://example.com/", Some("  same-origin  ")));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build_coop_snapshot(
            "https://example.com/",
            [("Cross-Origin-Opener-Policy", "same-origin")],
        );
        assert!(detect_coop_issues(&s).is_empty());
    }

    #[test]
    fn http_page_still_warned() {
        let f = detect_coop_issues(&snap("http://example.com/", None));
        assert_eq!(f.len(), 1, "http pages still warned (cost-of-fix is zero)");
    }

    #[test]
    fn dot_localhost_subdomain_exempt() {
        assert!(detect_coop_issues(&snap("https://app.localhost/", None)).is_empty());
    }
}
