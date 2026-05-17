//! `corp` — per-sub-resource Cross-Origin-Resource-Policy audit.
//!
//! Mirror of `src/corp.ts`. First per-sub-resource detector (vs the
//! per-page-headers pattern the other ports use). The crawler-side
//! `allHeaders` Map carries every sub-resource response's headers;
//! this detector walks them, filters to cross-origin loads, and
//! classifies each.
//!
//! Findings:
//!
//!   * `corp.cross-origin-resource-no-corp` — strict if the page
//!     declares COEP=require-corp (browser BLOCKS), warn otherwise
//!     (forward-compat: the resource breaks the moment the page
//!     adopts require-corp)
//!   * `corp.cross-origin-resource-invalid` — CORP value not in
//!     {same-origin, same-site, cross-origin}; browsers may reject
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::url_helpers::is_localhost;
use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One captured cross-origin sub-resource the page loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedSubResource {
    /// Full URL of the sub-resource.
    pub url: String,
    /// `<scheme>://<host>` of the sub-resource (cached for grouping).
    pub origin: String,
    /// Lowercase + trimmed CORP value, or `None` if absent.
    pub corp: Option<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CorpSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// `<scheme>://<host>` of the page.
    pub page_origin: String,
    /// Localhost / loopback exemption.
    pub page_is_localhost: bool,
    /// True iff the page's COEP header is exactly `require-corp`.
    pub page_requires_corp: bool,
    /// Cross-origin sub-resources only — same-origin filtered out
    /// at snapshot-build time.
    pub cross_origin_sub_resources: Vec<CapturedSubResource>,
}

/// Acceptable CORP values per W3C.
const ACCEPTABLE_CORP: &[&str] = &["same-origin", "same-site", "cross-origin"];

/// Extract `<scheme>://<host>` from a URL string. Returns empty
/// string on parse failure.
fn origin_of(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return String::new();
    };
    let scheme = &url[..scheme_end];
    let after = &url[scheme_end + 3..];
    let host_end = after
        .find('/')
        .or_else(|| after.find('?'))
        .or_else(|| after.find('#'))
        .unwrap_or(after.len());
    format!("{}://{}", scheme, &after[..host_end])
}

