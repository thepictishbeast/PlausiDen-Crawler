//! `lang_attribute_subtag_format` — BCP 47 audit of `lang=`
//! values across the document.
//!
//! Sibling axis to `html_lang` (which checks the document-level
//! `<html lang>`) and to `foreign_language_quote_lang` (which
//! checks `<q lang>` / `<blockquote lang>` foreign-language
//! quoting). This detector audits the SHAPE of every `lang=`
//! attribute value on every element in the document against
//! RFC 5646 / BCP 47.
//!
//! ## The bug class
//!
//! BCP 47 language tags are structured as:
//!
//! ```text
//! language[-script][-region][-variant…][-extension…][-privateuse]
//! ```
//!
//! Common authoring failures:
//!
//! 1. **Underscore separator** — `lang="en_US"` instead of
//!    `lang="en-US"`. Browsers + AT mostly tolerate underscore
//!    but RFC 5646 mandates hyphen.
//! 2. **Wrong case** — `lang="EN-us"` instead of `lang="en-US"`.
//!    Language subtag MUST be lowercase, region subtag MUST be
//!    uppercase per RFC 5646. UAs normalise but the source is
//!    non-conforming.
//! 3. **Bare region** — `lang="US"`. Region without language
//!    primary subtag is invalid; UAs fall back to UA-default
//!    language inference.
//! 4. **Too-long primary language subtag** — `lang="english"`
//!    instead of `lang="en"`. Subtag MUST be 2-3 chars (ISO 639-1
//!    or ISO 639-2 alpha codes) or a registered extlang.
//! 5. **Empty `lang=""`** — explicit empty value defeats
//!    language inheritance. Per HTML spec, `lang=""` means
//!    "unknown language" which is a valid choice but rarely the
//!    operator's intent.
//!
//! ## Findings
//!
//! * `lang.underscore-separator` strict — `lang=` value uses
//!   `_` instead of `-`.
//! * `lang.wrong-case` warn — primary language subtag not
//!   lowercase OR 2-letter region subtag not uppercase per
//!   RFC 5646 §2.1.1.
//! * `lang.bare-region` strict — value matches the 2-letter
//!   region shape (`US`, `GB`) with no preceding language
//!   subtag.
//! * `lang.primary-subtag-too-long` strict — primary subtag is
//!   > 3 chars (excluding the legacy `i-` / `x-` grandfathered
//!   tags). Operator likely typed a language NAME instead of a
//!   tag.
//! * `lang.empty-value` warn — `lang=""` (explicit empty).
//!
//! Out of scope:
//!
//! * Validating registered IANA subtags (`lang="zz"` — fake
//!   ISO 639-1 code — passes this detector because closed-world
//!   subtag validation requires bundling the IANA registry).
//! * `xml:lang` attribute — handled by future XML axis.
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

/// Legacy 2-letter region codes most often confused for
/// language tags. Used by `lang.bare-region` heuristic.
const COMMON_REGIONS: &[&str] = &[
    "US", "GB", "AU", "CA", "NZ", "IE", "ZA", "FR", "DE", "ES",
    "IT", "JP", "KR", "CN", "TW", "HK", "BR", "MX", "AR", "RU",
];

/// One captured element carrying a `lang=` attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LangAttrEntry {
    /// CSS-ish selector pointing at the host element.
    pub selector: String,
    /// Raw attribute value (not normalised; not trimmed unless
    /// the operator wrote whitespace).
    pub value: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LangAttributeSubtagFormatSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element on the page that carries a `lang=`
    /// attribute, including the root `<html>`.
    pub entries: Vec<LangAttrEntry>,
}

