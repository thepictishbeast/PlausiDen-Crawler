//! `x_frame_options` — clickjacking-defence detector.
//!
//! Mirror of `src/xFrameOptions.ts`. Findings:
//!
//!   * `frame-options.missing`   strict
//!   * `frame-options.allowall`  warn
//!   * `frame-options.invalid`   warn
//!
//! Either `X-Frame-Options` (RFC 7034) or
//! `Content-Security-Policy: frame-ancestors …` provides
//! clickjacking protection. The detector flags pages with
//! NEITHER, with an open wildcard, or with an unrecognised XFO
//! value.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O, no `unwrap`/`expect` in non-test code.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state the detector consumes.
///
/// BUG ASSUMPTION: caller is responsible for passing the
/// top-level navigation response headers, NOT subresource
/// headers. XFO/CSP only matter on the document response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct XFrameOptionsSnapshot {
    /// URL of the page (for evidence + http/localhost gates).
    pub page_url: String,
    /// True iff the page itself was loaded over https.
    pub page_is_https: bool,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw `X-Frame-Options` header value, empty string if absent.
    pub xfo_value: String,
    /// Raw `Content-Security-Policy` header value, empty if absent.
    pub csp_value: String,
}

use crate::url_helpers::is_localhost;

/// Build a snapshot from a page URL + a header map. Header keys
/// are looked up case-insensitively (per RFC 9110).
pub fn build_x_frame_options_snapshot<I, K, V>(
    page_url: &str,
    headers: I,
) -> XFrameOptionsSnapshot
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut xfo = String::new();
    let mut csp = String::new();
    for (k, v) in headers {
        let lk = k.as_ref().to_ascii_lowercase();
        match lk.as_str() {
            "x-frame-options" => xfo = v.as_ref().to_owned(),
            "content-security-policy" => csp = v.as_ref().to_owned(),
            _ => {}
        }
    }
    XFrameOptionsSnapshot {
        page_url: page_url.to_owned(),
        page_is_https: page_url.starts_with("https://"),
        page_is_localhost: is_localhost(page_url),
        xfo_value: xfo,
        csp_value: csp,
    }
}

