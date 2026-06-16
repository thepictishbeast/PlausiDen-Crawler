//! `download_attribute_audit` — `<a download>` semantic audit.
//!
//! Sibling axis to `pdf_link_indicator` (which audits PDF-link
//! affordances) and to `link_target_blank_safety` (which audits
//! `target="_blank"` cross-origin safety). This detector audits
//! the `download` attribute on anchors.
//!
//! ## The `download` attribute contract
//!
//! Per HTML spec + browser security model:
//!
//! 1. `download` is a **same-origin** affordance. Browsers
//!    ignore `download` on a cross-origin href to prevent
//!    attackers from forcing victims' browsers to save files
//!    they cannot otherwise reach. Operators routinely write
//!    `<a href="https://cdn.example.net/file.zip" download>`
//!    expecting it to trigger a save dialog — it doesn't.
//! 2. `download` requires a **resolvable href**. `<a download>`
//!    with no href is a no-op.
//! 3. The `download` attribute's **value** (when provided) is
//!    the suggested filename. A value containing path
//!    separators (`/`, `\`) is stripped by the browser to the
//!    basename. Operators sometimes pass an obvious path
//!    intending it to land in a subdirectory — it doesn't.
//! 4. `download` on an anchor with a `mailto:` / `tel:` /
//!    `javascript:` href is a no-op (browser ignores it).
//!
//! ## Findings
//!
//! * `download.cross-origin-ignored` strict — `<a href="https://
//!   other-host.example/...">` with a `download` attribute. The
//!   browser will treat the anchor as a normal navigation, not a
//!   download. Operator likely misunderstands the contract.
//! * `download.no-href` strict — `<a download>` with no `href`.
//!   Activating the anchor does nothing.
//! * `download.unsupported-scheme` strict — `<a download>` whose
//!   href uses `mailto:` / `tel:` / `javascript:` /
//!   `about:` / `blob:` (etc.). Browser ignores `download`.
//! * `download.filename-contains-path-separator` warn — the
//!   `download="…"` value contains `/` or `\`; browser will
//!   strip to the basename, so the operator's intent is being
//!   silently rewritten.
//!
//! Out of scope:
//!
//! * Verifying that the resource served actually carries a
//!   `Content-Disposition: attachment` header — separate runner
//!   axis (responses, not DOM).
//! * Whether the file extension matches the actual file type —
//!   future `download_extension_matches_content_type` axis.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.
//! * MAX_EXAMPLES = 5 for any per-bucket finding list.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const MAX_EXAMPLES: usize = 5;

/// Schemes where `download` is silently ignored by browsers.
/// Per HTML spec, `download` only triggers for `http(s):` (and
/// same-origin per the cross-origin check, but that's handled
/// separately).
const UNSUPPORTED_SCHEMES: &[&str] = &[
    "mailto:",
    "tel:",
    "javascript:",
    "about:",
    "blob:",
    "data:",
    "ws:",
    "wss:",
    "file:",
];

/// One captured anchor that carries a `download` attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DownloadAnchorEntry {
    /// CSS-ish selector pointing at the anchor.
    pub selector: String,
    /// `href=` attribute value (None when the attribute is
    /// absent — distinct from empty string).
    pub href: Option<String>,
    /// Raw value of the `download` attribute (empty string
    /// when the attribute is present without a value —
    /// `<a download>`).
    pub download_value: String,
    /// Resolved origin of the anchor's href (scheme + host +
    /// port), captured by the runner. `None` when href is
    /// missing or non-HTTP(S).
    pub href_origin: Option<String>,
    /// Page's own origin captured at snapshot time.
    pub page_origin: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DownloadAttributeAuditSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every anchor that carries `download`.
    pub anchors: Vec<DownloadAnchorEntry>,
}

