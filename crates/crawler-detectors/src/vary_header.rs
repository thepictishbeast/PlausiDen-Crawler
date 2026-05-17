//! `vary_header` — Vary header correctness audit. T76 port of `src/varyHeader.ts`.
//!
//! Vary tells caches HOW TO KEY cached responses. If a response varies
//! based on a request header (Cookie, Authorization, Accept-Language)
//! but the response doesn't say `Vary: <those headers>`, intermediate
//! caches store the response keyed by URL alone and serve it to ANY
//! visitor — including ones whose request headers would have produced
//! a different response. Sister detector to `cache_control`.
//!
//! Findings:
//!
//!   * `vary.invalid`                                       — warn
//!   * `vary.star`                                          — warn
//!   * `vary.duplicate-tokens`                              — warn
//!   * `vary.no-cookie-with-set-cookie-and-cacheable`       — warn
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VarySnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Raw Vary header value, or `None`.
    pub raw: Option<String>,
    /// Parsed token list (lowercased). Empty if no header or unparseable.
    pub tokens: Vec<String>,
    /// True iff Vary was present but no valid token parsed.
    pub unparseable: bool,
    /// Same token appears more than once (case-insensitive).
    pub has_duplicates: bool,
    /// True iff response carries Set-Cookie.
    pub has_set_cookie: bool,
    /// Cache-Control directive set, lowercased. Used to short-circuit
    /// the cookie-cacheability finding when response is already
    /// uncacheable by `no-store` / `private`.
    pub cache_control_directives: Vec<String>,
}

fn header_lookup<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// RFC 7230 token grammar: `!#$%&'*+-.^_\`|~ DIGIT ALPHA`, plus
/// the wildcard `*`.
fn is_valid_vary_token(t: &str) -> bool {
    if t == "*" {
        return true;
    }
    if t.is_empty() {
        return false;
    }
    t.bytes().all(|b| match b {
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_'
        | b'`' | b'|' | b'~' => true,
        _ => b.is_ascii_alphanumeric(),
    })
}

fn parse_cache_control_directives(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else { return Vec::new() };
    raw.split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return None;
            }
            let name = match trimmed.find('=') {
                Some(eq) => &trimmed[..eq],
                None => trimmed,
            };
            let name = name.trim().to_ascii_lowercase();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        })
        .collect()
}

/// Build a snapshot from a captured headers map.
pub fn build_vary_snapshot(page_url: &str, headers: &BTreeMap<String, String>) -> VarySnapshot {
    let page_is_localhost = is_localhost(page_url);
    let raw = header_lookup(headers, "vary").cloned();
    let has_set_cookie = header_lookup(headers, "set-cookie").is_some();
    let cache_control_directives =
        parse_cache_control_directives(header_lookup(headers, "cache-control").map(String::as_str));

    let Some(raw_str) = raw.clone() else {
        return VarySnapshot {
            page_url: page_url.to_owned(),
            page_is_localhost,
            raw: None,
            tokens: Vec::new(),
            unparseable: false,
            has_duplicates: false,
            has_set_cookie,
            cache_control_directives,
        };
    };

    let raw_tokens: Vec<String> = raw_str
        .split(',')
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();

    let mut tokens = Vec::new();
    let mut any_valid = false;
    for t in &raw_tokens {
        if is_valid_vary_token(t) {
            tokens.push(t.to_ascii_lowercase());
            any_valid = true;
        }
    }
    let unparseable = !raw_tokens.is_empty() && !any_valid;

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut has_duplicates = false;
    for t in &tokens {
        if !seen.insert(t.as_str()) {
            has_duplicates = true;
            break;
        }
    }

    VarySnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        raw: Some(raw_str),
        tokens,
        unparseable,
        has_duplicates,
        has_set_cookie,
        cache_control_directives,
    }
}

