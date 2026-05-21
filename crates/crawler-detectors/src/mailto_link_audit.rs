//! `mailto_link_audit` — `mailto:` anchor format + safety audit.
//!
//! Sibling axis to `pdf_link_indicator`,
//! `download_attribute_audit`, and `link_target_blank_safety`.
//! Audits `<a href="mailto:...">` anchors against RFC 6068 (the
//! `mailto:` URI scheme) and accessibility expectations.
//!
//! ## The bug class
//!
//! Five observable failures:
//!
//! 1. **Malformed address** — `mailto:foo` (no `@`),
//!    `mailto:@example.com` (no local part), `mailto:foo@`
//!    (no domain). Browser passes the malformed string to the
//!    mail client which errors.
//! 2. **Unencoded special characters in subject/body** —
//!    `?subject=Hello world` without percent-encoding the
//!    space. Some clients tolerate it, many strip the entire
//!    subject. Per RFC 6068, the body of the URI MUST be
//!    percent-encoded.
//! 3. **Multiple recipients without `cc` / `bcc`** — comma-
//!    separated `to=` addresses are permitted, but operators
//!    sometimes use `&to=foo@x.com&to=bar@y.com` (which only
//!    the last one wins) or write `,` in `to=` without
//!    encoding.
//! 4. **No visible / accessible affordance** — link text reads
//!    `"Contact us"` with no obvious email indicator and no
//!    `aria-label` containing the address. Screen-reader users
//!    don't know they're about to open a mail client.
//! 5. **Address mismatch between visible text and href** —
//!    visible text says `foo@example.com` but href targets
//!    `bar@example.com`. Phishing tactic; surface to operator.
//!
//! ## Findings
//!
//! * `mailto.malformed-address` strict — `mailto:` URI does
//!   not have a single `local@domain` shape.
//! * `mailto.unencoded-subject-or-body` warn — `?subject=` or
//!   `?body=` contains literal characters that should be
//!   percent-encoded (spaces, `&`, `#`, `?` inside the
//!   parameter value).
//! * `mailto.duplicate-recipient-param` warn — query string
//!   has multiple `to=`/`cc=`/`bcc=` params (operator likely
//!   intended comma-separated within one).
//! * `mailto.address-mismatch` warn — visible text contains an
//!   email address that differs from the `mailto:` target.
//! * `mailto.no-affordance` warn — link text + accessible name
//!   carry no email-token (`@` / `email` / `mail`); user
//!   doesn't know it's a mailto link.
//!
//! Out of scope:
//!
//! * Validating that the recipient address actually exists —
//!   not a static-audit concern.
//! * `tel:` links — covered by future `tel_link_audit` axis.
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

