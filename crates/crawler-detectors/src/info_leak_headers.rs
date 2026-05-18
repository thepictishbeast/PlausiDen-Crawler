//! `info_leak_headers` — opsec hygiene audit for headers that
//! disclose server software, framework versions, or debug state.
//!
//! Mirror of `src/infoLeakHeaders.ts`. 8 findings (all `warn` —
//! opsec hygiene, not exploitability):
//!
//!   * `info-leak.server-version`       (Server: nginx/1.20.1)
//!   * `info-leak.x-powered-by`         (PHP/Express/ASP.NET)
//!   * `info-leak.x-aspnet-version`
//!   * `info-leak.x-aspnetmvc-version`
//!   * `info-leak.x-runtime`            (Rails/Sinatra/Django)
//!   * `info-leak.x-debug-token`        (Symfony web-profiler)
//!   * `info-leak.via`                  (RFC 7230 proxy trace)
//!   * `info-leak.x-generator`          (Drupal/WordPress/Hugo)
//!
//! Threat: version disclosure enables CVE lookup against the exact
//! build. Stripping the headers forces the adversary to enumerate
//! the surface manually.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InfoLeakSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// Map of lowercased header name → raw value, for the headers
    /// we audit. BTreeMap so JSON output is deterministic.
    pub headers: BTreeMap<String, String>,
}

/// Header names the detector cares about (lowercase).
const AUDITED_HEADERS: &[&str] = &[
    "server",
    "x-powered-by",
    "x-aspnet-version",
    "x-aspnetmvc-version",
    "x-runtime",
    "x-debug-token",
    "x-debug-token-link",
    "via",
    "x-generator",
];