fn slice_for_log(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_vary_issues(snap: &VarySnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }
    let mut out = Vec::new();

    let cacheable = !snap
        .cache_control_directives
        .iter()
        .any(|d| d == "no-store" || d == "private");

    if let Some(raw) = snap.raw.as_deref() {
        let raw_snippet = slice_for_log(raw, 200);
        if snap.unparseable {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "vary.invalid".into(),
                detail: format!(
                    "Vary header is present but contains no valid token (RFC 7230 token grammar). Browsers / proxies typically ignore — silently disables the operator's intent. Header value: '{raw_snippet}'."
                ),
            });
        }
        if snap.tokens.iter().any(|t| t == "*") {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "vary.star".into(),
                detail: "Vary header contains '*' — explicitly tells caches the response is uncacheable because something not visible in the request headers determines it. Usually unintended; if you want the response uncached, set 'Cache-Control: no-store' instead — more intent-revealing and explicit.".into(),
            });
        }
        if snap.has_duplicates {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "vary.duplicate-tokens".into(),
                detail: format!(
                    "Vary header contains duplicate tokens (case-insensitive). Spec-conformant caches collapse, but parser errors in custom proxies have been reported. Header value: '{raw_snippet}'."
                ),
            });
        }
    }

    if snap.has_set_cookie && cacheable {
        let has_cookie =
            snap.tokens.iter().any(|t| t == "cookie") || snap.tokens.iter().any(|t| t == "*");
        if !has_cookie {
            let vary_clause = match snap.raw.as_deref() {
                None => "is absent".to_owned(),
                Some(raw) => format!(
                    "does not include 'cookie' or '*' (Vary: '{}')",
                    slice_for_log(raw, 200)
                ),
            };
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "vary.no-cookie-with-set-cookie-and-cacheable".into(),
                detail: format!(
                    "Response carries Set-Cookie AND Cache-Control allows shared caching (no 'private', no 'no-store') AND Vary {vary_clause}. A shared cache (CDN / corporate proxy / kiosk browser) may store the response keyed by URL alone and serve it back, leaking the Set-Cookie + personalised body to subsequent visitors. Add 'Vary: Cookie' (and ideally tighten Cache-Control to 'private' or 'no-store')."
                ),
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> &'static str {
        "https://example.com/"
    }

    fn build(headers: &[(&str, &str)]) -> VarySnapshot {
        let map: BTreeMap<String, String> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        build_vary_snapshot(page(), &map)
    }

    #[test]
    fn localhost_skipped() {
        let map = BTreeMap::new();
        let s = build_vary_snapshot("http://localhost/", &map);
        assert!(detect_vary_issues(&s).is_empty());
    }

    #[test]
    fn no_vary_no_set_cookie_clean() {
        let s = build(&[]);
        assert!(detect_vary_issues(&s).is_empty());
    }

    #[test]
    fn invalid_token_only_warns() {
        let s = build(&[("Vary", "$$$ invalid <<<")]);
        let f = detect_vary_issues(&s);
        assert!(f.iter().any(|x| x.kind == "vary.invalid"));
    }

    #[test]
    fn star_warns() {
        let s = build(&[("Vary", "*")]);
        let f = detect_vary_issues(&s);
        assert!(f.iter().any(|x| x.kind == "vary.star"));
    }

    #[test]
    fn duplicate_tokens_warn() {
        let s = build(&[("Vary", "Cookie, cookie")]);
        let f = detect_vary_issues(&s);
        assert!(f.iter().any(|x| x.kind == "vary.duplicate-tokens"));
    }

    #[test]
    fn set_cookie_cacheable_without_vary_cookie_warns() {
        let s = build(&[("Set-Cookie", "sid=xyz"), ("Cache-Control", "max-age=300")]);
        let f = detect_vary_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "vary.no-cookie-with-set-cookie-and-cacheable"));
    }

    #[test]
    fn set_cookie_with_vary_cookie_is_clean() {
        let s = build(&[
            ("Set-Cookie", "sid=xyz"),
            ("Cache-Control", "max-age=300"),
            ("Vary", "Cookie"),
        ]);
        let f = detect_vary_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "vary.no-cookie-with-set-cookie-and-cacheable"));
    }

    #[test]
    fn set_cookie_with_no_store_short_circuits() {
        let s = build(&[("Set-Cookie", "sid=xyz"), ("Cache-Control", "no-store")]);
        let f = detect_vary_issues(&s);
        // no-store means response isn't cacheable; no Vary finding needed
        assert!(!f
            .iter()
            .any(|x| x.kind == "vary.no-cookie-with-set-cookie-and-cacheable"));
    }

    #[test]
    fn set_cookie_with_private_short_circuits() {
        let s = build(&[
            ("Set-Cookie", "sid=xyz"),
            ("Cache-Control", "private, max-age=0"),
        ]);
        let f = detect_vary_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "vary.no-cookie-with-set-cookie-and-cacheable"));
    }

    #[test]
    fn vary_star_with_set_cookie_satisfies_cookie_key() {
        // '*' covers Cookie too — no Set-Cookie-leak finding.
        let s = build(&[
            ("Set-Cookie", "sid=xyz"),
            ("Cache-Control", "max-age=300"),
            ("Vary", "*"),
        ]);
        let f = detect_vary_issues(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "vary.no-cookie-with-set-cookie-and-cacheable"));
        // But vary.star still fires (the * itself is suspicious)
        assert!(f.iter().any(|x| x.kind == "vary.star"));
    }

    #[test]
    fn header_lookup_case_insensitive() {
        let s = build(&[("VARY", "Cookie")]);
        assert_eq!(s.tokens, vec!["cookie".to_string()]);
    }

    #[test]
    fn parses_multi_token_vary() {
        let s = build(&[("Vary", "Accept-Encoding, Cookie, Origin")]);
        assert_eq!(s.tokens.len(), 3);
        assert!(s.tokens.iter().any(|t| t == "cookie"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = build(&[
            ("Vary", "Cookie, Accept-Encoding"),
            ("Cache-Control", "max-age=60"),
        ]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: VarySnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.tokens, s.tokens);
    }

    #[test]
    fn empty_vary_string_emits_no_invalid_finding() {
        // An empty Vary string (after split+trim → 0 tokens) is
        // treated by the TS source as no tokens parsed AT ALL, so
        // `unparseable` becomes FALSE (because raw_tokens.is_empty()
        // → no candidates → !any_valid is true BUT the condition
        // requires raw_tokens NOT to be empty). Matches TS semantics.
        let s = build(&[("Vary", "")]);
        assert!(!s.unparseable);
        let f = detect_vary_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "vary.invalid"));
    }
}
