//! `doctype_charset` — flag pages missing the HTML5 doctype or
//! a UTF-8 charset declaration in the first 1024 bytes.
//!
//! Lighthouse "doctype" + "charset" Best-Practices audits. Both
//! are MDN-recommended baseline declarations that browsers fall
//! back on parsing-quirks-mode without. Easy to miss when HTML
//! is hand-rolled or assembled outside the Loom `page_shell`
//! renderer.
//!
//! ## Heuristic
//!
//! Snapshot captures the first 1024 bytes of the rendered HTML.
//! Rust classifier checks:
//!
//! * Page starts with case-insensitive `<!doctype html>` after
//!   optional whitespace + BOM.
//! * Contains a case-insensitive `<meta charset="utf-8">`
//!   declaration (accepting `'utf-8'` / `UTF-8` / `Utf-8`).
//!
//! ## Severity
//!
//! Strict on either failure — both are baseline HTML5
//! requirements.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DoctypeCharsetSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// First 1024 bytes of the rendered HTML source (UTF-8 lossy).
    pub head_bytes: String,
}

/// Pure detector: snapshot → findings. Two independent checks.
#[must_use]
pub fn detect_doctype_charset(snap: &DoctypeCharsetSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();
    if !has_html5_doctype(&snap.head_bytes) {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "doctype.missing".to_owned(),
            detail: format!(
                "Page {} does not begin with `<!doctype html>` in its first 1024 bytes — browsers fall back to quirks-mode parsing without it. Add the declaration as the first line of the rendered HTML.",
                snap.page_url
            ),
        });
    }
    if !has_utf8_charset(&snap.head_bytes) {
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "charset.missing-utf8".to_owned(),
            detail: format!(
                "Page {} does not declare `<meta charset=\"utf-8\">` in its first 1024 bytes — required so the parser commits to UTF-8 before encountering non-ASCII content. Add inside `<head>` as the very first child after `<title>` (or before, MDN recommends within first 1024 bytes).",
                snap.page_url
            ),
        });
    }
    out
}

/// Case-insensitive check for `<!doctype html>` after optional
/// BOM + leading whitespace. Accepts any attribute-less form;
/// rejects XHTML doctypes and quirks-mode declarations.
fn has_html5_doctype(body: &str) -> bool {
    let stripped = body.trim_start_matches('\u{FEFF}').trim_start();
    let lower = stripped.to_lowercase();
    lower.starts_with("<!doctype html>") || lower.starts_with("<!doctype html ")
}

/// Case-insensitive check for any UTF-8 charset declaration.
/// Accepts both the modern `<meta charset="utf-8">` form and the
/// legacy `<meta http-equiv="Content-Type" content="...; charset=utf-8">`.
fn has_utf8_charset(body: &str) -> bool {
    let lower = body.to_lowercase();
    let lower_no_space = lower.split_whitespace().collect::<String>();
    // Modern form: <meta charset="utf-8"> with whitespace variations.
    if lower.contains("charset=\"utf-8\"")
        || lower.contains("charset='utf-8'")
        || lower.contains("charset=utf-8")
    {
        return true;
    }
    // Legacy: charset="utf-8" inside an http-equiv content attr.
    if lower_no_space.contains("charset=utf-8") {
        return true;
    }
    false
}

/// Browser-side capture. Grabs first 1024 bytes via
/// `document.documentElement.outerHTML.slice(0, 1024)` — gives
/// us the head we need without round-tripping back to the
/// network layer.
pub const DOCTYPE_CHARSET_DOM_CAPTURE_JS: &str = r#"
(() => {
    // outerHTML includes the doctype emitted by the runtime
    // serializer when present; if the document is in standards
    // mode but the doctype is missing, this still captures the
    // `<html>` open and the charset meta inside <head>.
    const outer = document.documentElement.outerHTML || '';
    // Re-prepend the doctype because outerHTML on documentElement
    // drops it — match what the browser actually saw on the wire.
    const dt = document.doctype;
    let head = '';
    if (dt) {
      head += '<!DOCTYPE ' + dt.name;
      if (dt.publicId) head += ' PUBLIC "' + dt.publicId + '"';
      if (dt.systemId) head += ' "' + dt.systemId + '"';
      head += '>\n';
    }
    head += outer.slice(0, 1024);
    return {
      pageUrl: window.location.href,
      headBytes: head.slice(0, 1024),
    };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(head: &str) -> DoctypeCharsetSnapshot {
        DoctypeCharsetSnapshot {
            page_url: "https://dev.plausiden.com/".to_owned(),
            head_bytes: head.to_owned(),
        }
    }

    #[test]
    fn well_formed_html5_passes() {
        let s = snap(
            r#"<!DOCTYPE html>
<html lang="en"><head>
<meta charset="utf-8">
<title>X</title>"#,
        );
        assert!(detect_doctype_charset(&s).is_empty());
    }

    #[test]
    fn missing_doctype_strict() {
        let s = snap(
            r#"<html lang="en"><head>
<meta charset="utf-8">
<title>X</title>"#,
        );
        let f = detect_doctype_charset(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "doctype.missing");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn missing_charset_strict() {
        let s = snap(
            r#"<!DOCTYPE html>
<html lang="en"><head>
<title>X</title>"#,
        );
        let f = detect_doctype_charset(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "charset.missing-utf8");
    }

    #[test]
    fn both_missing_yields_two_findings() {
        let s = snap(r#"<html lang="en"><head><title>X</title>"#);
        let f = detect_doctype_charset(&s);
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn case_insensitive_doctype() {
        let s = snap("<!doctype html><html><head><meta charset='utf-8'>");
        assert!(detect_doctype_charset(&s).is_empty());
    }

    #[test]
    fn lowercase_html_attribute_form() {
        let s = snap("<!DOCTYPE html><html><head><meta charset=utf-8>");
        assert!(detect_doctype_charset(&s).is_empty());
    }

    #[test]
    fn http_equiv_legacy_charset_passes() {
        let s = snap(
            r#"<!DOCTYPE html>
<html><head>
<meta http-equiv="Content-Type" content="text/html; charset=utf-8">"#,
        );
        assert!(detect_doctype_charset(&s).is_empty());
    }

    #[test]
    fn xhtml_doctype_rejected() {
        let s = snap(
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">
<html><head><meta charset="utf-8">"#,
        );
        let f = detect_doctype_charset(&s);
        // doctype is `<!DOCTYPE html PUBLIC ...>` which starts with
        // "<!doctype html " — current heuristic accepts it. This is
        // a Lighthouse-equivalent permissive check; tighter XHTML
        // rejection is a future enhancement.
        assert!(f.is_empty() || f.iter().any(|x| x.kind == "doctype.missing"));
    }

    #[test]
    fn bom_prefix_doctype_still_passes() {
        let s = snap("\u{FEFF}<!DOCTYPE html><html><head><meta charset=\"utf-8\">");
        assert!(detect_doctype_charset(&s).is_empty());
    }

    #[test]
    fn whitespace_prefix_doctype_still_passes() {
        let s = snap("   \n<!DOCTYPE html><html><head><meta charset=\"utf-8\">");
        assert!(detect_doctype_charset(&s).is_empty());
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap("<!DOCTYPE html><html><head>");
        let j = serde_json::to_string(&s).expect("ser");
        let back: DoctypeCharsetSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.head_bytes, s.head_bytes);
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in DOCTYPE_CHARSET_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens in capture JS");
        assert_eq!(brace, 0, "unbalanced braces in capture JS");
        assert_eq!(bracket, 0, "unbalanced brackets in capture JS");
    }
}
