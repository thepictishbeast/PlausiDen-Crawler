//! `autocomplete` — form autocomplete-attribute hint detector.
//!
//! Mirror of `src/autocomplete.ts`. WCAG 1.3.5 (Identify Input
//! Purpose, AA). Findings:
//!
//!   * `autocomplete.missing-credentials`   strict   credential
//!                                                   field (email/
//!                                                   password/username)
//!                                                   has no
//!                                                   autocomplete attr
//!   * `autocomplete.missing-pii`           warn     PII field
//!                                                   without
//!                                                   autocomplete
//!   * `autocomplete.invalid-token`         warn     present but not
//!                                                   a recognized
//!                                                   WHATWG token
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const AUTOCOMPLETE_JS: &str = r##"(() => {
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
      if (rect.width === 0 && rect.height === 0) return false;
      return true;
    };
    const accessibleName = function(el) {
      if (el.id) {
        const lab = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
        if (lab) return (lab.textContent || '').trim().slice(0, 60);
      }
      let parent = el.parentElement;
      let hops = 0;
      while (parent && hops < 4) {
        if (parent.tagName === 'LABEL') return (parent.textContent || '').trim().slice(0, 60);
        parent = parent.parentElement;
        hops += 1;
      }
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim().slice(0, 60);
      return '';
    };
    const out = [];
    const els = document.querySelectorAll('input,textarea,select');
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      if (!isVisible(el)) continue;
      const tag = el.tagName.toLowerCase();
      const type = (el.getAttribute('type') || '').toLowerCase();
      if (tag === 'input') {
        const skip = ['hidden', 'submit', 'reset', 'button', 'image', 'checkbox', 'radio', 'file', 'color', 'range'];
        if (skip.indexOf(type) >= 0) continue;
      }
      const hasAutocomplete = el.hasAttribute('autocomplete');
      const ac = (el.getAttribute('autocomplete') || '').trim().toLowerCase();
      out.push({
        selector: selectorOf(el),
        type: type,
        name: (el.getAttribute('name') || '').toLowerCase(),
        id: el.getAttribute('id') || '',
        autocomplete: ac,
        hasAutocomplete: hasAutocomplete,
        accessibleName: accessibleName(el),
      });
    }
    return { fields: out };
})()"##;

/// One captured form field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedAutocompleteField {
    /// Best-effort CSS selector.
    pub selector: String,
    /// Lowercased input type, '' for textarea/select.
    pub r#type: String,
    /// HTML name attribute, lowercased.
    pub name: String,
    /// id attribute (case-preserved).
    pub id: String,
    /// Lowercased autocomplete attribute value.
    pub autocomplete: String,
    /// True iff the autocomplete attribute is present.
    pub has_autocomplete: bool,
    /// Computed accessible name (first 60 chars).
    pub accessible_name: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct AutocompleteSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Captured fields.
    pub fields: Vec<CapturedAutocompleteField>,
}

const VALID_AUTOCOMPLETE_TOKENS: &[&str] = &[
    "on",
    "off",
    "name",
    "honorific-prefix",
    "given-name",
    "additional-name",
    "family-name",
    "honorific-suffix",
    "nickname",
    "email",
    "username",
    "new-password",
    "current-password",
    "one-time-code",
    "organization-title",
    "organization",
    "street-address",
    "address-line1",
    "address-line2",
    "address-line3",
    "address-level1",
    "address-level2",
    "address-level3",
    "address-level4",
    "country",
    "country-name",
    "postal-code",
    "cc-name",
    "cc-given-name",
    "cc-additional-name",
    "cc-family-name",
    "cc-number",
    "cc-exp",
    "cc-exp-month",
    "cc-exp-year",
    "cc-csc",
    "cc-type",
    "transaction-currency",
    "transaction-amount",
    "language",
    "bday",
    "bday-day",
    "bday-month",
    "bday-year",
    "sex",
    "tel",
    "tel-country-code",
    "tel-national",
    "tel-area-code",
    "tel-local",
    "tel-extension",
    "impp",
    "url",
    "photo",
    "webauthn",
];

const MODIFIER_TOKENS: &[&str] = &[
    "shipping", "billing", "home", "work", "mobile", "fax", "pager",
];

