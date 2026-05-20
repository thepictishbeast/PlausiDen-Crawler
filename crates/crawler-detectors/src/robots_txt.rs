//! `robots_txt` — fetch `/robots.txt`, parse, audit against the
//! canonical URL.
//!
//! Lighthouse "robots-txt" + "is-crawlable" SEO audits.
//!
//! ## Heuristic
//!
//! Snapshot performs a same-origin `fetch('/robots.txt')` from
//! the page context and returns the HTTP status + body. Rust
//! classifier checks:
//!
//! * **`robots-txt.missing`** (warn) — 404 / network error. A
//!   robots.txt isn't strictly required, but its absence means
//!   crawlers default to "everything allowed" which is rarely
//!   what site owners want.
//! * **`robots-txt.disallows-canonical`** (strict) — robots.txt
//!   contains a `Disallow:` rule matching the page's canonical
//!   URL for `User-agent: *` AND the page lacks
//!   `<meta name="robots" content="noindex">`. Config mismatch —
//!   site claims indexable in HTML but blocks crawlers in
//!   robots.
//! * **`robots-txt.sitemap-unreachable`** (warn) — robots.txt
//!   references a sitemap URL whose own fetch returns non-2xx.
//!
//! Caller opt-out: there's no per-page opt-out — robots.txt is a
//! site-level config, not a per-page concern.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RobotsTxtSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Canonical pathname for the current page (e.g. `/`,
    /// `/about`). Used to test Disallow rules.
    pub canonical_path: String,
    /// True if the page declares `<meta name="robots"
    /// content="noindex">` — relaxes disallow-canonical strict.
    pub html_noindex: bool,
    /// HTTP status of the `/robots.txt` fetch. `0` = network
    /// error / CORS block.
    pub status: u16,
    /// Robots.txt body (capped at 8 KB on the browser side).
    pub body: String,
    /// Status of each sitemap URL the robots.txt references.
    /// Empty if no sitemaps declared.
    pub sitemap_statuses: Vec<SitemapStatus>,
}

/// One sitemap URL + its fetch status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SitemapStatus {
    /// Sitemap URL as declared in robots.txt.
    pub url: String,
    /// HTTP status. 0 on network error.
    pub status: u16,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_robots_txt(snap: &RobotsTxtSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    if snap.status == 0 || snap.status == 404 {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "robots-txt.missing".to_owned(),
            detail: format!(
                "No `/robots.txt` reachable (status {}). Crawlers default to \"everything allowed\". If that's intentional, declare an empty robots.txt with `User-agent: *\\nAllow: /` so the absence is intentional rather than oversight.",
                snap.status
            ),
        });
        return out;
    }
    if !(200..300).contains(&snap.status) {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "robots-txt.bad-status".to_owned(),
            detail: format!(
                "`/robots.txt` returned non-2xx status {}. Crawlers may interpret as missing.",
                snap.status
            ),
        });
        return out;
    }
    // Walk the body for User-agent: * Disallow: rules that match canonical.
    let disallows = parse_disallows_for_wildcard(&snap.body);
    if !snap.html_noindex && rule_matches(&disallows, &snap.canonical_path) {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "robots-txt.disallows-canonical".to_owned(),
            detail: format!(
                "/robots.txt disallows canonical path `{}` for User-agent: * AND the page does NOT declare `<meta name=\"robots\" content=\"noindex\">`. This is a config mismatch — the page is marked indexable in HTML but blocked in robots.txt. Reconcile: either remove the Disallow rule or add the noindex meta.",
                snap.canonical_path
            ),
        });
    }
    for s in &snap.sitemap_statuses {
        if !(200..300).contains(&s.status) {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "robots-txt.sitemap-unreachable".to_owned(),
                detail: format!(
                    "robots.txt references sitemap `{}` which returned status {}. Crawlers will skip it; remove the reference or fix the URL.",
                    s.url, s.status
                ),
            });
        }
    }
    out
}

/// Parse robots.txt and return the Disallow path-prefixes that
/// apply to `User-agent: *`. Case-insensitive directive names.
/// Comments (`#`) stripped. Other UA groups ignored — they're
/// per-bot policy, not site-default.
fn parse_disallows_for_wildcard(body: &str) -> Vec<String> {
    let mut in_wildcard_group = false;
    let mut disallows: Vec<String> = Vec::new();
    for raw_line in body.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_lowercase();
        let value = value.trim();
        if name == "user-agent" {
            in_wildcard_group = value == "*";
            continue;
        }
        if in_wildcard_group && name == "disallow" && !value.is_empty() {
            disallows.push(value.to_owned());
        }
    }
    disallows
}

/// True if any of the Disallow prefixes matches the given path.
/// Prefix-match per RFC 9309 §2.2.2 (Path-Matching).
fn rule_matches(disallows: &[String], path: &str) -> bool {
    for d in disallows {
        // Empty Disallow allows everything — skip.
        if d.is_empty() {
            continue;
        }
        // Trailing $ is end-of-string anchor.
        if let Some(prefix) = d.strip_suffix('$') {
            if path == prefix {
                return true;
            }
            continue;
        }
        if path.starts_with(d.as_str()) {
            return true;
        }
    }
    false
}

