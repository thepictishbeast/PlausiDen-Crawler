//! `password_paste_blocked` — flags `<input type="password">`
//! that block paste / copy / context-menu interactions.
//!
//! Defect class: an operator (often misguidedly told this is
//! "more secure") attaches `onpaste="return false"` /
//! `onkeydown` handlers that swallow `Ctrl+V` / `oncontextmenu`
//! / `oncopy` on a password field. Real-world impact:
//!
//! * **Breaks password managers.** 1Password / Bitwarden /
//!   built-in browser managers paste credentials via the same
//!   API; blocking paste blocks autofill.
//! * **Pushes users to shorter / reused passwords.** When the
//!   user can't paste a 40-character random string, they
//!   re-use one they can remember and type.
//! * **NIST SP 800-63B explicitly forbids it** ("Verifiers
//!   SHOULD permit claimants to use 'paste' functionality when
//!   entering a memorized secret. This facilitates the use of
//!   password managers, which are widely used and in many
//!   cases increase the likelihood that users will choose
//!   stronger memorized secrets").
//! * The Original Reason — "prevent shoulder-surfing or
//!   clipboard-leak" — doesn't survive scrutiny. The same
//!   attacker can read the keystrokes; preventing paste
//!   without preventing typing is theatre.
//!
//! ## Heuristic
//!
//! JS walks every `input[type="password"]`. For each, captures:
//!
//! * `has_onpaste` — `onpaste` attribute is present (any value;
//!   the canonical hostile form is `"return false"`).
//! * `has_oncopy` — `oncopy` attribute set (some sites block
//!   copy too, on the "don't let users save the password
//!   anywhere" theory).
//! * `has_oncontextmenu` — `oncontextmenu` set (blocks right-
//!   click → paste).
//! * `has_onkeydown_clipboard_block` — `onkeydown` whose value
//!   contains the strings `'V'` + `ctrlKey` (canonical
//!   hand-rolled clipboard block).
//!
//! Honors `data-paste-block-allow="true"` opt-out for the rare
//! input that genuinely should block paste (e.g. a high-
//! security re-entry confirmation flow — though we'd argue
//! it's still the wrong UX).
//!
//! ## Severity
//!
//! * **Strict** — has_onpaste OR has_onkeydown_clipboard_block.
//!   These are the canonical password-manager-breaking
//!   patterns.
//! * **Warn** — has_oncopy OR has_oncontextmenu only. Less
//!   directly hostile but worth flagging.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending password input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PasswordPasteBlockedHit {
    /// CSS-ish path of the offending `<input>`.
    pub selector: String,
    /// `autocomplete` attribute (empty when missing) — useful
    /// context: `current-password` is the canonical login
    /// field; `new-password` is signup; missing-autocomplete is
    /// itself a different bug.
    pub autocomplete: String,
    /// Best-effort accessible name (`<label>` text or
    /// `aria-label`, capped 60 chars). Empty when none.
    pub label: String,
    /// True iff `onpaste` attribute is present.
    pub has_onpaste: bool,
    /// True iff `oncopy` attribute is present.
    pub has_oncopy: bool,
    /// True iff `oncontextmenu` attribute is present.
    pub has_oncontextmenu: bool,
    /// True iff `onkeydown` attribute appears to be intercepting
    /// `Ctrl+V` (matches `ctrlKey` + `'v'` / `'V'` substrings).
    pub has_onkeydown_clipboard_block: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PasswordPasteBlockedSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending password inputs.
    pub hits: Vec<PasswordPasteBlockedHit>,
    /// Total `<input type="password">` elements walked.
    pub scanned_password_inputs: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_password_paste_blocked(snap: &PasswordPasteBlockedSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut strict: Vec<&PasswordPasteBlockedHit> = Vec::new();
    let mut warn: Vec<&PasswordPasteBlockedHit> = Vec::new();
    for h in &snap.hits {
        let blocks_paste = h.has_onpaste || h.has_onkeydown_clipboard_block;
        let other_block = h.has_oncopy || h.has_oncontextmenu;
        if blocks_paste {
            strict.push(h);
        } else if other_block {
            warn.push(h);
        }
    }

    let format_example = |h: &PasswordPasteBlockedHit| -> String {
        let label = if h.label.is_empty() {
            String::new()
        } else {
            format!(" [{}]", h.label)
        };
        let mut blocks: Vec<&str> = Vec::new();
        if h.has_onpaste { blocks.push("onpaste"); }
        if h.has_oncopy { blocks.push("oncopy"); }
        if h.has_oncontextmenu { blocks.push("oncontextmenu"); }
        if h.has_onkeydown_clipboard_block { blocks.push("onkeydown:ctrl+v"); }
        format!(
            "{}{} ({})",
            h.selector,
            label,
            blocks.join(",")
        )
    };

    let mut out = Vec::new();
    if !strict.is_empty() {
        let examples: Vec<String> = strict
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "password-paste-blocked.paste".to_owned(),
            detail: format!(
                "{} password input(s) block paste — breaks password managers, pushes users toward weaker reused passwords, and contradicts NIST SP 800-63B. Remove `onpaste` / `onkeydown` clipboard interception. Opt out with `data-paste-block-allow=\"true\"` for measured exceptions. Examples: {}",
                strict.len(),
                examples.join("; ")
            ),
        });
    }
    if !warn.is_empty() {
        let examples: Vec<String> = warn
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "password-paste-blocked.copy-or-context".to_owned(),
            detail: format!(
                "{} password input(s) block copy or right-click. Less directly hostile than paste-blocking but still security theatre — the underlying threat model doesn't survive scrutiny. Examples: {}",
                warn.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every
/// `input[type="password"]`, captures the relevant handler
/// attributes + the autocomplete hint + the resolved label.
///
/// Mirror any change in this file's `PasswordPasteBlockedHit`
/// + snapshot fields.
pub const PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS: &str = r#"
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

    const labelOf = function(input) {
      if (input.id) {
        const l = document.querySelector('label[for="' + CSS.escape(input.id) + '"]');
        if (l && l.textContent) return l.textContent.trim().substring(0, 60);
      }
      let p = input.parentElement;
      let depth = 0;
      while (p && depth < 5) {
        if (p.tagName === 'LABEL' && p.textContent) return p.textContent.trim().substring(0, 60);
        p = p.parentElement;
        depth += 1;
      }
      const aria = input.getAttribute('aria-label');
      if (aria) return aria.trim().substring(0, 60);
      return '';
    };

    // Returns true iff the supplied onkeydown source string
    // appears to block Ctrl+V. Matching against `ctrlKey` AND
    // (`'v'` or `'V'`) keeps false positives down (we won't
    // flag a handler that just listens for Enter).
    const onkeydownBlocksCtrlV = function(src) {
      if (!src) return false;
      const lower = src.toLowerCase();
      if (lower.indexOf('ctrlkey') === -1) return false;
      return lower.indexOf("'v'") !== -1 || lower.indexOf('"v"') !== -1;
    };

    const hits = [];
    let scanned = 0;
    const inputs = document.querySelectorAll('input[type="password"]');
    for (const input of inputs) {
      // Opt-out: operator measured + accepted the block.
      if (input.getAttribute && input.getAttribute('data-paste-block-allow') === 'true') continue;
      scanned += 1;
      const onpaste = input.getAttribute('onpaste');
      const oncopy = input.getAttribute('oncopy');
      const oncontextmenu = input.getAttribute('oncontextmenu');
      const onkeydown = input.getAttribute('onkeydown');
      const hasOnpaste = onpaste != null;
      const hasOncopy = oncopy != null;
      const hasOncontextmenu = oncontextmenu != null;
      const hasOnkeydownBlock = onkeydownBlocksCtrlV(onkeydown);
      if (!hasOnpaste && !hasOncopy && !hasOncontextmenu && !hasOnkeydownBlock) continue;
      hits.push({
        selector: selectorOf(input),
        autocomplete: (input.getAttribute('autocomplete') || '').trim(),
        label: labelOf(input),
        hasOnpaste: hasOnpaste,
        hasOncopy: hasOncopy,
        hasOncontextmenu: hasOncontextmenu,
        hasOnkeydownClipboardBlock: hasOnkeydownBlock
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedPasswordInputs: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        onpaste: bool,
        oncopy: bool,
        oncontextmenu: bool,
        onkeydown_block: bool,
    ) -> PasswordPasteBlockedHit {
        PasswordPasteBlockedHit {
            selector: selector.into(),
            autocomplete: "current-password".into(),
            label: String::new(),
            has_onpaste: onpaste,
            has_oncopy: oncopy,
            has_oncontextmenu: oncontextmenu,
            has_onkeydown_clipboard_block: onkeydown_block,
        }
    }

    fn snap(hits: Vec<PasswordPasteBlockedHit>) -> PasswordPasteBlockedSnapshot {
        PasswordPasteBlockedSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_password_inputs: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_password_paste_blocked(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn onpaste_is_strict() {
        let s = snap(vec![hit("#password", true, false, false, false)]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "password-paste-blocked.paste");
        assert!(findings[0].detail.contains("#password"));
        assert!(findings[0].detail.contains("NIST"));
    }

    #[test]
    fn onkeydown_clipboard_block_is_strict() {
        let s = snap(vec![hit("#pw", false, false, false, true)]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert!(findings[0].detail.contains("onkeydown:ctrl+v"));
    }

    #[test]
    fn oncopy_only_is_warn() {
        let s = snap(vec![hit("#pw", false, true, false, false)]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "password-paste-blocked.copy-or-context");
    }

    #[test]
    fn oncontextmenu_only_is_warn() {
        let s = snap(vec![hit("#pw", false, false, true, false)]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn both_paste_and_copy_only_emits_strict() {
        // Paste-block + copy-block on same input: the strict
        // bucket wins (paste is the worse defect).
        let s = snap(vec![hit("#pw", true, true, false, false)]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn mixed_inputs_emit_both_findings() {
        let s = snap(vec![
            hit("#a", true, false, false, false),
            hit("#b", false, true, false, false),
        ]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"password-paste-blocked.paste"));
        assert!(kinds.contains(&"password-paste-blocked.copy-or-context"));
    }

    #[test]
    fn label_appears_in_examples_when_present() {
        let mut h = hit("#pw", true, false, false, false);
        h.label = "Password".into();
        let s = snap(vec![h]);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("[Password]"));
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(&format!("#pw-{i}"), true, false, false, false));
        }
        let s = snap(hits);
        let findings = detect_password_paste_blocked(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 password input(s)"));
        // 5 examples → 4 "; " separators. Body message also
        // contains a `"; ` from "...don't survive scrutiny..."?
        // No — the detail message above carefully avoided `; `
        // inside the body. Count "; " (semi+space) total.
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape.
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("hits"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("scannedPasswordInputs"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("hasOnpaste"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("hasOncopy"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("hasOncontextmenu"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("hasOnkeydownClipboardBlock"));
        // Selector contract.
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("'input[type=\"password\"]'"));
        // Opt-out contract.
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("data-paste-block-allow"));
        // onkeydown heuristic — looks for ctrlKey + 'v'.
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("ctrlkey"));
        assert!(PASSWORD_PASTE_BLOCKED_DOM_CAPTURE_JS.contains("'v'"));
    }
}