fn classify_field(field: &CapturedAutocompleteField) -> &'static str {
    if field.r#type == "email" || field.r#type == "password" {
        return "credential";
    }
    if field.r#type == "tel" {
        return "pii";
    }
    let hay = format!(
        "{} {} {} {}",
        field.name,
        field.id.to_lowercase(),
        field.accessible_name.to_lowercase(),
        field.r#type
    );
    // Credential patterns (must match before PII).
    let cred = [
        "password", "username", "email", "signin", "sign-in", "login",
    ];
    for p in cred {
        if hay.contains(p) {
            return "credential";
        }
    }
    // Standalone `pass` / `pw` (matched as whole word-ish — simple
    // contains check is fine because the haystack is space-separated).
    if hay.split_whitespace().any(|t| t == "pass" || t == "pw") {
        return "credential";
    }
    let pii = [
        "name",
        "first.name",
        "first-name",
        "last.name",
        "last-name",
        "given-name",
        "family-name",
        "phone",
        "tel",
        "address",
        "city",
        "state",
        "zip",
        "postal",
        "postcode",
        "country",
        "birth",
        "dob",
        "credit-card",
        "credit.card",
        "cc",
    ];
    for p in pii {
        if hay.contains(p) {
            return "pii";
        }
    }
    "other"
}

