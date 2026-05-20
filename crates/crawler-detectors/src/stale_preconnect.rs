//! `stale_preconnect` — resource-hint waste detector.
//!
//! Findings:
//!
//!   * `stale_preconnect.unused-origin` warn — a
//!     `<link rel="preconnect|dns-prefetch|preload">` declared in
//!     `<head>` whose origin doesn't appear anywhere else in the
//!     page. Common in boilerplate left over from a font /
//!     analytics / CDN migration: the host page no longer loads
//!     anything from the origin, but the preconnect still
//!     opens a TCP+TLS handshake on every visit.
//!
//!   * `stale_preconnect.crossorigin-anonymous-missing` warn — a
//!     `<link rel="preconnect" href="https://...">` cross-origin
//!     hint without `crossorigin="anonymous"`. Browsers ignore
//!     the preconnect for fetch() / font requests that DO want
//!     anonymous credentials, so the hint is silently wasted.
//!
//! warn-only — these are perf cleanups, not breakage.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — collects every preconnect/dns-prefetch/preload
/// link in <head> and the set of origins actually referenced
/// elsewhere on the page.
pub const STALE_PRECONNECT_JS: &str = r##"(() => {
    const HINT_RELS = new Set(['preconnect', 'dns-prefetch', 'preload', 'modulepreload']);
    const hints = [];
    const links = document.querySelectorAll('head link[rel]');
    for (let i = 0; i < links.length; i++) {
        const rel = (links[i].getAttribute('rel') || '').toLowerCase().trim();
        const tokens = rel.split(/\s+/).filter(Boolean);
        if (!tokens.some(t => HINT_RELS.has(t))) continue;
        const href = links[i].getAttribute('href') || '';
        if (!href) continue;
        hints.push({
            rel,
            href,
            as_attr: links[i].getAttribute('as') || '',
            crossorigin: links[i].getAttribute('crossorigin') || '',
        });
    }

    // Used-origin set: every same-page element that references
    // an external URL (script src, img src, link href except
    // the hints themselves, source srcset, iframe src, video
    // src, audio src, etc).
    const used = new Set();
    function originOf(url) {
        try {
            const u = new URL(url, document.baseURI);
            return u.origin;
        } catch (_) {
            return '';
        }
    }
    const all = document.querySelectorAll('[src], [href], [srcset]');
    for (let i = 0; i < all.length; i++) {
        const el = all[i];
        // Skip the hint links themselves — they're what we're
        // auditing.
        const tag = el.tagName.toLowerCase();
        const elRel = (el.getAttribute('rel') || '').toLowerCase();
        if (tag === 'link' && HINT_RELS.has(elRel)) continue;
        for (const attr of ['src', 'href', 'srcset']) {
            const v = el.getAttribute(attr);
            if (!v) continue;
            // srcset is space + comma separated descriptors.
            const urls = attr === 'srcset' ? v.split(/[,\s]+/).filter(Boolean) : [v];
            for (const u of urls) {
                const o = originOf(u);
                if (o) used.add(o);
            }
        }
    }
    return { hints, used_origins: Array.from(used) };
})()"##;

/// One declared resource-hint link.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Hint {
    /// `rel` attribute value verbatim (`"preconnect"`,
    /// `"preconnect dns-prefetch"`, etc).
    pub rel: String,
    /// `href` attribute.
    pub href: String,
    /// `as` attribute (relevant for `rel="preload"`); empty if absent.
    pub as_attr: String,
    /// `crossorigin` attribute (`""` / `"anonymous"` / `"use-credentials"`).
    pub crossorigin: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct StalePreconnectSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Every resource-hint `<link>` in head.
    pub hints: Vec<Hint>,
    /// Origins (`scheme://host[:port]`) referenced elsewhere on
    /// the page via src/href/srcset.
    pub used_origins: Vec<String>,
}

/// Pure detector: returns one finding per stale hint.
#[must_use]
pub fn detect_stale_preconnects(snap: &StalePreconnectSnapshot) -> Vec<crate::AxisFinding> {
    use std::collections::BTreeSet;
    let used: BTreeSet<&str> = snap.used_origins.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::new();
    for hint in &snap.hints {
        let hint_origin = match origin_of(&hint.href) {
            Some(o) => o,
            None => continue,
        };
        if !used.contains(hint_origin.as_str()) {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Warn,
                kind: "stale_preconnect.unused-origin".to_owned(),
                detail: format!(
                    "<link rel=\"{}\" href=\"{}\"> opens a connection to {} but nothing else on the page loads from that origin. Common after a CDN / font / analytics migration left the boilerplate hint behind. Each unused preconnect adds DNS + TCP + TLS round-trips with zero benefit. Remove the hint if the origin is dead, or actually use it.",
                    hint.rel, hint.href, hint_origin
                ),
            });
        }
        // Cross-origin preconnect without crossorigin=anonymous is
        // silently wasted for fetch() / fonts. Skip same-origin
        // hints (no crossorigin attr expected).
        if hint_is_cross_origin(&hint_origin, &snap.page_url)
            && hint.rel.split_whitespace().any(|t| t == "preconnect")
            && hint.crossorigin.is_empty()
        {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Warn,
                kind: "stale_preconnect.crossorigin-anonymous-missing".to_owned(),
                detail: format!(
                    "<link rel=\"preconnect\" href=\"{}\"> targets a cross-origin host without crossorigin=\"anonymous\". Browsers IGNORE this hint for fetch() / font / CORS requests (the dominant cross-origin loads); the connection only warms for cookie-bearing requests. Add crossorigin=\"anonymous\" to match the actual load mode.",
                    hint.href
                ),
            });
        }
    }
    out
}