/// Pull the `frame-ancestors` directive value from a CSP header.
/// Returns `None` when absent. CSP directives are
/// semicolon-separated; values within a directive are
/// whitespace-separated source-list tokens.
fn parse_frame_ancestors(csp: &str) -> Option<String> {
    for part in csp.split(';') {
        let t = part.trim();
        // Match "frame-ancestors <whitespace> <value>" case-insensitively.
        let lower = t.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("frame-ancestors") {
            let after = rest.trim_start();
            if after.is_empty() || (rest.len() == after.len()) {
                continue; // need at least one whitespace separator
            }
            // Use the original casing for the returned value (it
            // may include URL hosts whose case is preserved).
            let original_after = &t[t.len() - after.len()..];
            return Some(original_after.trim().to_owned());
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
enum XfoClass {
    Deny,
    SameOrigin,
    AllowFrom,
    Invalid,
}

/// Validate an X-Frame-Options value against RFC 7034.
fn classify_xfo(value: &str) -> XfoClass {
    let v = value.trim().to_ascii_lowercase();
    if v == "deny" {
        XfoClass::Deny
    } else if v == "sameorigin" {
        XfoClass::SameOrigin
    } else if v.starts_with("allow-from") {
        // Must have whitespace + at least one non-ws char after.
        let after = v.trim_start_matches("allow-from");
        if after.starts_with(|c: char| c.is_ascii_whitespace())
            && after.split_whitespace().next().is_some()
        {
            XfoClass::AllowFrom
        } else {
            XfoClass::Invalid
        }
    } else {
        XfoClass::Invalid
    }
}

/// Run the detector on a snapshot. Returns zero findings when
/// the page is exempt (http or localhost).
pub fn detect_x_frame_options_issues(snap: &XFrameOptionsSnapshot) -> Vec<AxisFinding> {
    if !snap.page_is_https {
        return Vec::new();
    }
    if snap.page_is_localhost {
        return Vec::new();
    }

    let mut out = Vec::new();
    let xfo = snap.xfo_value.trim();
    let csp = snap.csp_value.trim();
    let frame_ancestors = if csp.is_empty() {
        None
    } else {
        parse_frame_ancestors(csp)
    };

    // CSP frame-ancestors supersedes XFO. If present and not the
    // open wildcard, we're protected.
    if let Some(fa) = frame_ancestors.as_deref() {
        if fa != "*" {
            return out;
        }
        // Wildcard: warn, then exit.
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "frame-options.allowall".into(),
            detail: format!(
                "Content-Security-Policy frame-ancestors '*' — page is iframable by any origin. \
                 If this is an embeddable widget, fine; if it's the main app, it's a clickjacking risk. \
                 Consider 'frame-ancestors 'none'' or a specific origin allowlist. (cspValue={csp})"
            ),
        });
        return out;
    }

    if xfo.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "frame-options.missing".into(),
            detail: format!(
                "Page has no X-Frame-Options header AND no Content-Security-Policy frame-ancestors directive. \
                 Any origin can iframe this page → clickjacking attacks possible. \
                 Add 'X-Frame-Options: SAMEORIGIN' OR 'Content-Security-Policy: frame-ancestors 'self''. \
                 (pageUrl={url})",
                url = snap.page_url
            ),
        });
        return out;
    }

    if classify_xfo(xfo) == XfoClass::Invalid {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "frame-options.invalid".into(),
            detail: format!(
                "X-Frame-Options has an unrecognised value '{xfo}'. \
                 Browsers treat invalid values as no-protection. \
                 Use DENY (no framing at all) or SAMEORIGIN (only your own origin can iframe)."
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, xfo: &str, csp: &str) -> XFrameOptionsSnapshot {
        build_x_frame_options_snapshot(
            url,
            [("x-frame-options", xfo), ("content-security-policy", csp)],
        )
    }

    #[test]
    fn http_page_is_exempt() {
        let s = snap("http://example.com/", "", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn localhost_is_exempt() {
        let s = snap("http://localhost:8080/", "", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
        let s = snap("https://localhost/", "", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
        let s = snap("https://127.0.0.1/", "", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn dot_localhost_subdomain_is_exempt() {
        let s = snap("https://app.localhost/", "", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn missing_both_strict() {
        let s = snap("https://example.com/", "", "");
        let f = detect_x_frame_options_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "frame-options.missing");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn xfo_deny_alone_is_protection() {
        let s = snap("https://example.com/", "DENY", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn xfo_sameorigin_alone_is_protection() {
        let s = snap("https://example.com/", "SAMEORIGIN", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn xfo_lowercase_normalized() {
        let s = snap("https://example.com/", "sameorigin", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn xfo_invalid_value_warn() {
        let s = snap("https://example.com/", "BANANA", "");
        let f = detect_x_frame_options_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "frame-options.invalid");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn xfo_allow_from_with_uri_is_valid() {
        let s = snap("https://example.com/", "ALLOW-FROM https://parent.example", "");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn xfo_allow_from_no_uri_is_invalid() {
        let s = snap("https://example.com/", "ALLOW-FROM", "");
        let f = detect_x_frame_options_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "frame-options.invalid");
    }

    #[test]
    fn csp_frame_ancestors_self_supersedes_missing_xfo() {
        let s = snap("https://example.com/", "", "frame-ancestors 'self'");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn csp_frame_ancestors_none_supersedes_missing_xfo() {
        let s = snap("https://example.com/", "", "frame-ancestors 'none'");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn csp_frame_ancestors_wildcard_warn() {
        let s = snap("https://example.com/", "", "frame-ancestors *");
        let f = detect_x_frame_options_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "frame-options.allowall");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn csp_frame_ancestors_in_middle_directive() {
        let s = snap(
            "https://example.com/",
            "",
            "default-src 'self'; frame-ancestors 'self'; script-src 'self'",
        );
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn csp_with_other_directives_no_frame_ancestors_uses_xfo() {
        // No frame-ancestors → fall through to XFO check (which is
        // missing) → strict.
        let s = snap("https://example.com/", "", "default-src 'self'");
        let f = detect_x_frame_options_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "frame-options.missing");
    }

    #[test]
    fn csp_frame_ancestors_case_insensitive_directive_name() {
        let s = snap("https://example.com/", "", "FRAME-ANCESTORS 'self'");
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let s = build_x_frame_options_snapshot(
            "https://example.com/",
            [("X-Frame-Options", "DENY")],
        );
        assert_eq!(detect_x_frame_options_issues(&s).len(), 0);
    }

    #[test]
    fn wildcard_csp_returns_only_warn_not_strict() {
        let s = snap("https://example.com/", "", "frame-ancestors *");
        let f = detect_x_frame_options_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "frame-options.missing"));
    }

    #[test]
    fn detail_text_includes_evidence_url() {
        let s = snap("https://acme.example.com/foo", "", "");
        let f = detect_x_frame_options_issues(&s);
        assert!(f[0].detail.contains("acme.example.com"));
    }

    #[test]
    fn malformed_url_is_treated_as_not_localhost() {
        // url::Url::parse will fail on "https://" alone; that's
        // fine — the host check returns false → not localhost.
        // Page-https check still triggers via str::starts_with.
        let s = snap("https://", "", "");
        let f = detect_x_frame_options_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "frame-options.missing");
    }
}