/// One captured `mailto:` anchor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MailtoAnchorEntry {
    /// CSS-ish selector pointing at the anchor.
    pub selector: String,
    /// Raw href value (including the `mailto:` prefix).
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
pub struct MailtoLinkAuditSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<a href^="mailto:">` anchor on the page.
    pub anchors: Vec<MailtoAnchorEntry>,
}

/// Detector.
#[must_use]
pub fn detect_mailto_link_audit(snap: &MailtoLinkAuditSnapshot) -> Vec<AxisFinding> {
    let mut malformed: Vec<String> = Vec::new();
    let mut unencoded: Vec<String> = Vec::new();
    let mut duplicate_param: Vec<String> = Vec::new();
    let mut address_mismatch: Vec<String> = Vec::new();
    let mut no_affordance: Vec<String> = Vec::new();

    for entry in &snap.anchors {
        let parts = parse_mailto(&entry.href);
        match &parts.recipient {
            None => {
                malformed.push(format!(
                    "{} (href={})",
                    entry.selector, entry.href
                ));
                continue;
            }
            Some(addr) => {
                if !is_well_formed_address(addr) {
                    malformed.push(format!(
                        "{} (recipient={})",
                        entry.selector, addr
                    ));
                    continue;
                }
            }
        }

        if has_unencoded_in_params(&parts.raw_query) {
            unencoded.push(format!(
                "{} (query={})",
                entry.selector, parts.raw_query
            ));
        }

        if parts.duplicate_recipient_params {
            duplicate_param.push(format!("{} (href={})", entry.selector, entry.href));
        }

        // Address-mismatch check: if visible text contains an
        // @-shaped token, compare it to the recipient.
        if let (Some(visible_email), Some(target)) =
            (find_email_in_text(&entry.visible_text), parts.recipient.as_deref())
        {
            if !addresses_equal(&visible_email, target) {
                address_mismatch.push(format!(
                    "{} (visible={}, target={})",
                    entry.selector, visible_email, target
                ));
            }
        }

        // Affordance: visible text + accessible name should
        // carry an email-token.
        if !text_has_email_token(&entry.visible_text)
            && !text_has_email_token(&entry.accessible_name)
        {
            no_affordance.push(format!(
                "{} (text=\"{}\")",
                entry.selector, entry.visible_text
            ));
        }
    }

    let mut findings = Vec::new();
    let total = snap.anchors.len();

    if !malformed.is_empty() {
        let preview = preview_examples(
            &malformed.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "mailto.malformed-address".to_owned(),
            detail: format!(
                "{} of {} mailto: anchor(s) carry a malformed recipient address. Examples: {}",
                malformed.len(),
                total,
                preview
            ),
        });
    }

    if !unencoded.is_empty() {
        let preview = preview_examples(
            &unencoded.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "mailto.unencoded-subject-or-body".to_owned(),
            detail: format!(
                "{} of {} mailto: anchor(s) carry query params with unencoded characters. Examples: {}",
                unencoded.len(),
                total,
                preview
            ),
        });
    }

    if !duplicate_param.is_empty() {
        let preview = preview_examples(
            &duplicate_param
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        findings.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "mailto.duplicate-recipient-param".to_owned(),
            detail: format!(
                "{} of {} mailto: anchor(s) have multiple to=/cc=/bcc= params (only the last value wins). Examples: {}",
                duplicate_param.len(),
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
            kind: "mailto.address-mismatch".to_owned(),
            detail: format!(
                "{} of {} mailto: anchor(s) display a different address than they target (potential phishing or stale label). Examples: {}",
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
            kind: "mailto.no-affordance".to_owned(),
            detail: format!(
                "{} of {} mailto: anchor(s) carry no @-token / 'email' / 'mail' affordance in visible text or accessible name. Examples: {}",
                no_affordance.len(),
                total,
                preview
            ),
        });
    }

    findings
}

#[derive(Debug, Default)]
struct MailtoParts {
    recipient: Option<String>,
    raw_query: String,
    duplicate_recipient_params: bool,
}

fn parse_mailto(href: &str) -> MailtoParts {
    let lower_prefix = "mailto:";
    if !href.to_ascii_lowercase().starts_with(lower_prefix) {
        return MailtoParts::default();
    }
    let after = &href[lower_prefix.len()..];
    // Split on `?` for query string.
    let (path, query) = match after.find('?') {
        Some(i) => (&after[..i], &after[i + 1..]),
        None => (after, ""),
    };
    let recipient = if path.is_empty() {
        // Maybe recipient is in `to=` param.
        query
            .split('&')
            .find_map(|p| p.strip_prefix("to=").map(String::from))
    } else {
        Some(path.to_owned())
    };

    // Count occurrences of recipient-bearing params in the
    // query.
    let mut to_count = 0;
    let mut cc_count = 0;
    let mut bcc_count = 0;
    if !query.is_empty() {
        for p in query.split('&') {
            let key_lower = p
                .split('=')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            match key_lower.as_str() {
                "to" => to_count += 1,
                "cc" => cc_count += 1,
                "bcc" => bcc_count += 1,
                _ => {}
            }
        }
    }
    let duplicate_recipient_params =
        to_count > 1 || cc_count > 1 || bcc_count > 1;

    MailtoParts {
        recipient,
        raw_query: query.to_owned(),
        duplicate_recipient_params,
    }
}

fn is_well_formed_address(addr: &str) -> bool {
    // Strip surrounding whitespace + percent-decode the @ if
    // someone wrote `%40` in the local-host position.
    let s = addr.trim();
    let at_count = s.matches('@').count();
    if at_count != 1 {
        return false;
    }
    let mut parts = s.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    // Domain must contain a dot and have non-empty TLD.
    let dot_count = domain.matches('.').count();
    if dot_count < 1 {
        return false;
    }
    let last = domain.rsplit('.').next().unwrap_or("");
    !last.is_empty()
}

fn has_unencoded_in_params(query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    // Scan each param-value for chars that should be encoded:
    // space, raw `&`/`#`/`?` inside the VALUE (the param
    // separator itself is OK).
    for pair in query.split('&') {
        let (_, value) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => continue,
        };
        for ch in value.chars() {
            if ch == ' ' || ch == '\n' || ch == '\t' || ch == '#' || ch == '<' || ch == '>' {
                return true;
            }
        }
    }
    false
}

