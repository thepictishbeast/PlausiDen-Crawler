//! `pdf_link_indicator` — PDF-link affordance audit.
//!
//! Sibling axis to `link_target_blank_safety` (which checks
//! `target="_blank"` security + a11y), to `link_text` (which
//! checks anchor-text quality), and to `placeholder_alt` (which
//! checks `<img alt>` defaults). This detector audits a third
//! affordance: when a link points at a PDF (or any non-HTML
//! download), the user deserves a visible signal that activating
//! the link will leave HTML behind and open a PDF reader / start
//! a download.
//!
//! ## The bug class
//!
//! Three observable failures:
//!
//! 1. **No visible PDF indicator** — link text reads
//!    `"Annual report"` with no `"(PDF)"` suffix, no icon, no
//!    leading badge. Sighted users only discover the file type
//!    after clicking. Screen-reader users have nothing in the
//!    accessible name to warn them either.
//! 2. **Icon-only indicator with no accessible name** — link
//!    text shows a generic SVG file icon but the icon has no
//!    `<title>` / `aria-label` / sr-only text. Screen-reader
//!    users hear the link text without the PDF signal.
//! 3. **PDF text claim with no actual PDF target** — link text
//!    says `"(PDF)"` but the href points at HTML. Operator left
//!    a stale label behind after migrating the resource. Sets
//!    user expectations incorrectly.
//!
//! ## Findings
//!
//! * `pdf-link.no-visible-indicator` strict — anchor href ends
//!   in `.pdf` (case-insensitive) AND the anchor text + accessible
//!   name contain no PDF token (`pdf` / `PDF` / `(PDF)` /
//!   `[pdf]`).
//! * `pdf-link.icon-only-without-accessible-name` strict —
//!   anchor's visible text is empty (icon-only) AND the
//!   accessible name carries no PDF token.
//! * `pdf-link.stale-pdf-claim` warn — anchor text contains a
//!   PDF token BUT href does NOT end in `.pdf` and does not
//!   carry `application/pdf` content-type (runner-supplied).
//!
//! Out of scope:
//!
//! * Other non-HTML downloads (.zip, .docx, .csv) — each gets
//!   its own future axis (`download_attribute_audit`).
//! * Content-type sniffing on the response body — the detector
//!   trusts the runner-supplied `content_type` field when
//!   present, and falls back to URL extension matching when not.
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

/// One captured anchor and its observable affordances.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AnchorEntry {
    /// CSS-ish selector pointing at the anchor.
    pub selector: String,
    /// `href=` attribute value (absolute URL when the runner
    /// resolved it; otherwise as-authored).
    pub href: String,
    /// Visible text content of the anchor (trimmed).
    pub visible_text: String,
    /// Accessible name as computed by the runner (typically
    /// from aria-label / aria-labelledby / image alt / title /
    /// visible text per ARIA name-computation).
    pub accessible_name: String,
    /// Optional `Content-Type` response header captured by the
    /// runner. `None` when the runner did not perform a HEAD
    /// request; the detector falls back to URL extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PdfLinkIndicatorSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every anchor on the page (or a runner-selected subset).
    pub anchors: Vec<AnchorEntry>,
}

