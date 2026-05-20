//! `webfont_preload_audit` — preload-as-font correctness detector.
//!
//! Forward step on rolling Crawler axes (#117). Pairs with
//! `font_loading` / `render_blocking_resources` / `image_dimensions`
//! — the first-paint / LCP performance dimension.
//!
//! ## The bug class
//!
//! `<link rel="preload" as="font" href="...">` tells the browser
//! to fetch the font ASAP, alongside the HTML parse. When done
//! correctly, this gets the font bytes into the cache before the
//! browser would naturally discover them (via CSS `@font-face` +
//! used-glyph match), preventing the FOUT (Flash of Unstyled Text)
//! cascade.
//!
//! When done WRONG, it's either:
//!
//! 1. **Missing `crossorigin` attribute.** Per HTML spec, preload
//!    requests for `as="font"` are CORS-mode REQUESTS. Without
//!    `crossorigin`, the browser issues an anonymous-mode preload
//!    AND a separate CORS-mode actual fetch — the preload bytes
//!    are discarded + the font effectively double-fetches.
//!    Real perf cost: 100-400ms LCP regression depending on
//!    network.
//! 2. **Missing `type=` attribute.** Without a MIME type, older
//!    browsers may skip the preload entirely.
//! 3. **Preload that doesn't actually get used** (font@font-face
//!    src URL doesn't match the preload href). The preload is
//!    pure waste — bandwidth + cache pressure for nothing.
//!
//! ## Findings
//!
//! * `webfont-preload.missing-crossorigin` strict — preload-as-
//!   font without a `crossorigin` attribute. Browser double-fetches
//!   the font; the LCP regression is real + measurable.
//! * `webfont-preload.missing-type`        warn   — no `type=`
//!   attribute. Most modern browsers cope; some skip the preload.
//!   Warn-only since modern browsers (which is what real users
//!   have) usually handle it.
//! * `webfont-preload.suspicious-extension` warn  — `href` doesn't
//!   end in a known webfont extension (.woff2 / .woff / .ttf /
//!   .otf). Likely operator mis-pasted the URL; verify the font
//!   actually loads.
//!
//! Out of scope:
//!
//! * Mismatch detection between the preload href + a `@font-face`
//!   src URL — that requires CSS parsing, which Crawler does at
//!   runtime via `css_health` for other dimensions; defer to a
//!   future combined detector.
//! * Subset audit (preloading a 200KB Latin-1 font when the page
//!   only uses Cyrillic) — needs corpus + font-format introspection.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; JS const is the only side-effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured `<link rel="preload" as="font">` entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct WebfontPreloadEntry {
    /// CSS-ish selector pointing at the link element.
    pub selector: String,
    /// `href` attribute value (capped at 200 chars for context).
    pub href: String,
    /// Whether the element has a `crossorigin` attribute. The
    /// VALUE of crossorigin (anonymous / use-credentials) is
    /// fine either way; presence is what matters for font preload.
    pub has_crossorigin: bool,
    /// `type` attribute value if present.
    pub type_attr: Option<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct WebfontPreloadSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every captured `<link rel="preload" as="font">`.
    pub entries: Vec<WebfontPreloadEntry>,
}

/// Page-side eval. Walks every `<link rel~="preload">` whose
/// `as` attribute is "font" (case-insensitive). Captures the
/// href + crossorigin presence + type attribute.
pub const WEBFONT_PRELOAD_AUDIT_JS: &str = r##"(() => {
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
      return 'head > ' + parts.join(' > ');
    };
    const truncate = function(s) { return (s || '').slice(0, 200); };
    const entries = [];
    const links = document.querySelectorAll('link[rel~="preload"][as="font" i]');
    for (let i = 0; i < links.length; i++) {
      const el = links[i];
      entries.push({
        selector: selectorOf(el),
        href: truncate(el.getAttribute('href') || ''),
        hasCrossorigin: el.hasAttribute('crossorigin'),
        typeAttr: el.getAttribute('type')
      });
    }
    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

