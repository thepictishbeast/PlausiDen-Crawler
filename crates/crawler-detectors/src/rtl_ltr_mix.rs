//! `rtl_ltr_mix` — flags inline mixed RTL+LTR text runs without
//! bidi isolation (`<bdi>` wrapper or `dir="auto"` on an ancestor).
//!
//! The browser's default Unicode Bidirectional Algorithm reorders
//! characters at run boundaries. When user-supplied content
//! contains, for example, an Arabic name embedded in an English
//! sentence, the reordering can swap punctuation, parentheses,
//! or numbers into the wrong visual position. The fix is per-run
//! isolation:
//!
//! - `<bdi>` wraps a bidi-isolated run (preferred for inline
//!   user content).
//! - `dir="auto"` on a containing block lets the browser pick
//!   direction per first-strong character.
//!
//! Sites that interpolate user-generated names / quoted text /
//! search queries without isolation routinely ship visually
//! broken output for RTL languages.
//!
//! ## Heuristic
//!
//! For each text-bearing element with >=8 chars:
//!
//! 1. Classify each character as RTL / LTR / neutral. RTL ranges:
//!    Hebrew (U+0590-05FF), Arabic (U+0600-06FF, U+0750-077F),
//!    Arabic Presentation Forms (U+FB50-FDFF, U+FE70-FEFF),
//!    Syriac (U+0700-074F), Thaana (U+0780-07BF), N'Ko
//!    (U+07C0-07FF). LTR: Basic Latin letters + Cyrillic +
//!    Greek + most CJK (which is LTR by default in BiDi).
//! 2. Compute `rtl_chars` + `ltr_chars`.
//! 3. Mixed iff `rtl_chars >= 4` AND `ltr_chars >= 4`.
//! 4. Flag iff mixed AND NO ancestor declares `dir="auto"` /
//!    `dir="rtl"` / `dir="ltr"` AND the element itself isn't
//!    inside `<bdi>`.
//!
//! Severity: `warn`. The default BiDi algorithm renders many
//! mixed cases correctly; isolation is a defense for the cases
//! that drift (parens, slashes, currency symbols). Flagging
//! strict would noise out on bilingual content authored by
//! teams that DID think about it but used a different
//! isolation primitive.
//!
//! AVP-2: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offender — an element with mixed RTL/LTR runs
/// and no bidi isolation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RtlLtrMixHit {
    /// CSS-ish path of the offending element.
    pub selector: String,
    /// Tag name (uppercase, e.g. `P`, `LI`, `SPAN`).
    pub tag: String,
    /// First 80 chars of the element's text for context.
    pub text_sample: String,
    /// Count of RTL-script characters.
    pub rtl_chars: u32,
    /// Count of LTR-script characters.
    pub ltr_chars: u32,
}

/// Captured page state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RtlLtrMixSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every element that fired the heuristic.
    pub hits: Vec<RtlLtrMixHit>,
    /// Total text-bearing elements walked.
    pub scanned_elements: u32,
}

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_rtl_ltr_mix(snap: &RtlLtrMixSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .hits
        .iter()
        .take(5)
        .map(|h| {
            format!(
                "{} <{}> ({}RTL / {}LTR) \"{}\"",
                h.selector, h.tag, h.rtl_chars, h.ltr_chars, h.text_sample
            )
        })
        .collect();
    vec![AxisFinding {
        severity: AxisSeverity::Warn,
        kind: "rtl-ltr-mix.no-isolation".to_owned(),
        detail: format!(
            "{} element(s) with mixed RTL + LTR character runs and no bidi isolation (no <bdi> wrapper, no dir=\"auto\" on an ancestor). Default Unicode BiDi may reorder punctuation / parens / numbers into the wrong visual slot. Wrap mixed inline runs in <bdi> or set dir=\"auto\" on the containing block. Examples: {}",
            snap.hits.len(),
            examples.join("; ")
        ),
    }]
}

/// Classify a single code point. `true` if RTL.
pub fn is_rtl_codepoint(c: char) -> bool {
    let u = c as u32;
    // Hebrew
    (0x0590..=0x05FF).contains(&u)
        // Arabic + Arabic Supplement
        || (0x0600..=0x06FF).contains(&u)
        || (0x0750..=0x077F).contains(&u)
        // Syriac + Thaana + N'Ko
        || (0x0700..=0x074F).contains(&u)
        || (0x0780..=0x07BF).contains(&u)
        || (0x07C0..=0x07FF).contains(&u)
        // Arabic Presentation Forms-A + Forms-B
        || (0xFB50..=0xFDFF).contains(&u)
        || (0xFE70..=0xFEFF).contains(&u)
}

/// Classify as LTR (alphabetic, non-RTL). Returns false for
/// neutrals (whitespace, punctuation, digits) and for RTL chars.
pub fn is_ltr_codepoint(c: char) -> bool {
    if is_rtl_codepoint(c) {
        return false;
    }
    c.is_alphabetic()
}

/// Count RTL + LTR chars in a string. Useful for offline analysis.
pub fn count_directional_chars(s: &str) -> (u32, u32) {
    let mut rtl = 0u32;
    let mut ltr = 0u32;
    for c in s.chars() {
        if is_rtl_codepoint(c) {
            rtl += 1;
        } else if is_ltr_codepoint(c) {
            ltr += 1;
        }
    }
    (rtl, ltr)
}

