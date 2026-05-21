//! `link_internal_external_indication` — audits whether
//! external links are visually + accessibly distinguishable
//! from internal links.
//!
//! Sibling axis to `link_target_blank_safety` (which audits
//! `target="_blank"` security), `link_text` (which audits anchor
//! text quality), `link_color_only` (which audits whether links
//! rely on color alone for distinction), and `pdf_link_indicator`
//! (which audits PDF-link affordances). This detector covers a
//! different bug class: when a link points outside the current
//! origin, the user deserves a clear signal that activation will
//! leave the current site.
//!
//! ## Why this matters
//!
//! 1. **Trust signal** — users decide whether to click based on
//!    where the link goes. Hiding the cross-origin nature
//!    enables phishing-by-mimicry.
//! 2. **Screen-reader announcement** — AT users without visible
//!    chrome rely on accessible-name affordances ("opens in new
//!    window", trailing "(external)", icon with `<title>`) to
//!    know they're leaving.
//! 3. **NCAG 2.4.4 / 2.5.3** — link text + accessible name must
//!    convey link purpose; "click here" or bare-domain anchor
//!    text on a cross-origin link leaves the user uninformed.
//!
//! ## Findings
//!
//! * `link-ext.no-indication` strict — anchor href is
//!   cross-origin AND visible text + accessible name carry no
//!   external-link indicator (no trailing icon with `<title>`,
//!   no "(external)" or "opens in" text, no
//!   `target="_blank"` with `rel="noopener"` advertising new
//!   window).
//! * `link-ext.bare-domain-text` warn — cross-origin anchor's
//!   visible text is exactly the bare domain or full URL of
//!   the target. Surfaces URLs but doesn't say what they
//!   contain.
//!
//! Out of scope:
//!
//! * Color-only distinction without underline / icon — covered
//!   by `link_color_only`.
//! * `target="_blank"` rel + noopener — covered by
//!   `link_target_blank_safety`.
//! * PDF and download links — covered by `pdf_link_indicator`
//!   and `download_attribute_audit`.
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

/// External-indicator tokens we look for in anchor text /
/// accessible name. Word-boundary matched, case-insensitive.
const EXTERNAL_TOKENS: &[&str] =
    &["external", "new window", "new tab", "opens in", "off-site", "offsite"];

/// One captured cross-origin anchor and its observable signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ExternalLinkEntry {
    /// CSS-ish selector pointing at the anchor.
    pub selector: String,
    /// Resolved href.
    pub href: String,
    /// Resolved origin of the anchor's href.
    pub href_origin: String,
    /// Page's own origin.
    pub page_origin: String,
    /// Visible text content of the anchor (trimmed).
    pub visible_text: String,
    /// Accessible name (runner-computed via aria-label /
    /// aria-labelledby / title / image alt / visible text).
    pub accessible_name: String,
    /// Whether the anchor contains a child element (icon /
    /// `<svg>`) whose accessible name contains an external-
    /// link indicator.
    pub has_external_icon_with_label: bool,
    /// `target=` attribute value (typically `_blank` for
    /// new-window links).
    pub target: String,
    /// `rel=` attribute value.
    pub rel: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkInternalExternalIndicationSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every cross-origin `<a href>` anchor on the page.
    pub anchors: Vec<ExternalLinkEntry>,
}