/// Known webfont MIME types + filename extensions. Recognized as
/// "real font URL" for the suspicious-extension check.
const WEBFONT_EXTENSIONS: &[&str] = &[".woff2", ".woff", ".ttf", ".otf"];

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_webfont_preload(snap: &WebfontPreloadSnapshot) -> Vec<AxisFinding> {
    let mut findings = Vec::new();
    for entry in &snap.entries {
        // Crossorigin check — most important; LCP regression.
        if !entry.has_crossorigin {
            findings.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "webfont-preload.missing-crossorigin".to_owned(),
                detail: format!(
                    "{} <link rel=\"preload\" as=\"font\" href=\"{}\"> is missing the `crossorigin` attribute. Per HTML spec, font preloads are CORS-mode by default; without the attr the browser anonymous-preloads + then double-fetches in CORS mode for the actual `@font-face` resolution. Add `crossorigin` (or `crossorigin=\"anonymous\"` for same-origin same-credentials defaults).",
                    entry.selector, entry.href
                ),
            });
        }
        // Type check — warn-only since modern browsers cope.
        if entry.type_attr.as_deref().is_none() {
            findings.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "webfont-preload.missing-type".to_owned(),
                detail: format!(
                    "{} <link rel=\"preload\" as=\"font\" href=\"{}\"> is missing the `type` attribute (e.g. `type=\"font/woff2\"`). Most modern browsers cope; some older ones skip the preload entirely without it.",
                    entry.selector, entry.href
                ),
            });
        }
        // Suspicious extension — warn-only since href could be a
        // path-style URL without an extension (e.g.
        // `/fonts/inter` with a server-side rewrite).
        let lower = entry.href.to_lowercase();
        let recognized = WEBFONT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext));
        if !recognized && !entry.href.is_empty() {
            findings.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "webfont-preload.suspicious-extension".to_owned(),
                detail: format!(
                    "{} <link rel=\"preload\" as=\"font\" href=\"{}\"> — href doesn't end in a known webfont extension (.woff2 / .woff / .ttf / .otf). If the URL is server-rewritten this is fine; if the operator mis-pasted, the preload is wasted bandwidth.",
                    entry.selector, entry.href
                ),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        href: &str,
        has_crossorigin: bool,
        type_attr: Option<&str>,
    ) -> WebfontPreloadEntry {
        WebfontPreloadEntry {
            selector: format!("head > link[href=\"{href}\"]"),
            href: href.to_owned(),
            has_crossorigin,
            type_attr: type_attr.map(str::to_owned),
        }
    }

    fn snap(entries: Vec<WebfontPreloadEntry>) -> WebfontPreloadSnapshot {
        WebfontPreloadSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_entries_no_findings() {
        assert!(detect_webfont_preload(&snap(Vec::new())).is_empty());
    }

    #[test]
    fn fully_correct_preload_is_silent() {
        let findings = detect_webfont_preload(&snap(vec![entry(
            "/fonts/inter-var.woff2",
            true,
            Some("font/woff2"),
        )]));
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_crossorigin_is_strict() {
        let findings = detect_webfont_preload(&snap(vec![entry(
            "/fonts/inter-var.woff2",
            false,
            Some("font/woff2"),
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "webfont-preload.missing-crossorigin");
        assert!(findings[0].detail.contains("double-fetches"));
    }

    #[test]
    fn missing_type_is_warn() {
        let findings = detect_webfont_preload(&snap(vec![entry(
            "/fonts/inter-var.woff2",
            true,
            None,
        )]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "webfont-preload.missing-type");
    }

    #[test]
    fn suspicious_extension_is_warn() {
        let findings = detect_webfont_preload(&snap(vec![entry(
            "/images/hero.jpg", // Not a font URL!
            true,
            Some("font/woff2"),
        )]));
        // Extension warn + (missing-type doesn't fire since type IS set).
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "webfont-preload.suspicious-extension");
    }

    #[test]
    fn empty_href_does_not_fire_extension_warn() {
        // Empty href — element has no actual URL; phase shouldn't
        // false-fire suspicious-extension. Other checks may fire.
        let findings = detect_webfont_preload(&snap(vec![entry("", true, Some("font/woff2"))]));
        assert!(
            !findings
                .iter()
                .any(|f| f.kind == "webfont-preload.suspicious-extension")
        );
    }

    #[test]
    fn multiple_issues_per_entry_emit_multiple_findings() {
        // Missing crossorigin AND missing type AND suspicious extension.
        let findings = detect_webfont_preload(&snap(vec![entry(
            "/fonts/font-bundle",
            false,
            None,
        )]));
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"webfont-preload.missing-crossorigin"));
        assert!(kinds.contains(&"webfont-preload.missing-type"));
        assert!(kinds.contains(&"webfont-preload.suspicious-extension"));
    }

    #[test]
    fn multiple_entries_emit_one_finding_each() {
        let findings = detect_webfont_preload(&snap(vec![
            entry("/fonts/a.woff2", true, Some("font/woff2")), // fine
            entry("/fonts/b.woff2", false, Some("font/woff2")), // missing co
            entry("/fonts/c.woff2", true, None),               // missing type
        ]));
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].kind, "webfont-preload.missing-crossorigin");
        assert_eq!(findings[1].kind, "webfont-preload.missing-type");
    }

    #[test]
    fn known_extensions_are_recognized() {
        for ext in [".woff2", ".woff", ".ttf", ".otf"] {
            let href = format!("/fonts/font{ext}");
            let findings = detect_webfont_preload(&snap(vec![entry(
                &href,
                true,
                Some("font/woff2"),
            )]));
            assert!(
                !findings
                    .iter()
                    .any(|f| f.kind == "webfont-preload.suspicious-extension"),
                "extension {ext} should be recognized as a font URL"
            );
        }
    }

    #[test]
    fn snapshot_serde_camel_case() {
        let s = snap(vec![entry("/fonts/a.woff2", true, Some("font/woff2"))]);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pageUrl\""));
        assert!(j.contains("\"hasCrossorigin\""));
        assert!(j.contains("\"typeAttr\""));
        let back: WebfontPreloadSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn js_eval_const_walks_preload_links() {
        assert!(WEBFONT_PRELOAD_AUDIT_JS
            .contains("querySelectorAll('link[rel~=\"preload\"][as=\"font\" i]')"));
        assert!(WEBFONT_PRELOAD_AUDIT_JS.contains("hasCrossorigin"));
        assert!(WEBFONT_PRELOAD_AUDIT_JS.contains("typeAttr"));
    }
}