/// Browser-side capture. Performs the fetches inside the page
/// context (CORS doesn't apply for same-origin / robots.txt).
pub const ROBOTS_TXT_DOM_CAPTURE_JS: &str = r#"
(async () => {
    const canonicalLink = document.querySelector('link[rel="canonical"]');
    let canonical = canonicalLink ? canonicalLink.getAttribute('href') || window.location.pathname : window.location.pathname;
    try {
      const u = new URL(canonical, window.location.href);
      canonical = u.pathname || '/';
    } catch (_) {
      canonical = window.location.pathname || '/';
    }

    const meta = document.querySelector('meta[name="robots" i]');
    const htmlNoindex = !!(meta && (meta.getAttribute('content') || '').toLowerCase().indexOf('noindex') !== -1);

    let status = 0;
    let body = '';
    try {
      const resp = await fetch('/robots.txt', { credentials: 'omit' });
      status = resp.status;
      if (resp.ok) {
        body = await resp.text();
        if (body.length > 8192) body = body.slice(0, 8192);
      }
    } catch (_) {
      status = 0;
    }

    // Sitemap URLs from robots.txt body.
    const sitemapUrls = [];
    body.split('\n').forEach(function(line) {
      const noComment = line.split('#')[0].trim();
      const colon = noComment.indexOf(':');
      if (colon < 0) return;
      const name = noComment.slice(0, colon).trim().toLowerCase();
      const value = noComment.slice(colon + 1).trim();
      if (name === 'sitemap' && value) sitemapUrls.push(value);
    });

    const sitemapStatuses = [];
    for (let i = 0; i < sitemapUrls.length && i < 5; i++) {
      let s = 0;
      try {
        const r = await fetch(sitemapUrls[i], { credentials: 'omit' });
        s = r.status;
      } catch (_) { s = 0; }
      sitemapStatuses.push({ url: sitemapUrls[i], status: s });
    }

    return {
      pageUrl: window.location.href,
      canonicalPath: canonical,
      htmlNoindex: htmlNoindex,
      status: status,
      body: body,
      sitemapStatuses: sitemapStatuses,
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(status: u16, body: &str, canonical: &str, noindex: bool) -> RobotsTxtSnapshot {
        RobotsTxtSnapshot {
            page_url: "https://example.com/".to_owned(),
            canonical_path: canonical.to_owned(),
            html_noindex: noindex,
            status,
            body: body.to_owned(),
            sitemap_statuses: Vec::new(),
        }
    }

    #[test]
    fn missing_robots_txt_warns() {
        let s = snap(404, "", "/", false);
        let f = detect_robots_txt(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "robots-txt.missing");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn network_error_warns_missing() {
        let s = snap(0, "", "/", false);
        let f = detect_robots_txt(&s);
        assert_eq!(f[0].kind, "robots-txt.missing");
    }

    #[test]
    fn bad_status_warns() {
        let s = snap(500, "", "/", false);
        let f = detect_robots_txt(&s);
        assert_eq!(f[0].kind, "robots-txt.bad-status");
    }

    #[test]
    fn allow_only_passes() {
        let s = snap(200, "User-agent: *\nAllow: /\n", "/", false);
        assert!(detect_robots_txt(&s).is_empty());
    }

    #[test]
    fn disallows_canonical_strict_when_no_noindex() {
        let s = snap(
            200,
            "User-agent: *\nDisallow: /private\n",
            "/private",
            false,
        );
        let f = detect_robots_txt(&s);
        assert_eq!(f[0].kind, "robots-txt.disallows-canonical");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn disallows_canonical_silent_with_noindex() {
        let s = snap(200, "User-agent: *\nDisallow: /private\n", "/private", true);
        assert!(detect_robots_txt(&s).is_empty());
    }

    #[test]
    fn end_of_string_anchor_respected() {
        // Disallow: /foo$ matches only exactly /foo, not /foo/bar.
        let s = snap(200, "User-agent: *\nDisallow: /foo$\n", "/foo/bar", false);
        assert!(detect_robots_txt(&s).is_empty());

        let s2 = snap(200, "User-agent: *\nDisallow: /foo$\n", "/foo", false);
        let f = detect_robots_txt(&s2);
        assert_eq!(f[0].kind, "robots-txt.disallows-canonical");
    }

    #[test]
    fn other_useragent_groups_ignored() {
        // Disallow under Googlebot doesn't fire wildcard rule.
        let s = snap(
            200,
            "User-agent: Googlebot\nDisallow: /private\nUser-agent: *\nAllow: /\n",
            "/private",
            false,
        );
        assert!(detect_robots_txt(&s).is_empty());
    }

    #[test]
    fn comments_stripped() {
        let s = snap(
            200,
            "# header comment\nUser-agent: * # this is wildcard\nDisallow: /private # private stuff\n",
            "/private",
            false,
        );
        let f = detect_robots_txt(&s);
        assert_eq!(f[0].kind, "robots-txt.disallows-canonical");
    }

    #[test]
    fn unreachable_sitemap_warns() {
        let mut s = snap(
            200,
            "Sitemap: https://example.com/sitemap.xml\n",
            "/",
            false,
        );
        s.sitemap_statuses.push(SitemapStatus {
            url: "https://example.com/sitemap.xml".to_owned(),
            status: 404,
        });
        let f = detect_robots_txt(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "robots-txt.sitemap-unreachable");
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(200, "User-agent: *\nAllow: /\n", "/", false);
        let j = serde_json::to_string(&s).expect("ser");
        let back: RobotsTxtSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.status, 200);
        assert_eq!(back.canonical_path, "/");
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in ROBOTS_TXT_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }
}
