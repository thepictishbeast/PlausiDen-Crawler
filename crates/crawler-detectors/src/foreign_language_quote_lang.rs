//! `foreign_language_quote_lang` — flags quotation elements whose
//! body is in a script different from the document language and
//! which carry no `lang=""` attribute (own or inherited from an
//! ancestor different than `<html>`).
//!
//! WCAG 3.1.2 ("Language of Parts", Level AA): "The human
//! language of each passage or phrase in the content can be
//! programmatically determined." A `<blockquote>` of Rilke in
//! German on an English page must declare `lang="de"` or screen
//! readers pronounce it with English phonetics.
//!
//! Defect class: an editorial site quotes a non-English author in
//! the original language (Han / Cyrillic / Arabic / Hebrew /
//! Devanagari / Greek / etc.) without tagging the quote element
//! with the right `lang`. Common causes:
//!
//! 1. Hand-authored HTML pasted from a translated source —
//!    operator forgot the lang.
//! 2. CMS doesn't expose a per-passage lang slot; everything
//!    defaults to the page's html lang.
//! 3. Inline quote `<q>` interpolation in a paragraph; the
//!    author thought the html lang covered everything.
//!
//! ## Heuristic
//!
//! For each `<blockquote>`, `<q>`, and `<cite>` element, the JS
//! computes the distribution of characters across script
//! categories (Han / Arabic / Cyrillic / Hebrew / Devanagari /
//! Greek / Thai / Korean / Japanese-Kana / Latin / Other). The
//! detector flags an element when:
//!
//! * `non_latin_ratio` ≥ `MIN_NON_LATIN_RATIO`
//! * `char_count` ≥ `MIN_CHAR_COUNT`
//! * neither the element nor any ancestor (other than `<html>`
//!   matching the page-level html lang) declares `lang="..."`
//! * the page-level html lang is a Latin-script language
//!   (English / French / German / Spanish / Italian / Portuguese
//!   / Dutch / Swedish / Norwegian / Danish / Finnish / Polish
//!   / Czech / Romanian — see `LATIN_SCRIPT_LANGS`)
//!
//! Severity tiers:
//!
//! * **Strict** — `char_count` ≥ `STRICT_CHAR_COUNT` AND
//!   `non_latin_ratio` ≥ `STRICT_NON_LATIN_RATIO`. Substantial
//!   passage clearly in a different language, screen-reader
//!   pronunciation will be incomprehensible.
//! * **Warn** — meets the minimum thresholds but not the strict
//!   ones. Short inline quote or mixed-script passage.
//!
//! Bidirectional script (Arabic / Hebrew) implies `dir="rtl"` is
//! usually also missing — this detector does NOT flag the dir
//! gap (see `rtl_ltr_mix` for that axis); it stays focused on
//! the lang contract.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — a quotation element in a non-Latin
/// script with no lang declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ForeignQuoteHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Element tag name (`"blockquote"`, `"q"`, `"cite"`).
    pub tag: String,
    /// Visible text (capped at 80 chars) for context.
    pub text: String,
    /// Character count of the visible text (full, not capped).
    pub char_count: u32,
    /// Fraction of `char_count` that fell into a non-Latin
    /// script category (0.0..=1.0).
    pub non_latin_ratio: f32,
    /// The dominant non-Latin script category, lowercased
    /// (`"han"`, `"arabic"`, `"cyrillic"`, `"hebrew"`,
    /// `"devanagari"`, `"greek"`, `"thai"`, `"hangul"`,
    /// `"kana"`, `"other"`).
    pub dominant_script: String,
    /// True iff the element has its own `lang` attribute.
    pub has_own_lang: bool,
    /// Nearest ancestor `lang` value (excluding the `<html>`
    /// root). `None` when no ancestor between the element and
    /// `<html>` declared one.
    pub nearest_ancestor_lang: Option<String>,
    /// The document's `<html lang>` value as-captured.
    pub html_lang: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ForeignQuoteSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Document-level `<html lang>` (empty when absent —
    /// the `html_lang` axis handles that gap, this one stays
    /// silent in that case).
    pub html_lang: String,
    /// Every `<blockquote>` / `<q>` / `<cite>` whose text
    /// crosses the non-Latin-ratio + char-count floors.
    pub hits: Vec<ForeignQuoteHit>,
    /// Total quotation elements walked.
    pub scanned_elements: u32,
}