/// Detector.
#[must_use]
pub fn detect_download_attribute_audit(
    snap: &DownloadAttributeAuditSnapshot,
) -> Vec<AxisFinding> {
    let mut cross_origin: Vec<String> = Vec::new();
    let mut no_href: Vec<&str> = Vec::new();
    let mut unsupported_scheme: Vec<String> = Vec::new();
    let mut path_separator: Vec<String> = Vec::new();

    for entry in &snap.anchors {
        match entry.href.as_deref() {
            None => {
                no_href.push(entry.selector.as_str());
            }
            Some(href) => {
                let trimmed = href.trim();
                if trimmed.is_empty() {
                    no_href.push(entry.selector.as_str());
                } else if let Some(scheme) = unsupported_scheme_of(trimmed) {
                    unsupported_scheme.push(format!(
                        "{} (href scheme={})",
                        entry.selector, scheme
                    ));
                } else if let (Some(href_origin), false) =
                    (entry.href_origin.as_deref(), entry.page_origin.is_empty())
                {
                    if !origins_equal(href_origin, &entry.page_origin) {
                        cross_origin.push(format!(
                            "{} (href_origin={}, page_origin={})",
                            entry.selector, href_origin, entry.page_origin
                        ));
                    }
                }
            }
        }

        // Path-separator check applies regardless of other
        // findings — it's about the attribute VALUE, not the
        // href.
        if entry.download_value.contains('/')
            || entry.download_value.contains('\\')
        {
            path_separator.push(format!(
                "{} (download=\"{}\")",
                entry.selector, entry.download_value
            ));
        }
    }

    let mut findings = Vec::new();
    let total = snap.anchors.len();

    if !cross_origin.is_empty() {
        let preview =
            preview_examples(&cross_origin.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "download.cross-origin-ignored".to_owned(),
            detail: format!(
                "{} of {} <a download> anchor(s) point at a cross-origin host; browsers silently ignore `download` cross-origin. Examples: {}",
                cross_origin.len(),
                total,
                preview
            ),
        });
    }

    if !no_href.is_empty() {
        let preview = preview_examples(&no_href);
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "download.no-href".to_owned(),
            detail: format!(
                "{} of {} <a download> anchor(s) have no href; activating the anchor does nothing. Examples: {}",
                no_href.len(),
                total,
                preview
            ),
        });
    }

    if !unsupported_scheme.is_empty() {
        let preview = preview_examples(
            &unsupported_scheme
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "download.unsupported-scheme".to_owned(),
            detail: format!(
                "{} of {} <a download> anchor(s) use a scheme where `download` is ignored (mailto:/tel:/javascript:/about:/blob:/data:/ws(s):/file:). Examples: {}",
                unsupported_scheme.len(),
                total,
                preview
            ),
        });
    }

    if !path_separator.is_empty() {
        let preview = preview_examples(
            &path_separator
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "download.filename-contains-path-separator".to_owned(),
            detail: format!(
                "{} of {} <a download> anchor(s) carry a download value with path separator(s); browsers strip to basename. Examples: {}",
                path_separator.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn unsupported_scheme_of(href: &str) -> Option<&'static str> {
    let lower = href.to_ascii_lowercase();
    UNSUPPORTED_SCHEMES
        .iter()
        .find(|s| lower.starts_with(*s))
        .copied()
}

fn origins_equal(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

fn preview_examples(examples: &[&str]) -> String {
    let mut buf = String::new();
    let n = examples.len().min(MAX_EXAMPLES);
    for (i, sel) in examples.iter().take(n).enumerate() {
        if i > 0 {
            buf.push_str(" | ");
        }
        buf.push_str(sel);
    }
    if examples.len() > MAX_EXAMPLES {
        buf.push_str(&format!(" (+{} more)", examples.len() - MAX_EXAMPLES));
    }
    buf
}

/// Page-side eval. Captures every `<a download>` plus its href +
/// resolved origin vs the page's own origin.
pub const DOWNLOAD_ATTRIBUTE_AUDIT_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return parts.join(' > ') || 'body';
    };

    const pageOrigin = location.origin;
    const anchors = Array.from(document.querySelectorAll('a[download]')).map(function(a) {
      const href = a.hasAttribute('href') ? a.getAttribute('href') : null;
      const dv = a.getAttribute('download') || '';
      let hrefOrigin = null;
      if (href) {
        try {
          const u = new URL(href, location.href);
          if (u.protocol === 'http:' || u.protocol === 'https:') {
            hrefOrigin = u.origin;
          }
        } catch (_) { /* unparseable url */ }
      }
      return {
        selector: selectorOf(a),
        href: href,
        downloadValue: dv,
        hrefOrigin: hrefOrigin,
        pageOrigin: pageOrigin
      };
    });

    return {
      pageUrl: location.href,
      anchors: anchors
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        sel: &str,
        href: Option<&str>,
        dv: &str,
        href_origin: Option<&str>,
        page_origin: &str,
    ) -> DownloadAnchorEntry {
        DownloadAnchorEntry {
            selector: sel.to_owned(),
            href: href.map(str::to_owned),
            download_value: dv.to_owned(),
            href_origin: href_origin.map(str::to_owned),
            page_origin: page_origin.to_owned(),
        }
    }

    fn snap(anchors: Vec<DownloadAnchorEntry>) -> DownloadAttributeAuditSnapshot {
        DownloadAttributeAuditSnapshot {
            page_url: "https://example.test/".to_owned(),
            anchors,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_download_attribute_audit(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn same_origin_https_download_is_clean() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#dl",
            Some("/files/report.pdf"),
            "report.pdf",
            Some("https://example.test"),
            "https://example.test",
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn cross_origin_download_is_strict() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#cdn",
            Some("https://cdn.other.net/file.zip"),
            "file.zip",
            Some("https://cdn.other.net"),
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "download.cross-origin-ignored")
            .expect("cross-origin finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("a#cdn"));
        assert!(hit.detail.contains("cdn.other.net"));
    }

    #[test]
    fn no_href_is_strict() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#bogus",
            None,
            "foo.zip",
            None,
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "download.no-href")
            .expect("no-href finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn empty_string_href_treated_as_no_href() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#empty",
            Some(""),
            "f.zip",
            None,
            "https://example.test",
        )]));
        assert!(f.iter().any(|x| x.kind == "download.no-href"));
    }

    #[test]
    fn mailto_scheme_is_strict_unsupported() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#mail",
            Some("mailto:foo@example.com"),
            "f.zip",
            None,
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "download.unsupported-scheme")
            .expect("unsupported-scheme expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("mailto"));
    }

    #[test]
    fn javascript_scheme_is_strict_unsupported() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#js",
            Some("javascript:alert(1)"),
            "f.zip",
            None,
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "download.unsupported-scheme")
            .expect("javascript scheme expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("javascript"));
    }

    #[test]
    fn each_unsupported_scheme_is_flagged() {
        let schemes = [
            "mailto:foo@example.com",
            "tel:+1234567890",
            "javascript:void(0)",
            "about:blank",
            "blob:abc123",
            "data:text/plain,hi",
            "ws://example.com",
            "wss://example.com",
            "file:///etc/hosts",
        ];
        for href in schemes {
            let f = detect_download_attribute_audit(&snap(vec![entry(
                "a#x",
                Some(href),
                "f.zip",
                None,
                "https://example.test",
            )]));
            assert!(
                f.iter().any(|x| x.kind == "download.unsupported-scheme"),
                "scheme {href} should flag unsupported"
            );
        }
    }

    #[test]
    fn forward_slash_in_download_value_is_warn() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#path",
            Some("/files/x.zip"),
            "subdir/file.zip",
            Some("https://example.test"),
            "https://example.test",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "download.filename-contains-path-separator")
            .expect("path-separator finding expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("subdir/file.zip"));
    }

    #[test]
    fn backslash_in_download_value_also_warns() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#path",
            Some("/files/x.zip"),
            r"sub\file.zip",
            Some("https://example.test"),
            "https://example.test",
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "download.filename-contains-path-separator"));
    }

    #[test]
    fn same_origin_with_trailing_slash_normalises() {
        let f = detect_download_attribute_audit(&snap(vec![entry(
            "a#dl",
            Some("/files/x.zip"),
            "x.zip",
            Some("https://example.test/"),
            "https://example.test",
        )]));
        assert!(f.is_empty(), "trailing-slash origins should normalise: {f:?}");
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let anchors: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("a#x{i}"),
                    Some("https://cdn.other.net/f.zip"),
                    "f.zip",
                    Some("https://cdn.other.net"),
                    "https://example.test",
                )
            })
            .collect();
        let f = detect_download_attribute_audit(&snap(anchors));
        let hit = f
            .iter()
            .find(|x| x.kind == "download.cross-origin-ignored")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_download_anchors() {
        assert!(DOWNLOAD_ATTRIBUTE_AUDIT_JS.starts_with("(() => {"));
        assert!(DOWNLOAD_ATTRIBUTE_AUDIT_JS.ends_with(")()"));
        assert!(DOWNLOAD_ATTRIBUTE_AUDIT_JS.contains("a[download]"));
        assert!(DOWNLOAD_ATTRIBUTE_AUDIT_JS.contains("location.origin"));
        assert!(DOWNLOAD_ATTRIBUTE_AUDIT_JS.contains("new URL"));
    }
}