/// Detector.
#[must_use]
pub fn detect_link_internal_external_indication(
    snap: &LinkInternalExternalIndicationSnapshot,
) -> Vec<AxisFinding> {
    let mut no_indication: Vec<String> = Vec::new();
    let mut bare_domain: Vec<String> = Vec::new();

    for entry in &snap.anchors {
        if origins_equal(&entry.href_origin, &entry.page_origin) {
            continue;
        }
        // Cross-origin from here on.

        let visible_lower = entry.visible_text.to_ascii_lowercase();
        let accessible_lower = entry.accessible_name.to_ascii_lowercase();

        let visible_has_token =
            EXTERNAL_TOKENS.iter().any(|t| has_token(&visible_lower, t));
        let accessible_has_token =
            EXTERNAL_TOKENS.iter().any(|t| has_token(&accessible_lower, t));

        let new_window_attrs = entry.target.eq_ignore_ascii_case("_blank")
            && entry
                .rel
                .to_ascii_lowercase()
                .split_whitespace()
                .any(|t| t == "noopener" || t == "noreferrer");

        let has_indicator = visible_has_token
            || accessible_has_token
            || entry.has_external_icon_with_label
            || new_window_attrs;

        if !has_indicator {
            no_indication.push(format!(
                "{} (text=\"{}\", href={})",
                entry.selector, entry.visible_text, entry.href
            ));
        }

        if visible_text_is_bare_domain(&entry.visible_text, &entry.href) {
            bare_domain.push(format!(
                "{} (text=\"{}\", href={})",
                entry.selector, entry.visible_text, entry.href
            ));
        }
    }

    let mut findings = Vec::new();
    let total = snap.anchors.len();

    if !no_indication.is_empty() {
        let preview = preview_examples(
            &no_indication.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "link-ext.no-indication".to_owned(),
            detail: format!(
                "{} of {} cross-origin anchor(s) carry no external-link affordance (no token, no icon, no _blank+noopener). Examples: {}",
                no_indication.len(),
                total,
                preview
            ),
        });
    }

    if !bare_domain.is_empty() {
        let preview = preview_examples(
            &bare_domain.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "link-ext.bare-domain-text".to_owned(),
            detail: format!(
                "{} of {} cross-origin anchor(s) have visible text that is exactly the URL / bare domain; surfaces URLs but doesn't convey link purpose. Examples: {}",
                bare_domain.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn origins_equal(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

fn has_token(haystack: &str, needle: &str) -> bool {
    if needle.contains(' ') {
        // Multi-word phrases: substring is fine because
        // operator can't accidentally embed inside another word.
        return haystack.contains(needle);
    }
    let bytes = haystack.as_bytes();
    let nb = needle.as_bytes();
    if bytes.len() < nb.len() {
        return false;
    }
    let mut i = 0;
    while i + nb.len() <= bytes.len() {
        if &bytes[i..i + nb.len()] == nb {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after_idx = i + nb.len();
            let after_ok =
                after_idx == bytes.len() || !bytes[after_idx].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn visible_text_is_bare_domain(visible: &str, href: &str) -> bool {
    let v = visible.trim();
    if v.is_empty() {
        return false;
    }
    if v.eq_ignore_ascii_case(href.trim()) {
        return true;
    }
    // Strip scheme + path from the href, compare bare host.
    let host = href
        .splitn(2, "://")
        .nth(1)
        .unwrap_or(href)
        .split('/')
        .next()
        .unwrap_or("");
    if host.is_empty() {
        return false;
    }
    v.eq_ignore_ascii_case(host) || v.eq_ignore_ascii_case(host.trim_start_matches("www."))
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

/// Page-side eval. Walks every `<a href>` and captures the
/// signals the detector consumes. Filters to cross-origin
/// candidates so the detector doesn't have to scan internal
/// links.
pub const LINK_INTERNAL_EXTERNAL_INDICATION_JS: &str = r##"(() => {
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

    const accessibleNameFor = function(a) {
      const aria = a.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim();
      const title = a.getAttribute('title');
      if (title && title.trim()) return title.trim();
      return (a.textContent || '').trim();
    };

    const pageOrigin = location.origin;
    const tokens = ['external', 'new window', 'new tab', 'opens in', 'off-site', 'offsite'];
    const hasIconWithExternalLabel = function(a) {
      const candidates = a.querySelectorAll('svg title, img[alt], [aria-label]');
      for (const c of candidates) {
        const t = (c.textContent || c.getAttribute('alt') || c.getAttribute('aria-label') || '').toLowerCase();
        if (tokens.some(function(tok) { return t.indexOf(tok) >= 0; })) return true;
      }
      return false;
    };

    const anchors = Array.from(document.querySelectorAll('a[href]')).map(function(a) {
      const href = a.getAttribute('href') || '';
      let hrefOrigin = '';
      try {
        const u = new URL(href, location.href);
        if (u.protocol === 'http:' || u.protocol === 'https:') {
          hrefOrigin = u.origin;
        }
      } catch (_) { /* unparseable */ }
      return {
        selector: selectorOf(a),
        href: href,
        hrefOrigin: hrefOrigin,
        pageOrigin: pageOrigin,
        visibleText: (a.textContent || '').trim(),
        accessibleName: accessibleNameFor(a),
        hasExternalIconWithLabel: hasIconWithExternalLabel(a),
        target: a.getAttribute('target') || '',
        rel: a.getAttribute('rel') || ''
      };
    }).filter(function(e) {
      return e.hrefOrigin && e.hrefOrigin !== e.pageOrigin;
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
        href: &str,
        href_origin: &str,
        page_origin: &str,
        visible: &str,
        accessible: &str,
        icon_external: bool,
        target: &str,
        rel: &str,
    ) -> ExternalLinkEntry {
        ExternalLinkEntry {
            selector: sel.to_owned(),
            href: href.to_owned(),
            href_origin: href_origin.to_owned(),
            page_origin: page_origin.to_owned(),
            visible_text: visible.to_owned(),
            accessible_name: accessible.to_owned(),
            has_external_icon_with_label: icon_external,
            target: target.to_owned(),
            rel: rel.to_owned(),
        }
    }

    fn snap(anchors: Vec<ExternalLinkEntry>) -> LinkInternalExternalIndicationSnapshot {
        LinkInternalExternalIndicationSnapshot {
            page_url: "https://example.test/".to_owned(),
            anchors,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_link_internal_external_indication(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn same_origin_anchor_is_ignored() {
        // Runner is supposed to filter same-origin out before
        // building the snapshot. If one slips through, detector
        // ignores it.
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a", "/docs", "https://example.test", "https://example.test",
            "Docs", "Docs", false, "", "",
        )]));
        assert!(f.is_empty(), "same-origin should be ignored: {f:?}");
    }

    #[test]
    fn cross_origin_no_indication_is_strict() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#out",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "Read more",
            "Read more",
            false,
            "",
            "",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-ext.no-indication")
            .expect("no-indication expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("Read more"));
    }

    #[test]
    fn external_token_in_visible_text_passes() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#out",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "Read more (external)",
            "Read more (external)",
            false,
            "",
            "",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "link-ext.no-indication"),
            "'external' token should pass: {f:?}"
        );
    }

    #[test]
    fn opens_in_phrase_in_accessible_name_passes() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#out",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "Read",
            "Read (opens in new window)",
            false,
            "",
            "",
        )]));
        assert!(!f.iter().any(|x| x.kind == "link-ext.no-indication"));
    }

    #[test]
    fn target_blank_with_noopener_passes() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#out",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "Read more",
            "Read more",
            false,
            "_blank",
            "noopener noreferrer",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "link-ext.no-indication"),
            "_blank+noopener should pass: {f:?}"
        );
    }

    #[test]
    fn icon_with_external_label_passes() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#out",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "Read",
            "Read",
            true,
            "",
            "",
        )]));
        assert!(!f.iter().any(|x| x.kind == "link-ext.no-indication"));
    }

    #[test]
    fn bare_url_text_is_warn() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#bare",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "https://other.net/x",
            "https://other.net/x",
            false,
            "_blank",
            "noopener",
        )]));
        // _blank+noopener satisfies no-indication, so only bare-
        // domain warn remains.
        let hit = f
            .iter()
            .find(|x| x.kind == "link-ext.bare-domain-text")
            .expect("bare-domain expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn bare_host_text_matches_with_www_strip() {
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#bare",
            "https://www.other.net/x",
            "https://www.other.net",
            "https://example.test",
            "other.net",
            "other.net",
            false,
            "_blank",
            "noopener",
        )]));
        assert!(f
            .iter()
            .any(|x| x.kind == "link-ext.bare-domain-text"));
    }

    #[test]
    fn word_boundary_excludes_substring_in_word() {
        // "externalia" should NOT satisfy the "external" token.
        let f = detect_link_internal_external_indication(&snap(vec![entry(
            "a#nope",
            "https://other.net/x",
            "https://other.net",
            "https://example.test",
            "Externalia inc",
            "Externalia inc",
            false,
            "",
            "",
        )]));
        assert!(
            f.iter().any(|x| x.kind == "link-ext.no-indication"),
            "'externalia' should not pass as 'external' token"
        );
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let anchors: Vec<_> = (0..8)
            .map(|i| {
                entry(
                    &format!("a#x{i}"),
                    "https://other.net/x",
                    "https://other.net",
                    "https://example.test",
                    "Read more",
                    "Read more",
                    false,
                    "",
                    "",
                )
            })
            .collect();
        let f = detect_link_internal_external_indication(&snap(anchors));
        let hit = f
            .iter()
            .find(|x| x.kind == "link-ext.no-indication")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_anchors() {
        assert!(LINK_INTERNAL_EXTERNAL_INDICATION_JS.starts_with("(() => {"));
        assert!(LINK_INTERNAL_EXTERNAL_INDICATION_JS.ends_with(")()"));
        assert!(LINK_INTERNAL_EXTERNAL_INDICATION_JS.contains("a[href]"));
        assert!(LINK_INTERNAL_EXTERNAL_INDICATION_JS.contains("new URL"));
        assert!(LINK_INTERNAL_EXTERNAL_INDICATION_JS.contains("location.origin"));
    }
}