/// Detector.
#[must_use]
pub fn detect_lang_attribute_subtag_format(
    snap: &LangAttributeSubtagFormatSnapshot,
) -> Vec<AxisFinding> {
    let mut underscore: Vec<String> = Vec::new();
    let mut wrong_case: Vec<String> = Vec::new();
    let mut bare_region: Vec<String> = Vec::new();
    let mut too_long: Vec<String> = Vec::new();
    let mut empty_value: Vec<&str> = Vec::new();

    for entry in &snap.entries {
        let raw = entry.value.as_str();
        if raw.trim().is_empty() {
            empty_value.push(entry.selector.as_str());
            continue;
        }

        if raw.contains('_') {
            underscore.push(format!("{} (lang={})", entry.selector, raw));
            continue;
        }

        let trimmed = raw.trim();

        // Bare-region heuristic.
        if COMMON_REGIONS
            .iter()
            .any(|r| trimmed.eq_ignore_ascii_case(r))
        {
            bare_region.push(format!("{} (lang={})", entry.selector, trimmed));
            continue;
        }

        // Split into subtags by hyphen.
        let parts: Vec<&str> = trimmed.split('-').collect();
        let primary = parts.first().copied().unwrap_or("");

        // Grandfathered tags i-* and x-* allow >3-char extensions
        // after the primary subtag.
        let grandfathered = primary.eq_ignore_ascii_case("i")
            || primary.eq_ignore_ascii_case("x");

        if !grandfathered {
            // Primary subtag MUST be 2 or 3 ASCII letters.
            if !is_subtag_letters(primary, 2, 3) {
                too_long.push(format!("{} (lang={})", entry.selector, trimmed));
                continue;
            }
        }

        // Case checks.
        let primary_lower = primary == primary.to_ascii_lowercase();
        let mut case_problems: Vec<String> = Vec::new();
        if !primary_lower {
            case_problems.push(format!("primary subtag '{primary}' should be lowercase"));
        }
        // Second subtag, if 2 ASCII letters, is region per BCP 47
        // and MUST be uppercase. (4-letter is script which is
        // titlecase; we don't validate script here to keep the
        // detector tight.)
        if parts.len() >= 2 {
            let region_candidate = parts[1];
            if is_subtag_letters(region_candidate, 2, 2) {
                let upper = region_candidate == region_candidate.to_ascii_uppercase();
                if !upper {
                    case_problems.push(format!(
                        "region subtag '{region_candidate}' should be uppercase"
                    ));
                }
            }
        }
        if !case_problems.is_empty() {
            wrong_case.push(format!(
                "{} (lang={}; {})",
                entry.selector,
                trimmed,
                case_problems.join(", ")
            ));
        }
    }

    let mut findings = Vec::new();
    let total = snap.entries.len();

    if !underscore.is_empty() {
        let preview = preview_examples(
            &underscore.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "lang.underscore-separator".to_owned(),
            detail: format!(
                "{} of {} lang= attribute(s) use `_` instead of `-` (BCP 47 mandates hyphen). Examples: {}",
                underscore.len(),
                total,
                preview
            ),
        });
    }

    if !bare_region.is_empty() {
        let preview = preview_examples(
            &bare_region.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "lang.bare-region".to_owned(),
            detail: format!(
                "{} of {} lang= attribute(s) carry a bare region code with no language primary subtag. Examples: {}",
                bare_region.len(),
                total,
                preview
            ),
        });
    }

    if !too_long.is_empty() {
        let preview = preview_examples(
            &too_long.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "lang.primary-subtag-too-long".to_owned(),
            detail: format!(
                "{} of {} lang= attribute(s) have a primary subtag that isn't 2-3 ASCII letters (operator likely typed a language name). Examples: {}",
                too_long.len(),
                total,
                preview
            ),
        });
    }

    if !empty_value.is_empty() {
        let preview = preview_examples(&empty_value);
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "lang.empty-value".to_owned(),
            detail: format!(
                "{} of {} lang= attribute(s) carry an empty value; explicit `lang=\"\"` means 'unknown language' which is rarely intentional. Examples: {}",
                empty_value.len(),
                total,
                preview
            ),
        });
    }

    if !wrong_case.is_empty() {
        let preview = preview_examples(
            &wrong_case.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "lang.wrong-case".to_owned(),
            detail: format!(
                "{} of {} lang= attribute(s) use non-canonical case per RFC 5646 §2.1.1 (primary lowercase, region uppercase). Examples: {}",
                wrong_case.len(),
                total,
                preview
            ),
        });
    }

    findings
}

