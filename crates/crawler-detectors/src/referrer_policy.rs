//! `referrer_policy` — Referrer-Policy header detector.
//!
//! Mirror of `src/referrerPolicy.ts`. Findings:
//!
//!   * `referrer-policy.missing`     warn
//!   * `referrer-policy.permissive`  strict
//!   * `referrer-policy.invalid`     warn
//!
//! Permissive policies (`unsafe-url`, `no-referrer-when-downgrade`,
//! `origin-when-cross-origin`) leak more than the modern default
//! (`strict-origin-when-cross-origin`) — they can expose paths +
//! query strings to third-party fetches.
//!
//! Browsers apply the LAST recognised token in a comma-separated
//! list (the spec's override-with-newer-value pattern).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReferrerPolicySnapshot {
    /// Page URL (for evidence + localhost exemption).
    pub page_url: String,
    /// True iff the page was loaded over https.
    pub page_is_https: bool,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw value of the Referrer-Policy header, empty if absent.
    pub policy_value: String,
}

const VALID_TOKENS: &[&str] = &[
    "no-referrer",
    "no-referrer-when-downgrade",
    "origin",
    "origin-when-cross-origin",
    "same-origin",
    "strict-origin",
    "strict-origin-when-cross-origin",
    "unsafe-url",
];

const PERMISSIVE_TOKENS: &[&str] = &[
    "unsafe-url",
    "no-referrer-when-downgrade",
    "origin-when-cross-origin",
];

/// Build a snapshot from a page URL + a header map.
pub fn build_referrer_policy_snapshot<I, K, V>(
    page_url: &str,
    headers: I,
) -> ReferrerPolicySnapshot
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut policy_value = String::new();
    for (k, v) in headers {
        if k.as_ref().to_ascii_lowercase() == "referrer-policy" {
            policy_value = v.as_ref().to_owned();
            break;
        }
    }
    ReferrerPolicySnapshot {
        page_url: page_url.to_owned(),
        page_is_https: page_url.starts_with("https://"),
        page_is_localhost: is_localhost(page_url),
        policy_value,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PolicyClass {
    Safe,
    Permissive,
    Invalid,
}

/// Parse a policy header. Multi-value form: comma-separated, the
/// LAST recognised token wins (W3C). Unknown tokens are skipped;
/// if no recognised token is present, the value is invalid.
fn classify_policy(value: &str) -> PolicyClass {
    let tokens: Vec<String> = value
        .split(',')
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return PolicyClass::Invalid;
    }
    for t in tokens.iter().rev() {
        if !VALID_TOKENS.iter().any(|v| *v == t) {
            continue;
        }
        return if PERMISSIVE_TOKENS.iter().any(|p| *p == t) {
            PolicyClass::Permissive
        } else {
            PolicyClass::Safe
        };
    }
    PolicyClass::Invalid
}

