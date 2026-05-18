//! `html_lang` — `<html lang>` attribute detector.
//!
//! Mirror of `src/htmlLang.ts`. WCAG 3.1.1 (Level A). Findings:
//!
//!   * `lang.missing`         strict   `<html>` has no lang attribute
//!   * `lang.empty`           strict   `<html lang="">`
//!   * `lang.invalid`         warn     structurally non-BCP-47
//!   * `lang.unknown-primary` warn     primary subtag not in common
//!                                     ISO 639-1 set
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const HTML_LANG_JS: &str = r##"(() => {
    const root = document.documentElement;
    if (!root.hasAttribute('lang')) return { present: false, value: '' };
    return { present: true, value: root.getAttribute('lang') || '' };
})()"##;

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HtmlLangSnapshot {
    /// Page URL.
    pub page_url: String,
    /// True iff `<html>` has a `lang` attribute (empty or not).
    pub present: bool,
    /// Raw attribute value, '' if absent.
    pub value: String,
}

/// Common ISO 639-1 codes (subset — covers ~99% of real web).
const COMMON_ISO_639_1: &[&str] = &[
    "aa", "ab", "af", "am", "ar", "as", "az", "ba", "be", "bg", "bh", "bm", "bn", "bo", "br", "bs",
    "ca", "ce", "co", "cs", "cy", "da", "de", "dv", "dz", "el", "en", "eo", "es", "et", "eu", "fa",
    "fi", "fj", "fo", "fr", "fy", "ga", "gd", "gl", "gn", "gu", "gv", "ha", "he", "hi", "hr", "ht",
    "hu", "hy", "ia", "id", "ie", "ig", "is", "it", "iu", "ja", "jv", "ka", "kk", "kl", "km", "kn",
    "ko", "ku", "kw", "ky", "la", "lb", "lo", "lt", "lv", "mg", "mk", "ml", "mn", "mr", "ms", "mt",
    "my", "na", "nb", "ne", "nl", "nn", "no", "oc", "or", "pa", "pl", "ps", "pt", "qu", "rm", "ro",
    "ru", "rw", "sa", "sd", "se", "sg", "si", "sk", "sl", "sm", "sn", "so", "sq", "sr", "ss", "st",
    "su", "sv", "sw", "ta", "te", "tg", "th", "ti", "tk", "tl", "tn", "to", "tr", "ts", "tt", "tw",
    "ug", "uk", "ur", "uz", "vi", "wa", "wo", "xh", "yi", "yo", "zh", "zu",
];

/// Loose BCP-47 structural validator. Mirror of `looksLikeBcp47`
/// in htmlLang.ts.
fn looks_like_bcp47(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.chars().any(char::is_whitespace) {
        return false;
    }
    if value.contains('_') {
        return false;
    }
    if value.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return false;
    }
    if value.starts_with('-') || value.ends_with('-') {
        return false;
    }
    if value.contains("--") {
        return false;
    }
    let primary = value.split('-').next().unwrap_or("");
    let primary_len = primary.chars().count();
    if !(2..=3).contains(&primary_len) || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    true
}

/// Pure detector: snapshot → findings. Flags missing / empty /
/// malformed `<html lang="…">` per WCAG 3.1.1 Language of Page.
/// Screen readers need this to pick the right pronunciation engine.
#[must_use]
pub fn detect_html_lang_issues(snap: &HtmlLangSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    if !snap.present {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "lang.missing".to_owned(),
            detail: "<html> element has no 'lang' attribute. WCAG 3.1.1 (Language of Page, A) — screen readers can't pick the right voice and pronounce the page in the user's UA default language. Add e.g. <html lang=\"en\">.".to_owned(),
        });
        return out;
    }

    if snap.value.trim().is_empty() {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "lang.empty".to_owned(),
            detail: "<html lang=\"\"> — attribute present but empty. Same effect as missing. Either remove the attribute or set it to a valid BCP-47 language tag (e.g. 'en', 'en-US', 'fr-CA').".to_owned(),
        });
        return out;
    }

    let value = snap.value.trim();

    if !looks_like_bcp47(value) {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "lang.invalid".to_owned(),
            detail: format!(
                "<html lang=\"{value}\"> doesn't look like a valid BCP-47 tag (no underscores, no whitespace, primary subtag must be 2-3 letters). Examples: 'en', 'en-US', 'pt-BR', 'zh-Hans'."
            ),
        });
        return out;
    }

    let primary = value.split('-').next().unwrap_or("").to_lowercase();
    if !COMMON_ISO_639_1.contains(&primary.as_str()) {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "lang.unknown-primary".to_owned(),
            detail: format!(
                "<html lang=\"{value}\"> primary subtag '{primary}' isn't in the common ISO 639-1 set. This may be a typo (e.g. 'engish' for 'en') or a rare valid language. Verify against the IANA Language Subtag Registry."
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_with(value: &str) -> HtmlLangSnapshot {
        HtmlLangSnapshot {
            page_url: "http://t/".to_owned(),
            present: true,
            value: value.to_owned(),
        }
    }

    fn snap_missing() -> HtmlLangSnapshot {
        HtmlLangSnapshot {
            page_url: "http://t/".to_owned(),
            present: false,
            value: String::new(),
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(
            HTML_LANG_JS.matches('(').count(),
            HTML_LANG_JS.matches(')').count()
        );
        assert_eq!(
            HTML_LANG_JS.matches('{').count(),
            HTML_LANG_JS.matches('}').count()
        );
    }

    #[test]
    fn en_passes() {
        assert!(detect_html_lang_issues(&snap_with("en")).is_empty());
    }

    #[test]
    fn en_us_passes() {
        assert!(detect_html_lang_issues(&snap_with("en-US")).is_empty());
    }

    #[test]
    fn missing_strict() {
        let f = detect_html_lang_issues(&snap_missing());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "lang.missing");
    }

    #[test]
    fn empty_strict_only() {
        let f = detect_html_lang_issues(&snap_with(""));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "lang.empty");
    }

    #[test]
    fn whitespace_treated_as_empty() {
        let f = detect_html_lang_issues(&snap_with("   "));
        assert!(f.iter().any(|x| x.kind == "lang.empty"));
    }

    #[test]
    fn underscore_invalid() {
        let f = detect_html_lang_issues(&snap_with("en_US"));
        assert!(f.iter().any(|x| x.kind == "lang.invalid"));
    }

    #[test]
    fn four_letter_primary_invalid() {
        let f = detect_html_lang_issues(&snap_with("english"));
        assert!(f.iter().any(|x| x.kind == "lang.invalid"));
    }

    #[test]
    fn unknown_primary_warn() {
        let f = detect_html_lang_issues(&snap_with("xx"));
        assert!(f.iter().any(|x| x.kind == "lang.unknown-primary"));
    }

    #[test]
    fn uppercase_normalized() {
        // Primary subtag lowercased before set lookup.
        assert!(detect_html_lang_issues(&snap_with("EN")).is_empty());
    }

    #[test]
    fn doubled_hyphen_invalid() {
        let f = detect_html_lang_issues(&snap_with("en--US"));
        assert!(f.iter().any(|x| x.kind == "lang.invalid"));
    }

    #[test]
    fn whitespace_inside_invalid() {
        let f = detect_html_lang_issues(&snap_with("en US"));
        assert!(f.iter().any(|x| x.kind == "lang.invalid"));
    }
}