/// Extract `scheme://host[:port]` from `url`. Returns `None`
/// for invalid URLs or schemeless relative refs (which can't
/// preconnect anywhere).
fn origin_of(url: &str) -> Option<String> {
    // Minimal URL origin extractor — full RFC 3986 isn't needed;
    // we only care about hint URLs which are typically absolute.
    let after_scheme = url.find("://")?;
    let scheme = &url[..after_scheme];
    let rest = &url[after_scheme + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

fn hint_is_cross_origin(hint_origin: &str, page_url: &str) -> bool {
    match origin_of(page_url) {
        Some(page_origin) => page_origin != hint_origin,
        None => true, // unknown page origin → conservatively treat as cross
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(page_url: &str, hints: Vec<Hint>, used: Vec<&str>) -> StalePreconnectSnapshot {
        StalePreconnectSnapshot {
            page_url: page_url.to_owned(),
            hints,
            used_origins: used.into_iter().map(String::from).collect(),
        }
    }

    fn hint(rel: &str, href: &str) -> Hint {
        Hint {
            rel: rel.to_owned(),
            href: href.to_owned(),
            as_attr: String::new(),
            crossorigin: String::new(),
        }
    }

    #[test]
    fn unused_origin_flags() {
        let s = snap(
            "https://example.com/",
            vec![hint("preconnect", "https://fonts.googleapis.com")],
            vec!["https://example.com"], // no fonts.googleapis.com use
        );
        let f = detect_stale_preconnects(&s);
        assert!(
            f.iter().any(|x| x.kind == "stale_preconnect.unused-origin"),
            "expected unused-origin, got {f:?}"
        );
    }

    #[test]
    fn used_origin_does_not_flag_unused() {
        let s = snap(
            "https://example.com/",
            vec![hint("preconnect", "https://fonts.googleapis.com")],
            vec!["https://example.com", "https://fonts.googleapis.com"],
        );
        let f = detect_stale_preconnects(&s);
        assert!(!f.iter().any(|x| x.kind == "stale_preconnect.unused-origin"));
    }

    #[test]
    fn crossorigin_missing_flags_on_cross_origin_preconnect() {
        let s = snap(
            "https://example.com/",
            vec![hint("preconnect", "https://fonts.googleapis.com")],
            vec!["https://example.com", "https://fonts.googleapis.com"],
        );
        let f = detect_stale_preconnects(&s);
        assert!(
            f.iter()
                .any(|x| x.kind == "stale_preconnect.crossorigin-anonymous-missing"),
            "expected crossorigin-anonymous-missing, got {f:?}"
        );
    }

    #[test]
    fn crossorigin_present_does_not_flag() {
        let mut h = hint("preconnect", "https://fonts.googleapis.com");
        h.crossorigin = "anonymous".to_owned();
        let s = snap(
            "https://example.com/",
            vec![h],
            vec!["https://example.com", "https://fonts.googleapis.com"],
        );
        let f = detect_stale_preconnects(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "stale_preconnect.crossorigin-anonymous-missing"));
    }

    #[test]
    fn same_origin_preconnect_does_not_flag_crossorigin() {
        let s = snap(
            "https://example.com/",
            vec![hint("preconnect", "https://example.com")],
            vec!["https://example.com"],
        );
        let f = detect_stale_preconnects(&s);
        assert!(!f
            .iter()
            .any(|x| x.kind == "stale_preconnect.crossorigin-anonymous-missing"));
    }

    #[test]
    fn dns_prefetch_is_audited_too() {
        let s = snap(
            "https://example.com/",
            vec![hint("dns-prefetch", "https://stale.example.net")],
            vec!["https://example.com"],
        );
        let f = detect_stale_preconnects(&s);
        assert!(f.iter().any(|x| x.kind == "stale_preconnect.unused-origin"));
    }

    #[test]
    fn origin_of_extracts_scheme_host() {
        assert_eq!(
            origin_of("https://fonts.googleapis.com/css?family=Inter"),
            Some("https://fonts.googleapis.com".to_owned())
        );
        assert_eq!(
            origin_of("http://example.com:8080/path"),
            Some("http://example.com:8080".to_owned())
        );
        assert_eq!(origin_of("/relative/path"), None);
        assert_eq!(origin_of(""), None);
    }

    #[test]
    fn no_hints_no_findings() {
        let s = snap("https://example.com/", vec![], vec!["https://example.com"]);
        assert!(detect_stale_preconnects(&s).is_empty());
    }
}