/// Only http(s) sub-resources are auditable; data:/blob:/about:
/// have no transport-level CORP.
fn is_auditable_scheme(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Build a snapshot from the captured per-resource headers map.
/// `all_headers` maps URL → that response's lowercased header map.
pub fn build_corp_snapshot(
    page_url: &str,
    page_headers: &BTreeMap<String, String>,
    all_headers: &BTreeMap<String, BTreeMap<String, String>>,
) -> CorpSnapshot {
    let page_origin = origin_of(page_url);
    let page_is_localhost = is_localhost(page_url);

    let coep = page_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-embedder-policy"))
        .map(|(_, v)| v.trim().to_ascii_lowercase());
    let page_requires_corp = coep.as_deref() == Some("require-corp");

    let mut cross_origin: Vec<CapturedSubResource> = Vec::new();
    for (url, headers) in all_headers {
        if !is_auditable_scheme(url) {
            continue;
        }
        let ro = origin_of(url);
        if ro.is_empty() || ro == page_origin {
            continue;
        }
        let corp = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-resource-policy"))
            .map(|(_, v)| v.trim().to_ascii_lowercase());
        cross_origin.push(CapturedSubResource {
            url: url.clone(),
            origin: ro,
            corp,
        });
    }

    CorpSnapshot {
        page_url: page_url.to_owned(),
        page_origin,
        page_is_localhost,
        page_requires_corp,
        cross_origin_sub_resources: cross_origin,
    }
}

/// Render an example string for one sub-resource finding.
fn render_example(r: &CapturedSubResource) -> String {
    format!("{} ← '{}'", r.origin, r.url)
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_corp_issues(snap: &CorpSnapshot) -> Vec<AxisFinding> {
    if snap.page_is_localhost {
        return Vec::new();
    }

    let mut no_corp: Vec<&CapturedSubResource> = Vec::new();
    let mut invalid: Vec<&CapturedSubResource> = Vec::new();

    for r in &snap.cross_origin_sub_resources {
        match r.corp.as_deref() {
            None => no_corp.push(r),
            Some(v) if !ACCEPTABLE_CORP.contains(&v) => invalid.push(r),
            Some(_) => {} // valid value → no finding
        }
    }

    let mut out = Vec::new();

    if !no_corp.is_empty() {
        let examples: Vec<String> = no_corp.iter().take(5).map(|r| render_example(r)).collect();
        let severity = if snap.page_requires_corp {
            AxisSeverity::Strict
        } else {
            AxisSeverity::Warn
        };
        let blocker = if snap.page_requires_corp {
            "The page sets Cross-Origin-Embedder-Policy: require-corp, so the browser BLOCKS these resources at load — they fail to render."
        } else {
            "The page does not currently enforce COEP=require-corp, so the resources still load. The moment the page adopts COEP=require-corp (a supersociety baseline for any app handling sensitive data), every one of these sub-resources stops working."
        };
        out.push(AxisFinding {
            severity,
            kind: "corp.cross-origin-resource-no-corp".into(),
            detail: format!(
                "{} cross-origin sub-resource(s) lack a Cross-Origin-Resource-Policy header. {} The fix lives on the SERVER side of each resource — set 'Cross-Origin-Resource-Policy: cross-origin' on the asset response (or 'same-site' / 'same-origin' for tighter scoping). Examples: {}",
                no_corp.len(),
                blocker,
                examples.join("; ")
            ),
        });
    }

    if !invalid.is_empty() {
        let examples: Vec<String> = invalid
            .iter()
            .take(5)
            .map(|r| {
                format!(
                    "{} (CORP='{}')",
                    render_example(r),
                    r.corp.as_deref().unwrap_or("")
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "corp.cross-origin-resource-invalid".into(),
            detail: format!(
                "{} cross-origin sub-resource(s) have a Cross-Origin-Resource-Policy header set to a value not in the W3C-recognised set ('same-origin', 'same-site', 'cross-origin'). Browsers may reject the resource entirely. Examples: {}",
                invalid.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_headers(require_corp: bool) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        if require_corp {
            m.insert("cross-origin-embedder-policy".into(), "require-corp".into());
        }
        m
    }

    fn build(
        page_url: &str,
        require_corp: bool,
        sub_resources: &[(&str, Option<&str>)],
    ) -> CorpSnapshot {
        let mut all = BTreeMap::new();
        for (url, corp) in sub_resources {
            let mut h = BTreeMap::new();
            if let Some(v) = corp {
                h.insert("cross-origin-resource-policy".into(), (*v).to_owned());
            }
            all.insert((*url).to_owned(), h);
        }
        build_corp_snapshot(page_url, &page_headers(require_corp), &all)
    }

    #[test]
    fn localhost_skipped() {
        let s = build("http://localhost/", false, &[]);
        assert!(detect_corp_issues(&s).is_empty());
    }

    #[test]
    fn same_origin_filtered_out_at_snapshot_time() {
        let s = build(
            "https://example.com/",
            false,
            &[("https://example.com/img.png", None)],
        );
        assert_eq!(s.cross_origin_sub_resources.len(), 0);
        assert!(detect_corp_issues(&s).is_empty());
    }

    #[test]
    fn cross_origin_no_corp_warns_without_require_corp() {
        let s = build(
            "https://example.com/",
            false,
            &[("https://cdn.example.org/img.png", None)],
        );
        let f = detect_corp_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "corp.cross-origin-resource-no-corp");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn cross_origin_no_corp_strict_when_page_requires_corp() {
        let s = build(
            "https://example.com/",
            true,
            &[("https://cdn.example.org/img.png", None)],
        );
        let f = detect_corp_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, AxisSeverity::Strict);
        assert!(f[0].detail.contains("BLOCKS"));
    }

    #[test]
    fn acceptable_corp_values_pass() {
        for val in ["same-origin", "same-site", "cross-origin"] {
            let s = build(
                "https://example.com/",
                false,
                &[("https://cdn.example.org/img.png", Some(val))],
            );
            assert!(
                detect_corp_issues(&s).is_empty(),
                "{val} should be acceptable"
            );
        }
    }

    #[test]
    fn corp_case_insensitive() {
        let s = build(
            "https://example.com/",
            false,
            &[("https://cdn.example.org/img.png", Some("SAME-ORIGIN"))],
        );
        assert!(detect_corp_issues(&s).is_empty());
    }

    #[test]
    fn invalid_corp_value_warns() {
        let s = build(
            "https://example.com/",
            false,
            &[("https://cdn.example.org/img.png", Some("anywhere"))],
        );
        let f = detect_corp_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "corp.cross-origin-resource-invalid"));
    }

    #[test]
    fn data_url_subresource_skipped() {
        let s = build(
            "https://example.com/",
            false,
            &[("data:image/png;base64,AAAA", None)],
        );
        assert!(detect_corp_issues(&s).is_empty());
    }

    #[test]
    fn multiple_subresources_aggregate_in_one_finding() {
        let s = build(
            "https://example.com/",
            false,
            &[
                ("https://cdn.a.org/1.png", None),
                ("https://cdn.b.org/2.png", None),
                ("https://cdn.c.org/3.png", None),
            ],
        );
        let f = detect_corp_issues(&s);
        assert_eq!(f.len(), 1);
        assert!(f[0].detail.contains("3 cross-origin"));
    }

    #[test]
    fn examples_capped_at_5() {
        let urls: Vec<(String, Option<&str>)> = (0..10)
            .map(|i| (format!("https://cdn.{i}.org/x.png"), None))
            .collect();
        let url_refs: Vec<(&str, Option<&str>)> =
            urls.iter().map(|(u, c)| (u.as_str(), *c)).collect();
        let s = build("https://example.com/", false, &url_refs);
        let f = detect_corp_issues(&s);
        assert!(f[0].detail.contains("10 cross-origin"));
        // 5 examples in the rendered list
        let count = f[0].detail.matches("← '").count();
        assert_eq!(count, 5);
    }
}
