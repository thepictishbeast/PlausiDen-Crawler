//! `cookie_security` — Set-Cookie attribute audit.
//!
//! Mirror of `src/cookieSecurity.ts`. Findings:
//!
//!   * `cookie.no-secure`               strict (https only)
//!   * `cookie.no-samesite`             warn
//!   * `cookie.session-no-httponly`     warn (heuristic on name)
//!   * `cookie.samesite-none-no-secure` strict (browsers REJECT)
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One parsed Set-Cookie line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedCookie {
    /// Cookie name (left side of the first `=`).
    pub name: String,
    /// `SameSite` attribute value. Empty if absent or bare.
    pub same_site: String,
    /// Whether the `Secure` attribute was set.
    pub has_secure: bool,
    /// Whether the `HttpOnly` attribute was set.
    pub has_http_only: bool,
    /// Raw Set-Cookie line, truncated to 200 chars.
    pub raw: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CookieSecuritySnapshot {
    /// Page URL.
    pub page_url: String,
    /// True iff the page was loaded over https.
    pub page_is_https: bool,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Parsed cookies from the top-level navigation response.
    pub cookies: Vec<CapturedCookie>,
}

/// Parse one Set-Cookie line. Returns `None` on malformed input
/// (no `=` before the first attribute separator, or empty name).
///
/// Browsers tolerate weird whitespace / casing; we normalise
/// attribute names to lowercase.
fn parse_set_cookie(raw: &str) -> Option<CapturedCookie> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split(';');
    let head = parts.next().unwrap_or("").trim();
    let eq = head.find('=')?;
    if eq == 0 {
        return None;
    }
    let name = head[..eq].trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let mut same_site = String::new();
    let mut has_secure = false;
    let mut has_http_only = false;
    for raw_attr in parts {
        let a = raw_attr.trim();
        if a.is_empty() {
            continue;
        }
        let lower = a.to_ascii_lowercase();
        if lower == "secure" {
            has_secure = true;
        } else if lower == "httponly" {
            has_http_only = true;
        } else if let Some(rest) = lower.strip_prefix("samesite=") {
            // Preserve original case for evidence display.
            let original_prefix_len = "samesite=".len();
            same_site = a[original_prefix_len..].trim().to_owned();
            // If empty after prefix strip (e.g. "SameSite="), keep empty.
            let _ = rest;
        } else if lower == "samesite" {
            // Bare `SameSite` with no value — same as absent.
            same_site.clear();
        }
    }
    let truncated_len = trimmed.len().min(200);
    let raw_truncated: String = trimmed.chars().take(truncated_len).collect();
    Some(CapturedCookie {
        name,
        same_site,
        has_secure,
        has_http_only,
        raw: raw_truncated,
    })
}