/// Languages whose primary writing system is Latin-script. The
/// detector only flags non-Latin-script quotes when the page
/// language is one of these — otherwise the page itself is in
/// a non-Latin script and quotes in that same script are fine.
///
/// Conservative list: extend as needed. A page with html lang
/// outside this set produces zero strict findings for ANY
/// quote (silent) — the warn tier still surfaces for visibility.
pub const LATIN_SCRIPT_LANGS: &[&str] = &[
    "en", "fr", "de", "es", "it", "pt", "nl",
    "sv", "no", "nb", "nn", "da", "fi",
    "pl", "cs", "ro", "hu", "hr", "sk", "sl",
    "et", "lv", "lt", "tr", "vi", "id", "ms",
    "ca", "eu", "ga", "cy", "is", "mt",
];

/// Minimum non-Latin character ratio to flag at any severity.
/// Below this we treat the text as essentially Latin-script
/// (proper names, technical terms — WCAG 3.1.2 explicit
/// exceptions).
pub const MIN_NON_LATIN_RATIO: f32 = 0.3;

/// Minimum character count to consider an element. Single words
/// of indeterminate language are WCAG exceptions.
pub const MIN_CHAR_COUNT: u32 = 10;

/// Non-Latin ratio above which the finding is Strict.
pub const STRICT_NON_LATIN_RATIO: f32 = 0.5;

/// Character count above which the finding is Strict.
/// Substantial passages need the lang tag the most.
pub const STRICT_CHAR_COUNT: u32 = 30;

/// Maximum number of examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Returns `true` iff `lang` (BCP-47 code or empty) has a
/// primary subtag in `LATIN_SCRIPT_LANGS`. The check is
/// case-insensitive and only looks at the bytes before the
/// first `-` so `en-US` / `en-GB` / `pt-BR` etc. all resolve.
#[must_use]
pub fn is_latin_script_lang(lang: &str) -> bool {
    if lang.is_empty() {
        return false;
    }
    let primary = lang
        .split(|c: char| c == '-' || c == '_')
        .next()
        .unwrap_or("");
    if primary.is_empty() {
        return false;
    }
    let primary_lower = primary.to_ascii_lowercase();
    LATIN_SCRIPT_LANGS
        .iter()
        .any(|s| *s == primary_lower.as_str())
}