/// Detector. Buckets findings by defect kind.
#[must_use]
pub fn detect_pdf_link_indicator(
    snap: &PdfLinkIndicatorSnapshot,
) -> Vec<AxisFinding> {
    let mut no_visible: Vec<String> = Vec::new();
    let mut icon_only: Vec<String> = Vec::new();
    let mut stale_claim: Vec<String> = Vec::new();

    for anchor in &snap.anchors {
        let target_is_pdf = is_pdf_target(anchor);
        let visible_has_pdf = text_has_pdf_token(&anchor.visible_text);
        let accessible_has_pdf = text_has_pdf_token(&anchor.accessible_name);
        let visible_empty = anchor.visible_text.trim().is_empty();

        if target_is_pdf {
            // Real PDF target — make sure the link advertises it.
            if !visible_has_pdf && !accessible_has_pdf {
                if visible_empty {
                    icon_only.push(format!(
                        "{} (href={}, accessible_name=\"{}\")",
                        anchor.selector, anchor.href, anchor.accessible_name
                    ));
                } else {
                    no_visible.push(format!(
                        "{} (text=\"{}\", href={})",
                        anchor.selector, anchor.visible_text, anchor.href
                    ));
                }
            }
        } else if visible_has_pdf || accessible_has_pdf {
            // PDF token claim but target is NOT a PDF.
            stale_claim.push(format!(
                "{} (text=\"{}\", href={})",
                anchor.selector, anchor.visible_text, anchor.href
            ));
        }
    }

    let mut findings = Vec::new();
    let total_anchors = snap.anchors.len();

    if !no_visible.is_empty() {
        let preview =
            preview_examples(&no_visible.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "pdf-link.no-visible-indicator".to_owned(),
            detail: format!(
                "{} of {} anchor(s) point at a PDF but their visible text + accessible name carry no PDF token. Examples: {}",
                no_visible.len(),
                total_anchors,
                preview
            ),
        });
    }

    if !icon_only.is_empty() {
        let preview =
            preview_examples(&icon_only.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "pdf-link.icon-only-without-accessible-name".to_owned(),
            detail: format!(
                "{} of {} anchor(s) are icon-only (empty visible text) PDF links without a PDF token in their accessible name. Examples: {}",
                icon_only.len(),
                total_anchors,
                preview
            ),
        });
    }

    if !stale_claim.is_empty() {
        let preview =
            preview_examples(&stale_claim.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "pdf-link.stale-pdf-claim".to_owned(),
            detail: format!(
                "{} of {} anchor(s) advertise PDF in their text but point at a non-PDF target. Examples: {}",
                stale_claim.len(),
                total_anchors,
                preview
            ),
        });
    }

    findings
}

fn is_pdf_target(anchor: &AnchorEntry) -> bool {
    if let Some(ct) = anchor.content_type.as_deref() {
        let lower = ct.to_ascii_lowercase();
        if lower.contains("application/pdf") {
            return true;
        }
    }
    href_ends_in_pdf(&anchor.href)
}

fn href_ends_in_pdf(href: &str) -> bool {
    // Strip query string + fragment, then check extension.
    let no_frag = href.split('#').next().unwrap_or(href);
    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
    no_query.to_ascii_lowercase().ends_with(".pdf")
}