/// Extract every Set-Cookie value from a headers map. The
/// upstream wire form joins multiple cookies on `\n`; we split
/// there to recover the per-cookie list.
fn extract_set_cookie_lines<'a, I, K, V>(headers: I) -> Vec<String>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    for (k, v) in headers {
        if k.as_ref().eq_ignore_ascii_case("set-cookie") {
            return v
                .as_ref()
                .split('\n')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Build a snapshot from a captured response-headers map.
pub fn build_cookie_security_snapshot(
    page_url: &str,
    headers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> CookieSecuritySnapshot {
    let page_is_https = page_url.starts_with("https://");
    let page_is_localhost = is_localhost(page_url);
    let lines = extract_set_cookie_lines(headers);
    let cookies: Vec<CapturedCookie> = lines
        .into_iter()
        .filter_map(|l| parse_set_cookie(&l))
        .collect();
    CookieSecuritySnapshot {
        page_url: page_url.to_owned(),
        page_is_https,
        page_is_localhost,
        cookies,
    }
}

/// Heuristic: cookie name matches a session-like pattern.
/// False positives are fine (warn-only); false negatives mean
/// we miss real defects, so be liberal.
fn looks_like_session_cookie(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["sess", "sid", "auth", "token", "jwt", "bearer"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Render the first 5 examples as a `; `-joined string.
fn render_examples(cookies: &[&CapturedCookie]) -> String {
    cookies
        .iter()
        .take(5)
        .map(|c| format!("'{}' raw='{}'", c.name, c.raw))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_cookie_security_issues(snap: &CookieSecuritySnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    if snap.cookies.is_empty() {
        return Vec::new();
    }

    let mut no_secure: Vec<&CapturedCookie> = Vec::new();
    let mut no_same_site: Vec<&CapturedCookie> = Vec::new();
    let mut session_no_http_only: Vec<&CapturedCookie> = Vec::new();
    let mut same_site_none_no_secure: Vec<&CapturedCookie> = Vec::new();

    for c in &snap.cookies {
        if c.same_site.eq_ignore_ascii_case("none") && !c.has_secure {
            same_site_none_no_secure.push(c);
        }
        if snap.page_is_https && !c.has_secure {
            no_secure.push(c);
        }
        if c.same_site.is_empty() {
            no_same_site.push(c);
        }
        if looks_like_session_cookie(&c.name) && !c.has_http_only {
            session_no_http_only.push(c);
        }
    }

    let mut out = Vec::new();

    if !no_secure.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "cookie.no-secure".into(),
            detail: format!(
                "{} cookie(s) set without 'Secure' attribute on this https page. The cookie can be sent over http if the user is briefly downgraded (MITM, network rewrite, mixed-content). Add '; Secure' to every Set-Cookie. Examples: {}",
                no_secure.len(),
                render_examples(&no_secure)
            ),
        });
    }
    if !same_site_none_no_secure.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "cookie.samesite-none-no-secure".into(),
            detail: format!(
                "{} cookie(s) set with 'SameSite=None' but no 'Secure' attribute. Browsers REJECT this combination — the cookie isn't stored at all. Either add Secure (and only set on https) or change to 'SameSite=Lax'. Examples: {}",
                same_site_none_no_secure.len(),
                render_examples(&same_site_none_no_secure)
            ),
        });
    }
    if !no_same_site.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cookie.no-samesite".into(),
            detail: format!(
                "{} cookie(s) set without 'SameSite' attribute. Modern browsers default 'Lax' (safe) but older clients leave it unrestricted, exposing the user to CSRF. Set 'SameSite=Strict' (or 'Lax' if cross-site GETs are needed). Examples: {}",
                no_same_site.len(),
                render_examples(&no_same_site)
            ),
        });
    }
    if !session_no_http_only.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cookie.session-no-httponly".into(),
            detail: format!(
                "{} session-looking cookie(s) (name matches sess/sid/auth/token/jwt/bearer) lack 'HttpOnly'. JS — including injected XSS — can read these via document.cookie and exfiltrate. Add '; HttpOnly'. Examples: {}",
                session_no_http_only.len(),
                render_examples(&session_no_http_only)
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(headers: &[(&str, &str)], url: &str) -> CookieSecuritySnapshot {
        build_cookie_security_snapshot(url, headers.iter().map(|(k, v)| (*k, *v)))
    }

    #[test]
    fn localhost_skipped() {
        let s = snap(&[("Set-Cookie", "sid=abc")], "http://localhost:8000/");
        assert!(detect_cookie_security_issues(&s).is_empty());
    }

    #[test]
    fn no_cookies_clean() {
        let s = snap(&[], "https://example.com/");
        assert!(detect_cookie_security_issues(&s).is_empty());
    }

    #[test]
    fn https_no_secure_is_strict() {
        let s = snap(
            &[("Set-Cookie", "user=joe; SameSite=Lax")],
            "https://example.com/",
        );
        let f = detect_cookie_security_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "cookie.no-secure" && x.severity == AxisSeverity::Strict));
    }

    #[test]
    fn http_no_secure_doesnt_fire() {
        // http page → Secure can't apply → no-secure finding suppressed
        let s = snap(
            &[("Set-Cookie", "user=joe; SameSite=Lax")],
            "http://example.com/",
        );
        let f = detect_cookie_security_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "cookie.no-secure"));
    }

    #[test]
    fn no_samesite_warns() {
        let s = snap(
            &[("Set-Cookie", "user=joe; Secure")],
            "https://example.com/",
        );
        let f = detect_cookie_security_issues(&s);
        assert!(f.iter().any(|x| x.kind == "cookie.no-samesite"));
    }

    #[test]
    fn samesite_none_without_secure_is_strict() {
        let s = snap(
            &[("Set-Cookie", "tracker=1; SameSite=None")],
            "https://example.com/",
        );
        let f = detect_cookie_security_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "cookie.samesite-none-no-secure"
                && x.severity == AxisSeverity::Strict));
    }

    #[test]
    fn session_cookie_without_httponly_warns() {
        let s = snap(
            &[("Set-Cookie", "sid=abc; Secure; SameSite=Lax")],
            "https://example.com/",
        );
        let f = detect_cookie_security_issues(&s);
        assert!(f.iter().any(|x| x.kind == "cookie.session-no-httponly"));
    }

    #[test]
    fn session_heuristic_matches_jwt_and_token() {
        for name in ["jwt", "auth_token", "MY_SID", "bearer-X"] {
            assert!(looks_like_session_cookie(name), "{name} should match");
        }
    }

    #[test]
    fn non_session_with_no_httponly_doesnt_warn_session() {
        let s = snap(
            &[("Set-Cookie", "prefs=dark; Secure; SameSite=Lax")],
            "https://example.com/",
        );
        let f = detect_cookie_security_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "cookie.session-no-httponly"));
    }

    #[test]
    fn multiple_cookies_join_via_newline_separator() {
        let raw = "a=1; Secure; SameSite=Lax\nsid=xyz; Secure; SameSite=Lax";
        let s = snap(&[("Set-Cookie", raw)], "https://example.com/");
        assert_eq!(s.cookies.len(), 2);
        // sid lacks HttpOnly → session warn
        let f = detect_cookie_security_issues(&s);
        assert!(f.iter().any(|x| x.kind == "cookie.session-no-httponly"));
    }

    #[test]
    fn malformed_set_cookie_is_dropped() {
        // No `=` in head → parser returns None → not in snapshot.
        let s = snap(&[("Set-Cookie", "garbage")], "https://example.com/");
        assert!(s.cookies.is_empty());
    }

    #[test]
    fn case_insensitive_set_cookie_header_lookup() {
        let s = snap(
            &[("SET-COOKIE", "a=1; Secure; SameSite=Lax")],
            "https://example.com/",
        );
        assert_eq!(s.cookies.len(), 1);
    }
}