/// Run the detector on a snapshot. Returns zero findings on the
/// http / localhost exemptions.
pub fn detect_referrer_policy_issues(snap: &ReferrerPolicySnapshot) -> Vec<AxisFinding> {
    if !snap.page_is_https {
        return Vec::new();
    }
    if snap.page_is_localhost {
        return Vec::new();
    }
    let value = snap.policy_value.trim();
    if value.is_empty() {
        return vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "referrer-policy.missing".into(),
            detail: format!(
                "No Referrer-Policy response header. Modern browsers fall back to \
                 'strict-origin-when-cross-origin' (safe) but older browsers and \
                 embedded views may leak the full URL + query string to every \
                 third-party fetch (analytics, fonts, CDNs). \
                 Set 'Referrer-Policy: strict-origin-when-cross-origin' explicitly. \
                 (pageUrl={url})",
                url = snap.page_url
            ),
        }];
    }
    match classify_policy(value) {
        PolicyClass::Permissive => vec![AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "referrer-policy.permissive".into(),
            detail: format!(
                "Referrer-Policy is '{value}' — explicitly LESS safe than the modern \
                 browser default. Every cross-origin fetch may include the page's \
                 full URL + query string. Switch to 'strict-origin-when-cross-origin' \
                 or 'no-referrer'."
            ),
        }],
        PolicyClass::Invalid => vec![AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "referrer-policy.invalid".into(),
            detail: format!(
                "Referrer-Policy value '{value}' contains no recognised W3C token. \
                 Browsers fall back to their default; the operator's INTENT is lost. \
                 Use one of: no-referrer, no-referrer-when-downgrade, origin, \
                 origin-when-cross-origin, same-origin, strict-origin, \
                 strict-origin-when-cross-origin, unsafe-url."
            ),
        }],
        PolicyClass::Safe => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, value: &str) -> ReferrerPolicySnapshot {
        if value.is_empty() {
            build_referrer_policy_snapshot(url, Vec::<(&str, &str)>::new())
        } else {
            build_referrer_policy_snapshot(url, [("referrer-policy", value)])
        }
    }

    #[test]
    fn http_page_exempt() {
        assert!(detect_referrer_policy_issues(&snap("http://example.com/", "")).is_empty());
    }

    #[test]
    fn localhost_exempt() {
        assert!(detect_referrer_policy_issues(&snap("https://localhost/", "")).is_empty());
        assert!(detect_referrer_policy_issues(&snap("https://127.0.0.1/", "")).is_empty());
        assert!(detect_referrer_policy_issues(&snap("https://app.localhost/", "")).is_empty());
    }

    #[test]
    fn missing_header_warn() {
        let f = detect_referrer_policy_issues(&snap("https://example.com/", ""));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.missing");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn strict_origin_when_cross_origin_no_finding() {
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "strict-origin-when-cross-origin",
        ));
        assert!(f.is_empty());
    }

    #[test]
    fn no_referrer_no_finding() {
        let f = detect_referrer_policy_issues(&snap("https://example.com/", "no-referrer"));
        assert!(f.is_empty());
    }

    #[test]
    fn unsafe_url_strict() {
        let f = detect_referrer_policy_issues(&snap("https://example.com/", "unsafe-url"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.permissive");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn no_referrer_when_downgrade_strict() {
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "no-referrer-when-downgrade",
        ));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.permissive");
    }

    #[test]
    fn origin_when_cross_origin_strict() {
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "origin-when-cross-origin",
        ));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.permissive");
    }

    #[test]
    fn unrecognised_token_invalid_warn() {
        let f = detect_referrer_policy_issues(&snap("https://example.com/", "BANANA"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.invalid");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn multi_token_last_wins_safe() {
        // unsafe-url, then strict-origin → strict-origin (safe) wins.
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "unsafe-url, strict-origin",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn multi_token_last_wins_permissive() {
        // strict-origin, then unsafe-url → permissive wins.
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "strict-origin, unsafe-url",
        ));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.permissive");
    }

    #[test]
    fn multi_token_skips_unknown_picks_known() {
        // Unknown last, then a real safe token — last recognised wins.
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "no-referrer, BANANA",
        ));
        // BANANA is unknown → walk left → no-referrer is safe.
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn case_insensitive_tokens() {
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "STRICT-ORIGIN-WHEN-CROSS-ORIGIN",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn whitespace_around_tokens_trimmed() {
        let f = detect_referrer_policy_issues(&snap(
            "https://example.com/",
            "  no-referrer  ",
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build_referrer_policy_snapshot(
            "https://example.com/",
            [("Referrer-Policy", "no-referrer")],
        );
        assert!(detect_referrer_policy_issues(&s).is_empty());
    }

    #[test]
    fn all_unknown_tokens_is_invalid() {
        let f = detect_referrer_policy_issues(&snap("https://example.com/", "FOO, BAR, BAZ"));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "referrer-policy.invalid");
    }
}
