//! `tel_link_audit` — `tel:` anchor format + safety audit.
//!
//! Sibling axis to `mailto_link_audit`,
//! `download_attribute_audit`, and `pdf_link_indicator`. Audits
//! `<a href="tel:...">` anchors against RFC 3966 (the `tel:` URI
//! scheme).
//!
//! ## The `tel:` URI contract
//!
//! Per RFC 3966:
//!
//! 1. The number SHOULD be in international format (leading `+`
//!    + country code) so the dialler works regardless of the
//!    user's default region. `tel:5551234567` works for US
//!    callers when the device's default region is set to US but
//!    breaks for everyone else.
//! 2. Permitted characters in the number portion: `+`, digits,
//!    `-`, `.`, `(`, `)`, and visual separators. Spaces are NOT
//!    valid — they must be encoded as `%20` (which most diallers
//!    strip anyway) or omitted entirely. Letters (vanity numbers
//!    like `tel:1-800-FLOWERS`) are non-conforming.
//! 3. Extensions go after `;ext=` (`tel:+12025550110;ext=42`).
//!    Operators sometimes write `,123` (DTMF post-dial) or
//!    `x123` which dialler support varies on.
//! 4. The `;phone-context=` parameter is required when the
//!    number is in local (non-international) format. Most
//!    `tel:` links in the wild omit this and rely on region
//!    inference.
//!
//! ## Findings
//!
//! * `tel.empty-number` strict — `tel:` with no number, only
//!   whitespace, or only formatting characters.
//! * `tel.contains-letters` warn — number portion contains
//!   ASCII letters (vanity numbers). Some diallers translate
//!   them but most don't; surface to operator.
//! * `tel.no-country-code` warn — number doesn't start with
//!   `+` AND has no `;phone-context=` parameter. Breaks for
//!   users outside the default region.
//! * `tel.address-mismatch` warn — visible text contains a
//!   phone number that differs from the `tel:` target after
//!   stripping formatting. Phishing tactic or stale label.
//! * `tel.no-affordance` warn — visible text + accessible name
//!   carry no phone-token (no digit / `call` / `phone` / `tel`).
//!
//! Out of scope:
//!
//! * Validating that the dialled number actually exists / is
//!   reachable.
//! * `sms:`, `whatsapp:`, `facetime:` schemes — covered by
//!   future protocol-specific axes.
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