/// Pure detector: snapshot → findings. Strict + warn buckets
/// split per the documented thresholds. Empty hits → empty
/// findings; non-Latin-script page lang → no strict findings
/// (warn tier remains for visibility).
#[must_use]
pub fn detect_foreign_language_quotes(snap: &ForeignQuoteSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let page_is_latin = is_latin_script_lang(&snap.html_lang);

    let mut strict: Vec<&ForeignQuoteHit> = Vec::new();
    let mut warn: Vec<&ForeignQuoteHit> = Vec::new();

    for hit in &snap.hits {
        // Element already has its own lang attribute — operator
        // declared the contract; nothing to flag.
        if hit.has_own_lang {
            continue;
        }
        // An ancestor (between the element and <html>) declared
        // a lang different from the page's. Treat that as the
        // operator's intentional scope; don't flag.
        if let Some(anc) = &hit.nearest_ancestor_lang {
            if anc != &snap.html_lang {
                continue;
            }
        }
        if hit.char_count < MIN_CHAR_COUNT {
            continue;
        }
        if hit.non_latin_ratio < MIN_NON_LATIN_RATIO {
            continue;
        }
        let is_strict_tier = page_is_latin
            && hit.char_count >= STRICT_CHAR_COUNT
            && hit.non_latin_ratio >= STRICT_NON_LATIN_RATIO;
        if is_strict_tier {
            strict.push(hit);
        } else {
            warn.push(hit);
        }
    }

    let mut out = Vec::new();

    if !strict.is_empty() {
        let examples: Vec<String> = strict
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                format!(
                    "{} <{}> ({} chars, {:.0}% {}, \"{}\")",
                    h.selector,
                    h.tag,
                    h.char_count,
                    h.non_latin_ratio * 100.0,
                    h.dominant_script,
                    h.text
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "foreign-language-quote.no-lang".to_owned(),
            detail: format!(
                "{} quotation element(s) carrying substantial non-Latin script content on a Latin-script page (html lang={:?}) with no `lang` attribute — screen readers will pronounce with the wrong phonetics. Add `lang=\"<bcp47>\"` (e.g. `lang=\"zh\"`, `lang=\"ar\"`) to the element or its nearest non-html ancestor. Examples: {}",
                strict.len(),
                snap.html_lang,
                examples.join("; ")
            ),
        });
    }

    if !warn.is_empty() {
        let examples: Vec<String> = warn
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| {
                format!(
                    "{} <{}> ({} chars, {:.0}% {}, \"{}\")",
                    h.selector,
                    h.tag,
                    h.char_count,
                    h.non_latin_ratio * 100.0,
                    h.dominant_script,
                    h.text
                )
            })
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "foreign-language-quote.partial-or-short".to_owned(),
            detail: format!(
                "{} quotation element(s) mixing scripts or short non-Latin passages with no `lang` attribute. Short quotes are WCAG exceptions when the words have become part of the surrounding vernacular; flag for audit. Examples: {}",
                warn.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

/// Browser-side DOM-capture script. Walks `blockquote`, `q`,
/// `cite` elements; computes per-script-category char ratios
/// and resolves nearest non-`<html>` ancestor lang.
///
/// Mirror any change in this file's `ForeignQuoteHit` /
/// `ForeignQuoteSnapshot` field set.
pub const FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const same = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (same.length > 1) parts.unshift(tag + ':nth-of-type(' + (same.indexOf(node) + 1) + ')');
          else parts.unshift(tag);
        } else parts.unshift(tag);
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };

    // Categorize a single code point into a script bucket.
    const scriptOf = function(cp) {
      if (cp < 0x80) return 'latin';
      // Latin extensions
      if (cp >= 0x0080 && cp <= 0x024F) return 'latin';
      // Greek
      if (cp >= 0x0370 && cp <= 0x03FF) return 'greek';
      // Cyrillic
      if (cp >= 0x0400 && cp <= 0x04FF) return 'cyrillic';
      // Hebrew
      if (cp >= 0x0590 && cp <= 0x05FF) return 'hebrew';
      // Arabic
      if (cp >= 0x0600 && cp <= 0x06FF) return 'arabic';
      if (cp >= 0x0750 && cp <= 0x077F) return 'arabic';
      // Devanagari
      if (cp >= 0x0900 && cp <= 0x097F) return 'devanagari';
      // Thai
      if (cp >= 0x0E00 && cp <= 0x0E7F) return 'thai';
      // Hangul (Korean)
      if (cp >= 0xAC00 && cp <= 0xD7AF) return 'hangul';
      if (cp >= 0x1100 && cp <= 0x11FF) return 'hangul';
      // Kana (Japanese)
      if (cp >= 0x3040 && cp <= 0x30FF) return 'kana';
      // Han (CJK ideographs — used by Chinese / Japanese / Korean)
      if (cp >= 0x4E00 && cp <= 0x9FFF) return 'han';
      if (cp >= 0x3400 && cp <= 0x4DBF) return 'han';
      // Punctuation / digits / whitespace shouldn't tip the verdict.
      if (cp <= 0x40 || (cp >= 0x5B && cp <= 0x60) || (cp >= 0x7B && cp <= 0x7F)) return 'ignored';
      return 'other';
    };

    // Walk up from `el` looking for the nearest ancestor (other
    // than <html>) carrying a `lang` attribute. Returns null
    // when only the root has one (or none does).
    const nearestAncestorLang = function(el) {
      let p = el.parentElement;
      while (p && p !== document.documentElement) {
        if (p.hasAttribute && p.hasAttribute('lang')) {
          return p.getAttribute('lang');
        }
        p = p.parentElement;
      }
      return null;
    };

    const htmlLang = document.documentElement.getAttribute('lang') || '';

    const hits = [];
    let scanned = 0;
    const elements = document.querySelectorAll('blockquote, q, cite');
    for (const el of elements) {
      scanned += 1;
      const text = (el.textContent || '').trim();
      if (text.length === 0) continue;
      const buckets = { latin: 0, han: 0, arabic: 0, cyrillic: 0, hebrew: 0, devanagari: 0, greek: 0, thai: 0, hangul: 0, kana: 0, other: 0 };
      let total = 0;
      for (const ch of text) {
        const cp = ch.codePointAt(0) || 0;
        const s = scriptOf(cp);
        if (s === 'ignored') continue;
        total += 1;
        if (buckets[s] != null) buckets[s] += 1;
        else buckets.other += 1;
      }
      if (total === 0) continue;
      const nonLatinTotal = total - buckets.latin;
      const ratio = nonLatinTotal / total;
      // Find the dominant non-Latin bucket.
      let dominant = 'other';
      let domCount = 0;
      for (const k of Object.keys(buckets)) {
        if (k === 'latin') continue;
        if (buckets[k] > domCount) {
          dominant = k;
          domCount = buckets[k];
        }
      }
      const hasOwnLang = el.hasAttribute && el.hasAttribute('lang');
      const ancLang = nearestAncestorLang(el);
      hits.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        text: text.substring(0, 80),
        charCount: total,
        nonLatinRatio: Math.round(ratio * 1000) / 1000,
        dominantScript: dominant,
        hasOwnLang: hasOwnLang,
        nearestAncestorLang: ancLang,
        htmlLang: htmlLang
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      htmlLang: htmlLang,
      hits: hits,
      scannedElements: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        tag: &str,
        text: &str,
        char_count: u32,
        non_latin_ratio: f32,
        dominant_script: &str,
        has_own_lang: bool,
        ancestor_lang: Option<&str>,
        html_lang: &str,
    ) -> ForeignQuoteHit {
        ForeignQuoteHit {
            selector: selector.into(),
            tag: tag.into(),
            text: text.into(),
            char_count,
            non_latin_ratio,
            dominant_script: dominant_script.into(),
            has_own_lang,
            nearest_ancestor_lang: ancestor_lang.map(str::to_owned),
            html_lang: html_lang.into(),
        }
    }

    fn snap(html_lang: &str, hits: Vec<ForeignQuoteHit>) -> ForeignQuoteSnapshot {
        let scanned = hits.len() as u32;
        ForeignQuoteSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            html_lang: html_lang.into(),
            hits,
            scanned_elements: scanned,
        }
    }

    #[test]
    fn is_latin_script_lang_handles_common_codes() {
        assert!(is_latin_script_lang("en"));
        assert!(is_latin_script_lang("en-US"));
        assert!(is_latin_script_lang("pt-BR"));
        assert!(is_latin_script_lang("fr-CA"));
        assert!(is_latin_script_lang("EN"));
        assert!(!is_latin_script_lang("zh"));
        assert!(!is_latin_script_lang("ja-JP"));
        assert!(!is_latin_script_lang("ar"));
        assert!(!is_latin_script_lang("he"));
        assert!(!is_latin_script_lang(""));
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap("en", vec![]);
        let findings = detect_foreign_language_quotes(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn substantial_han_quote_no_lang_is_strict() {
        let s = snap(
            "en",
            vec![hit(
                ".dispatch > blockquote",
                "blockquote",
                "床前明月光，疑是地上霜。",
                30,
                0.9,
                "han",
                false,
                None,
                "en",
            )],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "foreign-language-quote.no-lang");
        assert!(findings[0].detail.contains("han"));
        assert!(findings[0].detail.contains("\"en\""));
    }

    #[test]
    fn element_with_own_lang_skipped() {
        // Same text but the element carries its own lang attr.
        let s = snap(
            "en",
            vec![hit(
                ".q",
                "blockquote",
                "床前明月光，疑是地上霜。",
                30,
                0.9,
                "han",
                true,
                None,
                "en",
            )],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn ancestor_lang_different_from_page_skips() {
        // Operator declared lang at a parent (e.g. an entire
        // <article lang="zh">) different from the html root.
        // Element inherits that intentional scope.
        let s = snap(
            "en",
            vec![hit(
                ".q",
                "blockquote",
                "床前明月光，疑是地上霜。",
                30,
                0.9,
                "han",
                false,
                Some("zh"),
                "en",
            )],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn short_quote_below_min_chars_skipped() {
        let s = snap(
            "en",
            vec![hit(".q", "q", "明月", 2, 1.0, "han", false, None, "en")],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn mostly_latin_text_below_min_ratio_skipped() {
        let s = snap(
            "en",
            vec![hit(
                ".q",
                "blockquote",
                "The poet wrote: 床前明月光",
                25,
                0.2,
                "han",
                false,
                None,
                "en",
            )],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn mid_severity_short_or_partial_is_warn() {
        // Above min thresholds but below strict thresholds:
        // 12-char Cyrillic at 40% ratio — warn tier.
        let s = snap(
            "en",
            vec![hit(
                ".q",
                "q",
                "тестовая",
                12,
                0.4,
                "cyrillic",
                false,
                None,
                "en",
            )],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(
            findings[0].kind,
            "foreign-language-quote.partial-or-short"
        );
    }

    #[test]
    fn non_latin_page_lang_does_not_emit_strict_findings() {
        // Page is itself in Chinese (zh) — same-script quotes
        // are NOT WCAG 3.1.2 violations. The strict bucket
        // should be silent (warn tier may still surface
        // anything above the min thresholds for visibility).
        let s = snap(
            "zh",
            vec![hit(
                ".q",
                "blockquote",
                "床前明月光，疑是地上霜。",
                30,
                0.9,
                "han",
                false,
                None,
                "zh",
            )],
        );
        let findings = detect_foreign_language_quotes(&s);
        assert!(
            findings
                .iter()
                .all(|f| f.severity != AxisSeverity::Strict),
            "non-Latin page lang must not emit Strict findings"
        );
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".q-{i}"),
                "blockquote",
                "床前明月光，疑是地上霜。",
                30,
                0.9,
                "han",
                false,
                None,
                "en",
            ));
        }
        let s = snap("en", hits);
        let findings = detect_foreign_language_quotes(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 quotation element(s)"));
        // 5 examples joined by "; " → 4 semicolons in examples.
        // Body message also contains semicolons in the example
        // strings themselves (text includes Chinese full-stop
        // 。 which is not `;`), so we count `; ` separators
        // specifically.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("htmlLang"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("hits"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("scannedElements"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("dominantScript"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("nonLatinRatio"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("hasOwnLang"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("nearestAncestorLang"));
        // Selector contract — blockquote / q / cite.
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("'blockquote, q, cite'"));
        // Han / Arabic / Cyrillic / Hebrew script ranges present.
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("0x4E00"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("0x0600"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("0x0400"));
        assert!(FOREIGN_LANGUAGE_QUOTE_DOM_CAPTURE_JS.contains("0x0590"));
    }
}