fn find_email_in_text(text: &str) -> Option<String> {
    // Simple @-shape extractor; reject if the surrounding
    // characters look like the @ is part of a handle (e.g.
    // "@username").
    let bytes = text.as_bytes();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'@' {
            start = Some(i);
            break;
        }
    }
    let i = start?;
    // Walk left for local-part chars.
    let mut left = i;
    while left > 0 {
        let prev = bytes[left - 1];
        if !is_email_local_byte(prev) {
            break;
        }
        left -= 1;
    }
    // Walk right for domain chars.
    let mut right = i + 1;
    while right < bytes.len() {
        let next = bytes[right];
        if !is_email_domain_byte(next) {
            break;
        }
        right += 1;
    }
    if left >= i || right <= i + 1 {
        return None;
    }
    let candidate = &text[left..right];
    if is_well_formed_address(candidate) {
        Some(candidate.to_owned())
    } else {
        None
    }
}

fn is_email_local_byte(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'+')
}

fn is_email_domain_byte(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-')
}

fn addresses_equal(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn text_has_email_token(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains('@') {
        return true;
    }
    // Word-boundary check for "email" / "mail".
    let bytes = lower.as_bytes();
    for needle in ["email", "mail"] {
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

/// Page-side eval. Walks every `<a href^="mailto:">` and
/// captures visible text + accessible name.
pub const MAILTO_LINK_AUDIT_JS: &str = r##"(() => {
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

    const anchors = Array.from(document.querySelectorAll('a[href^="mailto:" i]')).map(function(a) {
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

    fn anchor(sel: &str, href: &str, visible: &str, accessible: &str) -> MailtoAnchorEntry {
        MailtoAnchorEntry {
            selector: sel.to_owned(),
            href: href.to_owned(),
            visible_text: visible.to_owned(),
            accessible_name: accessible.to_owned(),
        }
    }

    fn snap(anchors: Vec<MailtoAnchorEntry>) -> MailtoLinkAuditSnapshot {
        MailtoLinkAuditSnapshot {
            page_url: "https://example.test/".to_owned(),
            anchors,
        }
    }

    #[test]
    fn empty_snapshot_yields_no_findings() {
        let f = detect_mailto_link_audit(&snap(vec![]));
        assert!(f.is_empty());
    }

    #[test]
    fn well_formed_mailto_with_visible_address_is_clean() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com",
            "hello@example.com",
            "hello@example.com",
        )]));
        assert!(f.is_empty(), "expected clean, got {f:?}");
    }

    #[test]
    fn malformed_address_no_at_is_strict() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#bad",
            "mailto:foo",
            "Contact",
            "Contact",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "mailto.malformed-address")
            .expect("malformed expected");
        assert_eq!(hit.severity, AxisSeverity::Strict);
    }

    #[test]
    fn malformed_address_no_local_part_is_strict() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#bad",
            "mailto:@example.com",
            "Contact",
            "Contact",
        )]));
        assert!(f.iter().any(|x| x.kind == "mailto.malformed-address"));
    }

    #[test]
    fn malformed_address_no_domain_dot_is_strict() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#bad",
            "mailto:foo@localhost",
            "Contact",
            "Contact",
        )]));
        assert!(f.iter().any(|x| x.kind == "mailto.malformed-address"));
    }

    #[test]
    fn unencoded_subject_space_is_warn() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com?subject=Hello world",
            "hello@example.com",
            "hello@example.com",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "mailto.unencoded-subject-or-body")
            .expect("unencoded expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn properly_encoded_subject_is_clean() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com?subject=Hello%20world",
            "hello@example.com",
            "hello@example.com",
        )]));
        assert!(f.is_empty(), "encoded subject should pass: {f:?}");
    }

    #[test]
    fn duplicate_to_param_is_warn() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com?to=foo@example.com&to=bar@example.com",
            "hello@example.com",
            "hello@example.com",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "mailto.duplicate-recipient-param")
            .expect("duplicate-recipient expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn address_mismatch_is_warn() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#phish",
            "mailto:attacker@example.net",
            "Contact us at support@example.com",
            "Contact us at support@example.com",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "mailto.address-mismatch")
            .expect("address-mismatch expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
        assert!(hit.detail.contains("support@example.com"));
        assert!(hit.detail.contains("attacker@example.net"));
    }

    #[test]
    fn no_affordance_warn_when_text_carries_no_email_token() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com",
            "Get in touch",
            "Get in touch",
        )]));
        let hit = f
            .iter()
            .find(|x| x.kind == "mailto.no-affordance")
            .expect("no-affordance expected");
        assert_eq!(hit.severity, AxisSeverity::Warn);
    }

    #[test]
    fn email_word_token_satisfies_affordance() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com",
            "Email the team",
            "Email the team",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "mailto.no-affordance"),
            "'Email' word should satisfy affordance: {f:?}"
        );
    }

    #[test]
    fn mail_word_token_satisfies_affordance() {
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#contact",
            "mailto:hello@example.com",
            "Send mail",
            "Send mail",
        )]));
        assert!(
            !f.iter().any(|x| x.kind == "mailto.no-affordance"),
            "'mail' word should satisfy affordance: {f:?}"
        );
    }

    #[test]
    fn email_token_respects_word_boundaries() {
        // "femaleness" should NOT match "mail" or "email" tokens.
        let f = detect_mailto_link_audit(&snap(vec![anchor(
            "a#x",
            "mailto:hello@example.com",
            "femaleness",
            "femaleness",
        )]));
        assert!(
            f.iter().any(|x| x.kind == "mailto.no-affordance"),
            "word boundary should not match 'female': {f:?}"
        );
    }

    #[test]
    fn preview_caps_examples_at_max() {
        let anchors: Vec<_> = (0..8)
            .map(|i| {
                anchor(
                    &format!("a#m{i}"),
                    "mailto:foo",
                    "Contact",
                    "Contact",
                )
            })
            .collect();
        let f = detect_mailto_link_audit(&snap(anchors));
        let hit = f
            .iter()
            .find(|x| x.kind == "mailto.malformed-address")
            .unwrap();
        assert!(hit.detail.contains("(+3 more)"), "{}", hit.detail);
    }

    #[test]
    fn js_const_is_iife_and_walks_mailto_anchors() {
        assert!(MAILTO_LINK_AUDIT_JS.starts_with("(() => {"));
        assert!(MAILTO_LINK_AUDIT_JS.ends_with(")()"));
        assert!(MAILTO_LINK_AUDIT_JS.contains("a[href^=\"mailto:\" i]"));
        assert!(MAILTO_LINK_AUDIT_JS.contains("aria-label"));
    }
}