fn text_has_pdf_token(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Require "pdf" as a token boundary to avoid false positives
    // (e.g. "podcasting" doesn't match because we check that the
    // characters around the substring aren't ASCII alphanumeric).
    let needle = "pdf";
    let bytes = lower.as_bytes();
    let needle_bytes = needle.as_bytes();
    if bytes.len() < needle_bytes.len() {
        return false;
    }
    let mut i = 0;
    while i + needle_bytes.len() <= bytes.len() {
        if &bytes[i..i + needle_bytes.len()] == needle_bytes {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after_idx = i + needle_bytes.len();
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

/// Page-side eval. Walks every anchor with `href`, captures
/// visible text + accessible-name computation, returns the
/// anchor list to the detector. (Content-type is the runner's
/// responsibility — JS evaluation cannot HEAD third-party URLs.)
pub const PDF_LINK_INDICATOR_JS: &str = r##"(() => {
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
      const labelledby = a.getAttribute('aria-labelledby');
      if (labelledby) {
        const parts = labelledby.split(/\s+/).map(function(id) {
          const t = document.getElementById(id);
          return t ? (t.textContent || '').trim() : '';
        }).filter(function(s) { return s.length > 0; });
        if (parts.length) return parts.join(' ');
      }
      const title = a.getAttribute('title');
      if (title && title.trim()) return title.trim();
      const img = a.querySelector('img[alt]');
      if (img) {
        const alt = img.getAttribute('alt');
        if (alt && alt.trim()) return alt.trim();
      }
      const svgTitle = a.querySelector('svg title');
      if (svgTitle) {
        const t = (svgTitle.textContent || '').trim();
        if (t) return t;
      }
      return (a.textContent || '').trim();
    };

    const anchors = Array.from(document.querySelectorAll('a[href]')).map(function(a) {
      return {
        selector: selectorOf(a),
        href: a.getAttribute('href') || '',
        visibleText: (a.textContent || '').trim(),
        accessibleName: accessibleNameFor(a)
        // content_type is runner-supplied; this snapshot returns
        // null and the runner is expected to enrich.
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

    fn anchor(
        sel: &str,
        href: &str,
        visible: &str,
        accessible: &str,
        ct: Option<&str>,
    ) -> AnchorEntry {
        AnchorEntry {
            selector: sel.to_owned(),
            href: href.to_owned(),
            visible_text: visible.to_owned(),
            accessible_name: accessible.to_owned(),
            content_type: ct.map(str::to_owned),
        }
    }

    fn snap(anchors: Vec<AnchorEntry>) -> PdfLinkIndicatorSnapshot {
        PdfLinkIndicatorSnapshot {
            page_url: "https://example.test/".to_owned(),
            anchors,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_pdf_link_indicator(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn pdf_link_with_pdf_token_in_visible_text_is_clean() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#report",
            "/annual-report.pdf",
            "Annual report (PDF)",
            "Annual report (PDF)",
            None,
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn pdf_link_with_token_only_in_accessible_name_is_clean() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#download",
            "/report.pdf",
            "Annual report",
            "Annual report (PDF)",
            None,
        )]));
        assert!(f.is_empty(), "accessible-name token should pass: {f:?}");
    }

    #[test]
    fn pdf_link_without_indicator_is_strict() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#report",
            "/annual-report.pdf",
            "Annual report",
            "Annual report",
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "pdf-link.no-visible-indicator")
            .expect("no-visible-indicator expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("a#report"));
        assert!(hit.detail.contains("Annual report"));
    }

    #[test]
    fn icon_only_pdf_link_without_accessible_token_is_strict() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a.download-icon",
            "/report.pdf",
            "", // icon-only
            "Download",
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "pdf-link.icon-only-without-accessible-name")
            .expect("icon-only expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        // Should NOT also be reported as no-visible-indicator
        // (icon-only is a separate, more specific finding).
        assert!(!f
            .iter()
            .any(|x| x.kind == "pdf-link.no-visible-indicator"));
    }

    #[test]
    fn content_type_application_pdf_overrides_url_extension() {
        // Href doesn't look like a PDF but content-type says so.
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#deferred",
            "/api/documents/42",
            "Annual report",
            "Annual report",
            Some("application/pdf"),
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "pdf-link.no-visible-indicator")
            .expect("content-type triggers detection");
        assert!(hit.detail.contains("a#deferred"));
    }

    #[test]
    fn stale_pdf_claim_is_warn() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#stale",
            "/annual-report",
            "Annual report (PDF)",
            "Annual report (PDF)",
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "pdf-link.stale-pdf-claim")
            .expect("stale claim expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("a#stale"));
    }

    #[test]
    fn pdf_token_match_respects_word_boundaries() {
        // "podcasting" should NOT match the pdf token.
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#pod",
            "/episode.pdf",
            "Latest podcasting episode",
            "Latest podcasting episode",
            None,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "pdf-link.no-visible-indicator")
            .expect("word boundary should not match 'podcasting'");
        assert!(hit.detail.contains("a#pod"));
    }

    #[test]
    fn href_with_query_string_still_treated_as_pdf() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#qs",
            "/report.pdf?download=1",
            "Annual report",
            "Annual report",
            None,
        )]));
        assert!(f.iter().any(|x| x.kind == "pdf-link.no-visible-indicator"));
    }

    #[test]
    fn href_with_fragment_still_treated_as_pdf() {
        let f = detect_pdf_link_indicator(&snap(vec![anchor(
            "a#frag",
            "/report.pdf#page=4",
            "Annual report",
            "Annual report",
            None,
        )]));
        assert!(f.iter().any(|x| x.kind == "pdf-link.no-visible-indicator"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let anchors: Vec<_> = (0..8)
            .map(|i| {
                anchor(
                    &format!("a#r{i}"),
                    "/report.pdf",
                    "Annual report",
                    "Annual report",
                    None,
                )
            })
            .collect();
        let f = detect_pdf_link_indicator(&snap(anchors));
        let hit = f
            .iter()
            .find(|x| x.kind == "pdf-link.no-visible-indicator")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_anchors() {
        assert!(PDF_LINK_INDICATOR_JS.starts_with("(() => {"));
        assert!(PDF_LINK_INDICATOR_JS.ends_with(")()"));
        assert!(PDF_LINK_INDICATOR_JS.contains("a[href]"));
        assert!(PDF_LINK_INDICATOR_JS.contains("aria-label"));
        assert!(PDF_LINK_INDICATOR_JS.contains("svg title"));
    }
}