fn is_subtag_letters(s: &str, min: usize, max: usize) -> bool {
    let len = s.chars().count();
    if len < min || len > max {
        return false;
    }
    s.chars().all(|c| c.is_ascii_alphabetic())
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

/// Page-side eval. Captures every element with a `lang=`
/// attribute, including the root `<html>`.
pub const LANG_ATTRIBUTE_SUBTAG_FORMAT_JS: &str = r##"(() => {
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

    const hosts = Array.from(document.querySelectorAll('[lang]'));
    const entries = hosts.map(function(el) {
      return {
        selector: selectorOf(el),
        value: el.getAttribute('lang') || ''
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

    fn entry(sel: &str, value: &str) -> LangAttrEntry {
        LangAttrEntry {
            selector: sel.to_owned(),
            value: value.to_owned(),
        }
    }

    fn snap(entries: Vec<LangAttrEntry>) -> LangAttributeSubtagFormatSnapshot {
        LangAttributeSubtagFormatSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn well_formed_canonical_is_clean() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![
            entry("html", "en"),
            entry("html", "en-US"),
            entry("html", "zh-Hant-TW"),
            entry("html", "fr-CA"),
        ]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn underscore_separator_is_strict() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "en_US",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.underscore-separator")
            .expect("underscore expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("en_US"));
    }

    #[test]
    fn wrong_case_primary_subtag_is_warn() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "EN-US",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.wrong-case")
            .expect("wrong-case expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("primary subtag 'EN'"));
    }

    #[test]
    fn wrong_case_region_subtag_is_warn() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "en-us",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.wrong-case")
            .expect("wrong-case expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("region subtag 'us'"));
    }

    #[test]
    fn bare_region_is_strict() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "US",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.bare-region")
            .expect("bare-region expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
        assert!(hit.detail.contains("US"));
    }

    #[test]
    fn primary_subtag_too_long_is_strict() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "english",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.primary-subtag-too-long")
            .expect("too-long expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn empty_value_is_warn() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry("html", "")]));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.empty-value")
            .expect("empty-value expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn iso_639_2_three_letter_codes_pass() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![
            entry("html", "haw"),
            entry("html", "yue"),
        ]));
        assert!(f.is_empty(), "ISO 639-2 should pass: {f:?}");
    }

    #[test]
    fn fake_two_letter_code_passes_shape_check() {
        // Detector intentionally doesn't validate IANA registry;
        // closed-world subtag validation is out of scope.
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "zz",
        )]));
        assert!(
            f.is_empty(),
            "shape-only validation should let unregistered subtags pass: {f:?}"
        );
    }

    #[test]
    fn grandfathered_i_tag_passes() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "i-klingon",
        )]));
        assert!(f.is_empty(), "i-* grandfathered tag should pass: {f:?}");
    }

    #[test]
    fn grandfathered_x_tag_passes() {
        let f = detect_lang_attribute_subtag_format(&snap(vec![entry(
            "html", "x-private",
        )]));
        assert!(f.is_empty(), "x-* private-use tag should pass: {f:?}");
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let entries: Vec<_> = (0..8)
            .map(|i| entry(&format!("section#h{i}"), "en_us"))
            .collect();
        let f = detect_lang_attribute_subtag_format(&snap(entries));
        let hit = f
            .iter()
            .find(|x| x.kind == "lang.underscore-separator")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_lang_hosts() {
        assert!(LANG_ATTRIBUTE_SUBTAG_FORMAT_JS.starts_with("(() => {"));
        assert!(LANG_ATTRIBUTE_SUBTAG_FORMAT_JS.ends_with(")()"));
        assert!(LANG_ATTRIBUTE_SUBTAG_FORMAT_JS.contains("[lang]"));
        assert!(LANG_ATTRIBUTE_SUBTAG_FORMAT_JS.contains("getAttribute"));
    }
}
