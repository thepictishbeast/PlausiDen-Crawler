//! `meta_charset_position` — `<meta charset>` byte-position
//! audit.
//!
//! Sibling to `doctype_charset` (which audits whether the
//! charset is declared at all), `viewport_meta`,
//! `meta_description`, and `meta_color_scheme`. This detector
//! covers a different invariant: the HTML living standard
//! requires `<meta charset>` to appear in the FIRST 1024
//! BYTES of the document, otherwise the browser may have
//! already decoded preceding bytes with the wrong encoding
//! (and won't re-parse).
//!
//! ## The contract
//!
//! Per HTML Living Standard §4.2.5.4 ("Character encoding
//! declaration"):
//!
//! > The element containing the character encoding declaration
//! > must be serialized completely within the first 1024 bytes
//! > of the document.
//!
//! Operators ship `<meta charset="utf-8">` AFTER large
//! `<head>` blocks (`<title>` with locale text, multi-line
//! `<meta name="description">`, `<link rel="preload">` lists,
//! `<style>` inline blocks) and the charset declaration slips
//! past 1024 bytes. The browser has already started decoding
//! with its guess (typically Windows-1252 outside East Asian
//! locales) and the page renders mojibake.
//!
//! ## Findings
//!
//! * `meta-charset.beyond-1024-bytes` strict — `<meta
//!   charset>` element starts at or after byte 1024 of the
//!   document.
//! * `meta-charset.missing` strict — no `<meta charset>` and
//!   no `<meta http-equiv="Content-Type">` declaration. (We
//!   don't double-report when `doctype_charset` already
//!   captures the missing case — but runners that don't run
//!   that axis get this fallback strict finding.)
//! * `meta-charset.duplicate` warn — 2+ `<meta charset>`
//!   declarations; spec says only the first applies.
//!
//! Out of scope:
//!
//! * Whether the declared charset matches the actual response
//!   `Content-Type` header — covered by `doctype_charset`.
//! * `<meta http-equiv="Content-Type">` variant — same byte-
//!   position rule applies; the runner is expected to capture
//!   either form into `byte_offset`.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; the JS const is the only side-
//!   effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

const SPEC_BYTE_LIMIT: usize = 1024;

/// One captured charset declaration with its byte position.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CharsetEntry {
    /// CSS-ish selector pointing at the meta element.
    pub selector: String,
    /// Byte offset of the meta element's opening `<` within
    /// the raw HTML response body.
    pub byte_offset: usize,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MetaCharsetPositionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<meta charset>` or `<meta http-equiv="Content-
    /// Type">` declaration the runner found.
    pub entries: Vec<CharsetEntry>,
}

/// Detector.
#[must_use]
pub fn detect_meta_charset_position(
    snap: &MetaCharsetPositionSnapshot,
) -> Vec<AxisFinding> {
    if snap.entries.is_empty() {
        return vec![AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "meta-charset.missing".to_owned(),
            detail: "Page has no <meta charset> or <meta http-equiv=\"Content-Type\"> declaration; browsers fall back to UA-default encoding which may differ from response Content-Type header.".to_owned(),
        }];
    }

    let mut findings = Vec::new();

    let first = &snap.entries[0];
    if first.byte_offset >= SPEC_BYTE_LIMIT {
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "meta-charset.beyond-1024-bytes".to_owned(),
            detail: format!(
                "<meta charset> starts at byte offset {} (HTML spec requires < {}); browser may have already decoded preceding bytes with its guessed encoding. Offending element: {}",
                first.byte_offset, SPEC_BYTE_LIMIT, first.selector
            ),
        });
    }

    if snap.entries.len() >= 2 {
        let selectors: Vec<&str> =
            snap.entries.iter().map(|e| e.selector.as_str()).collect();
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "meta-charset.duplicate".to_owned(),
            detail: format!(
                "{} <meta charset> declarations on the page; per spec only the first applies. Elements: {}",
                snap.entries.len(),
                selectors.join(", ")
            ),
        });
    }

    findings
}