/// Browser-side capture script.
pub const RTL_LTR_MIX_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
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

    const isVisible = function(el) {
      const cs = window.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    };

    const isRtl = function(code) {
      return (code >= 0x0590 && code <= 0x05FF)
          || (code >= 0x0600 && code <= 0x06FF)
          || (code >= 0x0750 && code <= 0x077F)
          || (code >= 0x0700 && code <= 0x074F)
          || (code >= 0x0780 && code <= 0x07BF)
          || (code >= 0x07C0 && code <= 0x07FF)
          || (code >= 0xFB50 && code <= 0xFDFF)
          || (code >= 0xFE70 && code <= 0xFEFF);
    };

    const hasBidiIsolation = function(el) {
      let node = el;
      while (node && node.nodeType === 1) {
        if (node.tagName === 'BDI') return true;
        const dir = node.getAttribute && node.getAttribute('dir');
        if (dir === 'auto' || dir === 'rtl' || dir === 'ltr') return true;
        node = node.parentElement;
        if (node === document.body) break;
      }
      return false;
    };

    const hits = [];
    let scanned = 0;
    const candidates = document.querySelectorAll('p, li, td, th, span, h1, h2, h3, h4, h5, h6, dt, dd, button, a, label');
    for (let i = 0; i < candidates.length; i++) {
      const el = candidates[i];
      if (!isVisible(el)) continue;
      const text = (el.textContent || '').trim();
      if (text.length < 8) continue;
      scanned += 1;
      let rtl = 0;
      let ltr = 0;
      for (let j = 0; j < text.length; j++) {
        const code = text.charCodeAt(j);
        if (isRtl(code)) {
          rtl += 1;
        } else if ((code >= 65 && code <= 90) || (code >= 97 && code <= 122) ||
                   (code >= 0x00C0 && code <= 0x024F) || (code >= 0x0370 && code <= 0x03FF) ||
                   (code >= 0x0400 && code <= 0x04FF)) {
          ltr += 1;
        }
      }
      if (rtl < 4 || ltr < 4) continue;
      if (hasBidiIsolation(el)) continue;
      hits.push({
        selector: selectorOf(el),
        tag: el.tagName,
        textSample: text.slice(0, 80),
        rtlChars: rtl,
        ltrChars: ltr,
      });
    }

    return {
      pageUrl: window.location.href,
      hits: hits,
      scannedElements: scanned,
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn mix_hit(selector: &str, rtl: u32, ltr: u32) -> RtlLtrMixHit {
        RtlLtrMixHit {
            selector: selector.into(),
            tag: "P".into(),
            text_sample: "mixed text".into(),
            rtl_chars: rtl,
            ltr_chars: ltr,
        }
    }

    #[test]
    fn empty_snapshot_produces_no_findings() {
        let snap = RtlLtrMixSnapshot {
            page_url: "https://x".into(),
            hits: vec![],
            scanned_elements: 0,
        };
        assert!(detect_rtl_ltr_mix(&snap).is_empty());
    }

    #[test]
    fn mixed_run_produces_warn_finding() {
        let snap = RtlLtrMixSnapshot {
            page_url: "https://x".into(),
            hits: vec![mix_hit("body > p", 12, 25)],
            scanned_elements: 1,
        };
        let findings = detect_rtl_ltr_mix(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "rtl-ltr-mix.no-isolation");
    }

    #[test]
    fn is_rtl_codepoint_recognises_hebrew_arabic() {
        assert!(is_rtl_codepoint('א'));   // Hebrew aleph
        assert!(is_rtl_codepoint('ا'));   // Arabic alef
        assert!(is_rtl_codepoint('ܐ'));   // Syriac alaph (U+0710)
        assert!(!is_rtl_codepoint('A'));
        assert!(!is_rtl_codepoint('a'));
        assert!(!is_rtl_codepoint(' '));
        assert!(!is_rtl_codepoint('1'));
    }

    #[test]
    fn is_ltr_codepoint_excludes_neutrals_and_rtl() {
        assert!(is_ltr_codepoint('A'));
        assert!(is_ltr_codepoint('a'));
        assert!(is_ltr_codepoint('Б')); // Cyrillic
        assert!(!is_ltr_codepoint('א'));
        assert!(!is_ltr_codepoint('1'));
        assert!(!is_ltr_codepoint(' '));
        assert!(!is_ltr_codepoint('!'));
    }

    #[test]
    fn count_directional_chars_classifies_correctly() {
        let (rtl, ltr) = count_directional_chars("Hello, שלום world");
        assert_eq!(rtl, 4); // שלום = 4 Hebrew chars
        assert_eq!(ltr, 10); // Hello + world = 10 Latin letters
    }

    #[test]
    fn count_skips_digits_and_punctuation() {
        let (rtl, ltr) = count_directional_chars("abc 123 .,!?");
        assert_eq!(rtl, 0);
        assert_eq!(ltr, 3);
    }

    #[test]
    fn js_capture_constant_is_sensible() {
        assert!(RTL_LTR_MIX_DOM_CAPTURE_JS.contains("isRtl"));
        assert!(RTL_LTR_MIX_DOM_CAPTURE_JS.contains("hasBidiIsolation"));
        assert!(RTL_LTR_MIX_DOM_CAPTURE_JS.contains("BDI"));
    }
}
