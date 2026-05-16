//! `cache_control` — Cache-Control directive hygiene detector.
//!
//! Mirror of `src/cacheControl.ts`. Catches Web Cache Deception
//! (Omer Gil, 2017) + sloppy directive hygiene. Findings:
//!
//!   * `cache-control.missing`                 warn
//!   * `cache-control.public-with-cookie`      strict
//!   * `cache-control.no-private-with-cookie`  warn
//!   * `cache-control.invalid`                 warn
//!   * `cache-control.unrealistic-maxage`      warn  (>1y)
//!   * `cache-control.contradictory`           warn
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const ONE_YEAR_SECONDS: u64 = 31_536_000;

/// Parsed form of the Cache-Control header value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCacheControl {
    /// All directive names (lowercased), in declaration order.
    pub directives: Vec<String>,
    /// Lookup of directive name → value (for value-bearing forms).
    pub values: HashMap<String, String>,
    /// True iff the header was present but no directive parsed.
    pub unparseable: bool,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CacheControlSnapshot {
    /// Page URL (used for localhost exemption).
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw header value, `None` if absent.
    pub raw: Option<String>,
    /// Parsed form. Default-empty if `raw` is `None`.
    pub parsed: ParsedCacheControl,
    /// True iff the response also carries Set-Cookie.
    pub has_set_cookie: bool,
}

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

fn parse_cache_control(raw: &str) -> ParsedCacheControl {
    let mut directives = Vec::new();
    let mut values: HashMap<String, String> = HashMap::new();
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (name, value) = match trimmed.find('=') {
            None => (trimmed.to_ascii_lowercase(), String::new()),
            Some(eq) => {
                let name = trimmed[..eq].trim().to_ascii_lowercase();
                let mut value = trimmed[eq + 1..].trim().to_owned();
                if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
                    || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
                {
                    value = value[1..value.len() - 1].to_owned();
                }
                (name, value)
            }
        };
        if name.is_empty() {
            continue;
        }
        directives.push(name.clone());
        if !value.is_empty() {
            values.insert(name, value);
        }
    }
    let unparseable = !raw.trim().is_empty() && directives.is_empty();
    ParsedCacheControl { directives, values, unparseable }
}

/// Build a snapshot from a page URL + a header map. The map must
/// expose every relevant header — Cache-Control + Set-Cookie at
/// minimum. Headers are looked up case-insensitively.
pub fn build_cache_control_snapshot<I, K, V>(
    page_url: &str,
    headers: I,
) -> CacheControlSnapshot
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    // Collect headers once because the iterator is consumed.
    let collected: Vec<(String, String)> = headers
        .into_iter()
        .map(|(k, v)| (k.as_ref().to_owned(), v.as_ref().to_owned()))
        .collect();
    let raw = lookup_header(collected.iter().map(|(k, v)| (k.as_str(), v.as_str())), "cache-control");
    let parsed = match &raw {
        None => ParsedCacheControl::default(),
        Some(r) => parse_cache_control(r),
    };
    let has_set_cookie = lookup_header(
        collected.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        "set-cookie",
    )
    .is_some();
    CacheControlSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost: is_localhost(page_url),
        raw,
        parsed,
        has_set_cookie,
    }
}