/// Page-side eval is more complex than other axes because we
/// need byte offsets within the raw HTML source. The runner is
/// expected to compute byte offsets by either re-fetching the
/// raw HTML or instrumenting the loader. This JS const helps
/// the runner identify which meta tags it needs offsets for.
pub const META_CHARSET_POSITION_JS: &str = r##"(() => {
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

    // Find both forms of charset declaration. The runner is
    // responsible for computing byteOffset from the raw HTML;
    // this evaluator emits null offsets that the runner fills
    // in via re-fetch / instrumented load.
    const charsets = Array.from(document.querySelectorAll('head meta[charset], head meta[http-equiv="Content-Type" i]'));
    const entries = charsets.map(function(m) {
      return {
        selector: selectorOf(m),
        byteOffset: null
      };
    });

    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sel: &str, offset: usize) -> CharsetEntry {
        CharsetEntry {
            selector: sel.to_owned(),
            byte_offset: offset,
        }
    }

    fn snap(entries: Vec<CharsetEntry>) -> MetaCharsetPositionSnapshot {
        MetaCharsetPositionSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_missing_finding() {
        let f = detect_meta_charset_position(&snap(vec![]));
        let hit = f
            .iter()
            .find(|x| x.kind == "meta-charset.missing")
            .expect("missing finding expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn charset_in_first_kilobyte_is_clean() {
        let f = detect_meta_charset_position(&snap(vec![entry(
            "head > meta", 100,
        )]));
        assert!(f.is_empty(), "early charset should pass: {f:?}");
    }

    #[test]
    fn charset_at_byte_1023_is_clean() {
        // Right at the boundary — spec says MUST be SERIALIZED
        // COMPLETELY within first 1024 bytes; the start offset
        // at 1023 means at most 1 byte fits (so the close `>`
        // is past 1024, but our axis flags only start position).
        // We treat 1023 as clean — the conservative "beyond"
        // threshold is `>=` SPEC_BYTE_LIMIT (1024).
        let f = detect_meta_charset_position(&snap(vec![entry(
            "head > meta", 1023,
        )]));
        assert!(f.is_empty(), "1023 should pass: {f:?}");
    }

    #[test]
    fn charset_at_byte_1024_is_strict() {
        let f = detect_meta_charset_position(&snap(vec![entry(
            "head > meta:nth-of-type(5)", 1024,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "meta-charset.beyond-1024-bytes")
            .expect("beyond-1024 expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("byte offset 1024"));
    }

    #[test]
    fn charset_far_beyond_limit_is_strict() {
        let f = detect_meta_charset_position(&snap(vec![entry(
            "head > meta", 4096,
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "meta-charset.beyond-1024-bytes")
            .expect("beyond-1024 expected");
        assert!(hit.detail.contains("4096"));
    }

    #[test]
    fn duplicate_charset_is_warn() {
        let f = detect_meta_charset_position(&snap(vec![
            entry("head > meta:nth-of-type(1)", 50),
            entry("head > meta:nth-of-type(2)", 80),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "meta-charset.duplicate")
            .expect("duplicate expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("2 <meta charset>"));
    }

    #[test]
    fn three_charsets_aggregated_into_single_warn() {
        let f = detect_meta_charset_position(&snap(vec![
            entry("a", 50),
            entry("b", 80),
            entry("c", 110),
        ]));
        let hit = f
            .iter()
            .find(|x| x.kind == "meta-charset.duplicate")
            .unwrap();
        assert!(hit.detail.contains("3 <meta charset>"));
    }

    #[test]
    fn beyond_1024_and_duplicate_both_fire() {
        // First entry at 1500 + second entry — both findings.
        let f = detect_meta_charset_position(&snap(vec![
            entry("a", 1500),
            entry("b", 1600),
        ]));
        assert!(f
            .iter()
            .any(|x| x.kind == "meta-charset.beyond-1024-bytes"));
        assert!(f.iter().any(|x| x.kind == "meta-charset.duplicate"));
    }

    #[test]
    fn single_charset_does_not_flag_duplicate() {
        let f = detect_meta_charset_position(&snap(vec![entry(
            "head > meta", 50,
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "meta-charset.duplicate"),
            "single charset should not flag dup: {f:?}"
        );
    }

    #[test]
    fn js_const_is_iife_and_collects_charset_metas() {
        assert!(META_CHARSET_POSITION_JS.starts_with("(() => {"));
        assert!(META_CHARSET_POSITION_JS.ends_with(")()"));
        assert!(META_CHARSET_POSITION_JS.contains("meta[charset]"));
        assert!(META_CHARSET_POSITION_JS.contains("Content-Type"));
    }
}
