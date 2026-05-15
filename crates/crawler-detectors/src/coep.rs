//! `coep` — Cross-Origin-Embedder-Policy header detector.
//!
//! Mirror of `src/coep.ts`. Findings:
//!
//!   * `coep.missing`     warn  — header absent → defaults to unsafe-none
//!   * `coep.unsafe-none` warn  — explicit opt-out of isolation
//!   * `coep.invalid`     warn  — unrecognised value → effective unsafe-none
//!
//! COEP, paired with COOP=`same-origin`, enables the
//! `crossOriginIsolated` document state — the modern primitive
//! that gates `SharedArrayBuffer` and high-resolution timers
//! used to mitigate Spectre-class side channels.
//!
//! Acceptable values: `require-corp`, `credentialless`.
//! Recognised but discouraged: `unsafe-none`.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CoepSnapshot {
    /// Page URL (used for localhost exemption).
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw header value, lowercased + trimmed, or `None` if absent.
    pub raw: Option<String>,
}

const ACCEPTABLE: &[&str] = &["require-corp", "credentialless"];
const RECOGNISED: &[&str] = &["require-corp", "credentialless", "unsafe-none"];

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
pub fn build_coep_snapshot<I, K, V>(page_url: &str, headers: I) -> CoepSnapshot
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let raw = lookup_header(headers, "cross-origin-embedder-policy")
        .map(|s| s.trim().to_ascii_lowercase());
    CoepSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost: is_localhost(page_url),
        raw,
    }
}

/// Run the detector. Returns zero findings on localhost / loopback.
pub fn detect_coep_issues(snap: &CoepSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let Some(value) = snap.raw.as_deref() else {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "coep.missing".into(),
            detail: "No Cross-Origin-Embedder-Policy header. Defaults to 'unsafe-none' — \
                cross-origin isolation cannot be enabled, so SharedArrayBuffer + \
                high-resolution timers stay disabled and Spectre-class side-channel \
                mitigations are unavailable. Set to 'require-corp' (strictest, requires \
                every cross-origin sub-resource to opt in via CORP) or 'credentialless' \
                (newer; allows cross-origin embeds without credentials)."
                .into(),
        }];
    };
    if value == "unsafe-none" {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "coep.unsafe-none".into(),
            detail: format!(
                "Cross-Origin-Embedder-Policy explicitly set to 'unsafe-none'. \
                 Cross-origin isolation is disabled. Confirm this is intentional — \
                 the supersociety baseline is 'require-corp'. (value={value})"
            ),
        }];
    }
    if !RECOGNISED.iter().any(|r| *r == value) {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "coep.invalid".into(),
            detail: format!(
                "Cross-Origin-Embedder-Policy value '{value}' is not in the W3C-recognised \
                 set ('require-corp', 'credentialless', 'unsafe-none'). Browsers ignore \
                 unknown values and fall back to 'unsafe-none'."
            ),
        }];
    }
    debug_assert!(ACCEPTABLE.iter().any(|a| *a == value));
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, raw: Option<&str>) -> CoepSnapshot {
        let headers: Vec<(&str, &str)> = match raw {
            Some(v) => vec![("cross-origin-embedder-policy", v)],
            None => vec![],
        };
        build_coep_snapshot(url, headers)
    }

    #[test]
    fn localhost_exempt() {
        assert!(detect_coep_issues(&snap("https://localhost/", None)).is_empty());
        assert!(detect_coep_issues(&snap("http://127.0.0.1:8080/", None)).is_empty());
    }

    #[test]
    fn missing_header_warn() {
        let f = detect_coep_issues(&snap("https://example.com/", None));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "coep.missing");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn require_corp_no_finding() {
        let f = detect_coep_issues(&snap("https://example.com/", Some("require-corp")));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn credentialless_no_finding() {
        let f = detect_coep_issues(&snap("https://example.com/", Some("credentialless")));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn unsafe_none_explicit_warn() {
        let f = detect_coep_issues(&snap("https://example.com/", Some("unsafe-none")));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "coep.unsafe-none");
    }

    #[test]
    fn unrecognised_value_warn() {
        let f = detect_coep_issues(&snap("https://example.com/", Some("banana")));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "coep.invalid");
    }

    #[test]
    fn case_insensitive_value() {
        let f = detect_coep_issues(&snap("https://example.com/", Some("REQUIRE-CORP")));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn whitespace_trimmed() {
        let f = detect_coep_issues(&snap("https://example.com/", Some(" require-corp ")));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build_coep_snapshot(
            "https://example.com/",
            [("Cross-Origin-Embedder-Policy", "require-corp")],
        );
        assert!(detect_coep_issues(&s).is_empty());
    }

    #[test]
    fn http_page_still_warned() {
        let f = detect_coep_issues(&snap("http://example.com/", None));
        assert_eq!(f.len(), 1, "http pages still warned (cost-of-fix is zero)");
    }

    #[test]
    fn dot_localhost_subdomain_exempt() {
        assert!(detect_coep_issues(&snap("https://app.localhost/", None)).is_empty());
    }
}