/// Build a snapshot. Only headers in `AUDITED_HEADERS` (private)
/// are kept.
pub fn build_info_leak_snapshot(
    page_url: &str,
    headers: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> InfoLeakSnapshot {
    let page_is_localhost = is_localhost(page_url);
    let mut kept = BTreeMap::new();
    for (k, v) in headers {
        let lower = k.as_ref().to_ascii_lowercase();
        if AUDITED_HEADERS.contains(&lower.as_str()) {
            kept.insert(lower, v.as_ref().to_owned());
        }
    }
    InfoLeakSnapshot {
        page_url: page_url.to_owned(),
        page_is_localhost,
        headers: kept,
    }
}

/// Does this `Server`-like value contain a `<digit>.<digit>`
/// version token? Bare product names (`Server: nginx`) don't fire.
fn has_version_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'.' {
                let mut k = j + 1;
                while k < bytes.len() && bytes[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 {
                    return true;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

/// Truncate to at most 200 chars for evidence display.
fn preview(s: &str) -> String {
    s.chars().take(200).collect()
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_info_leak_issues(snap: &InfoLeakSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }

    let mut out = Vec::new();

    if let Some(server) = snap.headers.get("server") {
        if has_version_token(server) {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "info-leak.server-version".into(),
                detail: format!(
                    "Server header reveals version: '{}'. Enables CVE lookup against the exact build. Strip the header (nginx: 'server_tokens off;'; Apache: 'ServerTokens Prod' + 'ServerSignature Off') or set to a generic value ('Server: web').",
                    preview(server)
                ),
            });
        }
    }

    if let Some(xpb) = snap.headers.get("x-powered-by") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.x-powered-by".into(),
            detail: format!(
                "X-Powered-By header present: '{}'. Identifies the application framework — almost always auto-emitted and almost never needed in production. Disable in framework config (PHP: 'expose_php=Off'; Express: 'app.disable(\"x-powered-by\")'; ASP.NET: '<httpRuntime enableVersionHeader=\"false\">').",
                preview(xpb)
            ),
        });
    }

    if let Some(xav) = snap.headers.get("x-aspnet-version") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.x-aspnet-version".into(),
            detail: format!(
                "X-AspNet-Version header reveals CLR/.NET version: '{}'. Disable via '<httpRuntime enableVersionHeader=\"false\">' in web.config.",
                preview(xav)
            ),
        });
    }

    if let Some(xamv) = snap.headers.get("x-aspnetmvc-version") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.x-aspnetmvc-version".into(),
            detail: format!(
                "X-AspNetMvc-Version header reveals MVC framework version: '{}'. Disable in Global.asax: 'MvcHandler.DisableMvcResponseHeader = true;'.",
                preview(xamv)
            ),
        });
    }

    if let Some(xr) = snap.headers.get("x-runtime") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.x-runtime".into(),
            detail: format!(
                "X-Runtime header present: '{}'. Rails/Sinatra/Django emit per-request handling time, useful for operator debugging but reveals performance characteristics that aid timing-attack reconnaissance. Strip in production middleware.",
                preview(xr)
            ),
        });
    }

    let dbg = snap
        .headers
        .get("x-debug-token")
        .or_else(|| snap.headers.get("x-debug-token-link"));
    if let Some(v) = dbg {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.x-debug-token".into(),
            detail: format!(
                "X-Debug-Token / X-Debug-Token-Link header present: '{}'. Symfony web-profiler exposure — if this reached production, the debug toolbar is ALSO accessible (full route map, SQL query log, cache state). Disable the WebProfilerBundle in prod.",
                preview(v)
            ),
        });
    }

    if let Some(via) = snap.headers.get("via") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.via".into(),
            detail: format!(
                "Via header present: '{}'. Legitimate intermediate-proxy trace per RFC 7230, but in production it usually leaks internal hostnames or proxy software versions. Strip at the edge.",
                preview(via)
            ),
        });
    }

    if let Some(xgen) = snap.headers.get("x-generator") {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "info-leak.x-generator".into(),
            detail: format!(
                "X-Generator header reveals CMS/SSG: '{}'. Drupal/WordPress/Hugo/Jekyll auto-emit. Same CVE-targeting threat as the Server header. Strip via web-server config or CMS plugin.",
                preview(xgen)
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(headers: &[(&str, &str)], url: &str) -> InfoLeakSnapshot {
        build_info_leak_snapshot(url, headers.iter().map(|(k, v)| (*k, *v)))
    }

    #[test]
    fn localhost_skipped() {
        let s = snap(&[("Server", "nginx/1.20.1")], "http://localhost/");
        assert!(detect_info_leak_issues(&s).is_empty());
    }

    #[test]
    fn server_with_version_warns() {
        let s = snap(&[("Server", "nginx/1.20.1")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.server-version"));
    }

    #[test]
    fn server_bare_product_passes() {
        let s = snap(&[("Server", "nginx")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "info-leak.server-version"));
    }

    #[test]
    fn version_token_handles_multiple_formats() {
        assert!(has_version_token("Apache/2.4.41 (Ubuntu)"));
        assert!(has_version_token("Microsoft-IIS/10.0"));
        assert!(has_version_token("nginx 1.20.1"));
        assert!(!has_version_token("cloudflare"));
        assert!(!has_version_token("web"));
        // Lone digit doesn't count.
        assert!(!has_version_token("v1 server"));
    }

    #[test]
    fn x_powered_by_warns() {
        let s = snap(&[("X-Powered-By", "Express")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.x-powered-by"));
    }

    #[test]
    fn x_aspnet_version_warns() {
        let s = snap(&[("X-AspNet-Version", "4.0.30319")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.x-aspnet-version"));
    }

    #[test]
    fn x_runtime_warns() {
        let s = snap(&[("X-Runtime", "0.012345")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.x-runtime"));
    }

    #[test]
    fn x_debug_token_warns() {
        let s = snap(&[("X-Debug-Token", "abc123")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.x-debug-token"));
    }

    #[test]
    fn x_debug_token_link_also_warns() {
        let s = snap(
            &[("X-Debug-Token-Link", "/_profiler/abc123")],
            "https://example.com/",
        );
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.x-debug-token"));
    }

    #[test]
    fn via_warns() {
        let s = snap(&[("Via", "1.1 internal-proxy.svc")], "https://example.com/");
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.via"));
    }

    #[test]
    fn x_generator_warns() {
        let s = snap(
            &[("X-Generator", "WordPress 6.4.2")],
            "https://example.com/",
        );
        let f = detect_info_leak_issues(&s);
        assert!(f.iter().any(|x| x.kind == "info-leak.x-generator"));
    }

    #[test]
    fn unrelated_headers_dont_pollute_snapshot() {
        let s = snap(
            &[
                ("Content-Type", "text/html"),
                ("Cache-Control", "max-age=0"),
                ("Server", "nginx"),
            ],
            "https://example.com/",
        );
        assert_eq!(s.headers.len(), 1);
        assert!(s.headers.contains_key("server"));
    }

    #[test]
    fn case_insensitive_header_match() {
        let s = snap(&[("SERVER", "nginx/1.20.1")], "https://example.com/");
        assert!(detect_info_leak_issues(&s)
            .iter()
            .any(|x| x.kind == "info-leak.server-version"));
    }
}