/// Run the detector. Returns zero findings on localhost.
pub fn detect_cache_control_issues(snap: &CacheControlSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let mut out = Vec::new();

    let Some(raw) = snap.raw.as_deref() else {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cache-control.missing".into(),
            detail: format!(
                "No Cache-Control header. Browsers and intermediate caches fall back to \
                 RFC 7234 heuristic freshness (typically 10% of Last-Modified age) — \
                 unpredictable across implementations. Be explicit: 'Cache-Control: \
                 no-store' for sensitive pages, 'private, max-age=<seconds>' for \
                 personalised but cacheable, 'public, max-age=<seconds>, immutable' \
                 for static assets. (hasSetCookie={})",
                snap.has_set_cookie
            ),
        });
        return out;
    };

    if snap.parsed.unparseable {
        let preview = &raw[..raw.len().min(200)];
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cache-control.invalid".into(),
            detail: format!(
                "Cache-Control header is set but couldn't be parsed into any directive. \
                 Browsers and proxies fall back to no-Cache-Control semantics, silently \
                 disabling the operator's intent. Header value: '{preview}'."
            ),
        });
        return out;
    }

    let dset: std::collections::HashSet<&str> =
        snap.parsed.directives.iter().map(|s| s.as_str()).collect();

    if snap.has_set_cookie && dset.contains("public") {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "cache-control.public-with-cookie".into(),
            detail: format!(
                "Response sets a Set-Cookie header AND Cache-Control includes 'public'. \
                 Web Cache Deception risk: an intermediate cache (CDN, reverse proxy, \
                 kiosk browser) can store the response keyed by URL, then serve it WITH \
                 THE ORIGINAL Set-Cookie to the next visitor. Replace 'public' with \
                 'no-store' (sensitive) or 'private' (personalised, browser-only). \
                 (directives={:?})",
                snap.parsed.directives
            ),
        });
    } else if snap.has_set_cookie
        && !dset.contains("no-store")
        && !dset.contains("private")
    {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cache-control.no-private-with-cookie".into(),
            detail: format!(
                "Response sets a Set-Cookie header but Cache-Control omits both \
                 'no-store' and 'private'. Sufficiently permissive proxies (some CDNs, \
                 corporate caches) may store and serve the response cross-user. \
                 Add 'private' (browser-only caching) or 'no-store' (no caching at all). \
                 (directives={:?})",
                snap.parsed.directives
            ),
        });
    }

    if let Some(max_age_str) = snap.parsed.values.get("max-age") {
        match max_age_str.parse::<i64>() {
            Err(_) => {
                out.push(AxisFinding {
                    severity: AxisSeverity::Warn,
                    kind: "cache-control.invalid".into(),
                    detail: format!(
                        "Cache-Control max-age value '{max_age_str}' is not a \
                         non-negative integer. Browsers / proxies typically ignore."
                    ),
                });
            }
            Ok(n) if n < 0 => {
                out.push(AxisFinding {
                    severity: AxisSeverity::Warn,
                    kind: "cache-control.invalid".into(),
                    detail: format!(
                        "Cache-Control max-age value '{max_age_str}' is negative. \
                         Browsers / proxies typically ignore."
                    ),
                });
            }
            Ok(n) if (n as u64) > ONE_YEAR_SECONDS => {
                out.push(AxisFinding {
                    severity: AxisSeverity::Warn,
                    kind: "cache-control.unrealistic-maxage".into(),
                    detail: format!(
                        "Cache-Control max-age={n} exceeds 1 year (31536000s). \
                         RFC 7234 §5.2.1.1: caches SHOULD treat values greater than \
                         1 year as 1 year — anything larger is dead code at best."
                    ),
                });
            }
            _ => {}
        }
    }

    let mut contradictions: Vec<String> = Vec::new();
    if dset.contains("no-store")
        && (dset.contains("max-age") || dset.contains("s-maxage"))
    {
        contradictions
            .push("'no-store' + 'max-age': no-store wins, max-age is dead".into());
    }
    if dset.contains("public") && dset.contains("private") {
        contradictions.push(
            "'public' + 'private': spec ambiguous, most implementations honour 'private'"
                .into(),
        );
    }
    if dset.contains("no-cache") && dset.contains("immutable") {
        contradictions.push(
            "'no-cache' + 'immutable': cancel each other (revalidate-always vs skip-revalidate)"
                .into(),
        );
    }
    if dset.contains("no-store") && dset.contains("immutable") {
        contradictions.push(
            "'no-store' + 'immutable': no-store forbids any cache, immutable assumes one"
                .into(),
        );
    }
    if !contradictions.is_empty() {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "cache-control.contradictory".into(),
            detail: format!(
                "Cache-Control contains contradictory directives. {}. Pick one intent \
                 and stick with it. (directives={:?})",
                contradictions.join(". "),
                snap.parsed.directives
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(url: &str, cc: Option<&str>, set_cookie: bool) -> CacheControlSnapshot {
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if let Some(v) = cc {
            headers.push(("cache-control", v));
        }
        if set_cookie {
            headers.push(("set-cookie", "session=abc; Path=/"));
        }
        build_cache_control_snapshot(url, headers)
    }

    #[test]
    fn localhost_exempt() {
        assert!(detect_cache_control_issues(&snap("https://localhost/", None, true)).is_empty());
    }

    #[test]
    fn missing_header_warn() {
        let f = detect_cache_control_issues(&snap("https://example.com/", None, false));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "cache-control.missing");
    }

    #[test]
    fn public_with_set_cookie_strict() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, max-age=600"),
            true,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.public-with-cookie"
            && x.severity == AxisSeverity::Strict));
    }

    #[test]
    fn cookie_without_private_or_no_store_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("max-age=600"),
            true,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.no-private-with-cookie"));
    }

    #[test]
    fn private_with_set_cookie_clean() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("private, max-age=0"),
            true,
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn no_store_with_set_cookie_clean() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("no-store"),
            true,
        ));
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn unrealistic_maxage_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, max-age=99999999"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.unrealistic-maxage"));
    }

    #[test]
    fn invalid_maxage_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, max-age=banana"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.invalid"));
    }

    #[test]
    fn negative_maxage_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, max-age=-1"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.invalid"));
    }

    #[test]
    fn contradictory_no_store_max_age_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("no-store, max-age=600"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.contradictory"));
    }

    #[test]
    fn contradictory_public_private_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, private"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.contradictory"));
    }

    #[test]
    fn contradictory_no_cache_immutable_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("no-cache, immutable, max-age=600"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.contradictory"));
    }

    #[test]
    fn contradictory_no_store_immutable_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("no-store, immutable"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.contradictory"));
    }

    #[test]
    fn unparseable_value_invalid_warn() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some(",,,,,"),
            false,
        ));
        assert!(f.iter().any(|x| x.kind == "cache-control.invalid"));
    }

    #[test]
    fn case_insensitive_directives() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("PUBLIC, MAX-AGE=600"),
            true,
        ));
        // Public + cookie still triggers strict — case-insensitive parse.
        assert!(f.iter().any(|x| x.kind == "cache-control.public-with-cookie"));
    }

    #[test]
    fn quoted_max_age_accepted() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, max-age=\"3600\""),
            false,
        ));
        // No findings — value parses, no-cookie scenario, sensible TTL.
        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build_cache_control_snapshot(
            "https://example.com/",
            [("Cache-Control", "private, max-age=600")],
        );
        assert!(detect_cache_control_issues(&s).is_empty());
    }

    #[test]
    fn one_year_boundary_is_acceptable() {
        let f = detect_cache_control_issues(&snap(
            "https://example.com/",
            Some("public, max-age=31536000"),
            false,
        ));
        assert!(f.is_empty(), "1y exact is OK; got {f:?}");
    }
}