/// One captured `tel:` anchor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TelAnchorEntry {
    /// CSS-ish selector pointing at the anchor.
    pub selector: String,
    /// Raw href value (including the `tel:` prefix).
    pub href: String,
    /// Visible text content of the anchor (trimmed).
    pub visible_text: String,
    /// Accessible name as computed by the runner.
    pub accessible_name: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TelLinkAuditSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<a href^="tel:">` anchor on the page.
    pub anchors: Vec<TelAnchorEntry>,
}

/// Detector.
#[must_use]
pub fn detect_tel_link_audit(snap: &TelLinkAuditSnapshot) -> Vec<AxisFinding> {
    let mut empty_number: Vec<String> = Vec::new();
    let mut contains_letters: Vec<String> = Vec::new();
    let mut no_country_code: Vec<String> = Vec::new();
    let mut address_mismatch: Vec<String> = Vec::new();
    let mut no_affordance: Vec<String> = Vec::new();

    for entry in &snap.anchors {
        let parts = parse_tel(&entry.href);

        let trimmed_number = parts.number.trim();
        let digits_only: String = trimmed_number.chars().filter(|c| c.is_ascii_digit()).collect();

        if digits_only.is_empty() {
            empty_number.push(format!("{} (href={})", entry.selector, entry.href));
            continue;
        }

        if number_contains_letters(trimmed_number) {
            contains_letters.push(format!("{} (number={})", entry.selector, trimmed_number));
        }

        // International-format check: leading + AND no
        // ;phone-context= parameter means a regional number.
        let has_intl_prefix = trimmed_number.starts_with('+');
        let has_phone_context = parts.has_phone_context;
        if !has_intl_prefix && !has_phone_context {
            no_country_code.push(format!(
                "{} (number={})",
                entry.selector, trimmed_number
            ));
        }

        if let Some(visible_phone) = find_phone_in_text(&entry.visible_text) {
            if !numbers_equivalent(&visible_phone, trimmed_number) {
                address_mismatch.push(format!(
                    "{} (visible={}, target={})",
                    entry.selector, visible_phone, trimmed_number
                ));
            }
        }

        if !text_has_phone_affordance(&entry.visible_text)
            && !text_has_phone_affordance(&entry.accessible_name)
        {
            no_affordance.push(format!(
                "{} (text=\"{}\")",
                entry.selector, entry.visible_text
            ));
        }
    }

    let mut findings = Vec::new();
    let total = snap.anchors.len();

    if !empty_number.is_empty() {
        let preview =
            preview_examples(&empty_number.iter().map(String::as_str).collect::<Vec<_>>());
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "tel.empty-number".to_owned(),
            detail: format!(
                "{} of {} tel: anchor(s) carry no digits in the number portion. Examples: {}",
                empty_number.len(),
                total,
                preview
            ),
        });
    }

    if !contains_letters.is_empty() {
        let preview = preview_examples(
            &contains_letters
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tel.contains-letters".to_owned(),
            detail: format!(
                "{} of {} tel: anchor(s) include letters in the number portion (vanity numbers); most diallers don't translate. Examples: {}",
                contains_letters.len(),
                total,
                preview
            ),
        });
    }

    if !no_country_code.is_empty() {
        let preview = preview_examples(
            &no_country_code
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tel.no-country-code".to_owned(),
            detail: format!(
                "{} of {} tel: anchor(s) lack an international prefix (+) AND no `;phone-context=` parameter; breaks for users outside the default region. Examples: {}",
                no_country_code.len(),
                total,
                preview
            ),
        });
    }

    if !address_mismatch.is_empty() {
        let preview = preview_examples(
            &address_mismatch
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tel.address-mismatch".to_owned(),
            detail: format!(
                "{} of {} tel: anchor(s) display a phone number that differs from the tel: target. Examples: {}",
                address_mismatch.len(),
                total,
                preview
            ),
        });
    }

    if !no_affordance.is_empty() {
        let preview = preview_examples(
            &no_affordance
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "tel.no-affordance".to_owned(),
            detail: format!(
                "{} of {} tel: anchor(s) carry no digit / 'call' / 'phone' / 'tel' affordance in visible text or accessible name. Examples: {}",
                no_affordance.len(),
                total,
                preview
            ),
        });
    }

    findings
}

#[derive(Debug, Default)]
struct TelParts {
    number: String,
    has_phone_context: bool,
}

fn parse_tel(href: &str) -> TelParts {
    let lower_prefix = "tel:";
    if !href.to_ascii_lowercase().starts_with(lower_prefix) {
        return TelParts::default();
    }
    let after = &href[lower_prefix.len()..];
    // Number portion ends at first `;` or `?`.
    let mut split_at = after.len();
    for (i, c) in after.char_indices() {
        if c == ';' || c == '?' {
            split_at = i;
            break;
        }
    }
    let number = after[..split_at].to_owned();
    let params = if split_at < after.len() {
        &after[split_at..]
    } else {
        ""
    };
    let has_phone_context = params
        .to_ascii_lowercase()
        .contains(";phone-context=");
    TelParts {
        number,
        has_phone_context,
    }
}

fn number_contains_letters(number: &str) -> bool {
    number.chars().any(|c| c.is_ascii_alphabetic())
}

fn find_phone_in_text(text: &str) -> Option<String> {
    // Pull the longest digit-rich run from the text. Accept
    // optional leading +, ASCII digits, and visual separators
    // (space, -, ., (, )).
    let mut runs: Vec<&str> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i] == b'+' {
            i += 1;
        }
        let run_start = i;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_digit() || matches!(c, b' ' | b'-' | b'.' | b'(' | b')') {
                i += 1;
            } else {
                break;
            }
        }
        let digits = text[run_start..i].chars().filter(|c| c.is_ascii_digit()).count();
        if digits >= 7 {
            runs.push(&text[start..i]);
            continue;
        }
        if i == start {
            i += 1;
        }
    }
    runs.into_iter().max_by_key(|s| s.chars().filter(|c| c.is_ascii_digit()).count()).map(str::to_owned)
}

fn numbers_equivalent(a: &str, b: &str) -> bool {
    let strip = |s: &str| -> String {
        s.chars().filter(|c| c.is_ascii_digit()).collect()
    };
    let na = strip(a);
    let nb = strip(b);
    if na == nb && !na.is_empty() {
        return true;
    }
    // Tail-match: country-code prefix variance ("+1 555..." vs
    // "555..."). Either na ends with nb or nb ends with na, and
    // the suffix is at least 7 digits.
    if na.len() >= 7 && nb.len() >= 7 {
        let shorter = na.len().min(nb.len());
        if na.ends_with(&nb[nb.len() - shorter..])
            || nb.ends_with(&na[na.len() - shorter..])
        {
            return true;
        }
    }
    false
}

fn text_has_phone_affordance(text: &str) -> bool {
    if text.chars().filter(|c| c.is_ascii_digit()).count() >= 7 {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for needle in ["call", "phone", "tel"] {
        let nb = needle.as_bytes();
        if bytes.len() < nb.len() {
            continue;
        }
        let mut i = 0;
        while i + nb.len() <= bytes.len() {
            if &bytes[i..i + nb.len()] == nb {
                let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
                let after_idx = i + nb.len();
                let after_ok = after_idx == bytes.len()
                    || !bytes[after_idx].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    return true;
                }
            }
            i += 1;
        }
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

/// Page-side eval. Walks every `<a href^="tel:">` and captures
/// visible text + accessible name.
pub const TEL_LINK_AUDIT_JS: &str = r##"(() => {
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

    const anchors = Array.from(document.querySelectorAll('a[href^="tel:" i]')).map(function(a) {
      return {
        selector: selectorOf(a),
        href: a.getAttribute('href') || '',
        visibleText: (a.textContent || '').trim(),
        accessibleName: accessibleNameFor(a)
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

    fn anchor(sel: &str, href: &str, visible: &str, accessible: &str) -> TelAnchorEntry {
        TelAnchorEntry {
            selector: sel.to_owned(),
            href: href.to_owned(),
            visible_text: visible.to_owned(),
            accessible_name: accessible.to_owned(),
        }
    }

    fn snap(anchors: Vec<TelAnchorEntry>) -> TelLinkAuditSnapshot {
        TelLinkAuditSnapshot {
            page_url: "https://example.test/".to_owned(),
            anchors,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_tel_link_audit(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn international_format_with_matching_visible_is_clean() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#contact",
            "tel:+12025550110",
            "+1 202 555 0110",
            "+1 202 555 0110",
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn empty_number_is_strict() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#bad",
            "tel:",
            "Call",
            "Call",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tel.empty-number")
            .expect("empty-number expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn vanity_number_is_warn() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#vanity",
            "tel:+1-800-FLOWERS",
            "1-800-FLOWERS",
            "1-800-FLOWERS",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tel.contains-letters")
            .expect("contains-letters expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn no_country_code_is_warn() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#local",
            "tel:5551234567",
            "555 123 4567",
            "555 123 4567",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tel.no-country-code")
            .expect("no-country-code expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn phone_context_param_satisfies_country_code_check() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#with-ctx",
            "tel:5551234567;phone-context=+1",
            "555-123-4567",
            "555-123-4567",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "tel.no-country-code"),
            "phone-context should satisfy: {f:?}"
        );
    }

    #[test]
    fn address_mismatch_is_warn() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#phish",
            "tel:+19995551234",
            "Call +1 202 555 0110",
            "Call +1 202 555 0110",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tel.address-mismatch")
            .expect("address-mismatch expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn country_code_prefix_variance_is_not_a_mismatch() {
        // Visible "555 123 4567" vs target "+1 555 123 4567" —
        // tail-match should not flag.
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#contact",
            "tel:+15551234567",
            "555-123-4567",
            "555-123-4567",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "tel.address-mismatch"),
            "country-code variance should be tolerated: {f:?}"
        );
    }

    #[test]
    fn no_affordance_warn_when_text_has_no_phone_token() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#contact",
            "tel:+12025550110",
            "Contact us",
            "Contact us",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "tel.no-affordance")
            .expect("no-affordance expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn call_word_satisfies_affordance() {
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#contact",
            "tel:+12025550110",
            "Call our team",
            "Call our team",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "tel.no-affordance"),
            "'Call' should satisfy: {f:?}"
        );
    }

    #[test]
    fn digit_run_satisfies_affordance() {
        // Plenty of digits in visible text — counts as affordance.
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#contact",
            "tel:+12025550110",
            "+1 202 555 0110",
            "+1 202 555 0110",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "tel.no-affordance"),
            "digit-rich text should satisfy: {f:?}"
        );
    }

    #[test]
    fn extension_parameter_is_clean() {
        // tel:+1...;ext=42 is valid; just check it doesn't false-
        // positive any of the strict / warn paths beyond the
        // expected.
        let f = detect_tel_link_audit(&snap(vec![anchor(
            "a#ext",
            "tel:+12025550110;ext=42",
            "+1 202 555 0110 ext 42",
            "+1 202 555 0110 ext 42",
        )]));
        // Should ONLY have potential mismatch checks; not empty,
        // not letters, not no-country-code.
        assert!(!f.iter().any(|x| x.kind == "tel.empty-number"));
        assert!(!f.iter().any(|x| x.kind == "tel.contains-letters"));
        assert!(!f.iter().any(|x| x.kind == "tel.no-country-code"));
        assert!(!f.iter().any(|x| x.kind == "tel.no-affordance"));
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let anchors: Vec<_> = (0..8)
            .map(|i| {
                anchor(
                    &format!("a#t{i}"),
                    "tel:",
                    "Call",
                    "Call",
                )
            })
            .collect();
        let f = detect_tel_link_audit(&snap(anchors));
        let hit = f
            .iter()
            .find(|x| x.kind == "tel.empty-number")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_tel_anchors() {
        assert!(TEL_LINK_AUDIT_JS.starts_with("(() => {"));
        assert!(TEL_LINK_AUDIT_JS.ends_with(")()"));
        assert!(TEL_LINK_AUDIT_JS.contains("a[href^=\"tel:\" i]"));
        assert!(TEL_LINK_AUDIT_JS.contains("aria-label"));
    }
}