/// Pure detector: snapshot → findings. Flags form inputs missing
/// `autocomplete` attributes (or carrying the wrong values for
/// the input type) per WCAG 2.1 Input Purposes.
#[must_use]
pub fn detect_autocomplete_issues(snap: &AutocompleteSnapshot) -> Vec<crate::AxisFinding> {
    let mut missing_cred = Vec::<&CapturedAutocompleteField>::new();
    let mut missing_pii = Vec::<&CapturedAutocompleteField>::new();
    let mut invalid_token = Vec::<&CapturedAutocompleteField>::new();

    for f in &snap.fields {
        if f.has_autocomplete {
            let tokens: Vec<&str> = f.autocomplete.split_whitespace().collect();
            let all_valid = !tokens.is_empty()
                && tokens.iter().all(|t| {
                    VALID_AUTOCOMPLETE_TOKENS.contains(t)
                        || t.starts_with("section-")
                        || MODIFIER_TOKENS.contains(t)
                });
            if !all_valid {
                invalid_token.push(f);
            }
            continue;
        }
        match classify_field(f) {
            "credential" => missing_cred.push(f),
            "pii" => missing_pii.push(f),
            _ => {}
        }
    }

    let render = |f: &&CapturedAutocompleteField| -> String {
        let type_part = if f.r#type.is_empty() {
            String::new()
        } else {
            format!("[type={}]", f.r#type)
        };
        let name = if f.name.is_empty() {
            if f.id.is_empty() {
                "?".to_owned()
            } else {
                f.id.clone()
            }
        } else {
            f.name.clone()
        };
        format!(
            "{} {name}{type_part} (label='{}')",
            f.selector, f.accessible_name
        )
    };

    let mut out = Vec::<crate::AxisFinding>::new();

    if !missing_cred.is_empty() {
        let examples: Vec<String> = missing_cred.iter().take(5).map(render).collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "autocomplete.missing-credentials".to_owned(),
            detail: format!(
                "{} credential field(s) (login/email/password) have no autocomplete attribute. Password managers can't reliably save or autofill — security AND UX harm. WCAG 1.3.5 (Identify Input Purpose, AA). Add autocomplete=\"username\" / \"email\" / \"current-password\" / \"new-password\" as appropriate. Examples: {}",
                missing_cred.len(),
                examples.join("; ")
            ),
        });
    }

    if !missing_pii.is_empty() {
        let examples: Vec<String> = missing_pii.iter().take(5).map(render).collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "autocomplete.missing-pii".to_owned(),
            detail: format!(
                "{} PII field(s) (name/phone/address/etc.) have no autocomplete attribute. Browsers can't autofill — slower checkout / higher form abandonment. Add the appropriate WHATWG token. Examples: {}",
                missing_pii.len(),
                examples.join("; ")
            ),
        });
    }

    if !invalid_token.is_empty() {
        let examples: Vec<String> = invalid_token
            .iter()
            .take(5)
            .map(|f| {
                format!(
                    "{} (autocomplete='{}', label='{}')",
                    f.selector, f.autocomplete, f.accessible_name
                )
            })
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "autocomplete.invalid-token".to_owned(),
            detail: format!(
                "{} field(s) have autocomplete values that aren't recognized WHATWG tokens. Browsers will ignore and fall back to default behaviour. Examples: {}",
                invalid_token.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fld(ty: &str, name: &str, ac: &str, has_ac: bool, label: &str) -> CapturedAutocompleteField {
        CapturedAutocompleteField {
            selector: "body > input".to_owned(),
            r#type: ty.to_owned(),
            name: name.to_owned(),
            id: name.to_owned(),
            autocomplete: ac.to_owned(),
            has_autocomplete: has_ac,
            accessible_name: label.to_owned(),
        }
    }

    fn snap(fields: Vec<CapturedAutocompleteField>) -> AutocompleteSnapshot {
        AutocompleteSnapshot {
            page_url: "http://t/".to_owned(),
            fields,
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(
            AUTOCOMPLETE_JS.matches('(').count(),
            AUTOCOMPLETE_JS.matches(')').count()
        );
        assert_eq!(
            AUTOCOMPLETE_JS.matches('{').count(),
            AUTOCOMPLETE_JS.matches('}').count()
        );
    }

    #[test]
    fn generic_no_findings() {
        let f = detect_autocomplete_issues(&snap(vec![fld(
            "text",
            "comment",
            "",
            false,
            "Your comment",
        )]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn email_missing_strict() {
        let f = detect_autocomplete_issues(&snap(vec![fld("email", "email", "", false, "Email")]));
        assert!(f
            .iter()
            .any(|x| x.kind == "autocomplete.missing-credentials"
                && x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn password_strict() {
        let f =
            detect_autocomplete_issues(&snap(vec![fld("password", "pw", "", false, "Password")]));
        assert!(f
            .iter()
            .any(|x| x.kind == "autocomplete.missing-credentials"));
    }

    #[test]
    fn username_strict() {
        let f =
            detect_autocomplete_issues(&snap(vec![fld("text", "username", "", false, "Username")]));
        assert!(f
            .iter()
            .any(|x| x.kind == "autocomplete.missing-credentials"));
    }

    #[test]
    fn tel_pii_warn() {
        let f = detect_autocomplete_issues(&snap(vec![fld("tel", "phone", "", false, "Phone")]));
        assert!(f.iter().any(
            |x| x.kind == "autocomplete.missing-pii" && x.severity == crate::AxisSeverity::Warn
        ));
    }

    #[test]
    fn email_with_token_clean() {
        let f =
            detect_autocomplete_issues(&snap(vec![fld("email", "email", "email", true, "Email")]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn off_token_accepted() {
        let f =
            detect_autocomplete_issues(&snap(vec![fld("email", "email", "off", true, "Email")]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn bogus_token_warn() {
        let f = detect_autocomplete_issues(&snap(vec![fld(
            "text", "comment", "bogus", true, "Comment",
        )]));
        assert!(f.iter().any(|x| x.kind == "autocomplete.invalid-token"));
    }

    #[test]
    fn multi_token_shipping_accepted() {
        let f = detect_autocomplete_issues(&snap(vec![fld(
            "text",
            "addr",
            "shipping street-address",
            true,
            "Address",
        )]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn section_prefix_accepted() {
        let f = detect_autocomplete_issues(&snap(vec![fld(
            "text",
            "cc",
            "section-billing cc-number",
            true,
            "Card",
        )]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn aggregation() {
        let f = detect_autocomplete_issues(&snap(vec![
            fld("email", "e1", "", false, "Email"),
            fld("password", "p1", "", false, "Password"),
            fld("password", "p2", "", false, "Confirm"),
        ]));
        let cred = f
            .iter()
            .find(|x| x.kind == "autocomplete.missing-credentials")
            .expect("found");
        assert!(
            cred.detail.starts_with("3 credential field(s)"),
            "{}",
            cred.detail
        );
    }
}
