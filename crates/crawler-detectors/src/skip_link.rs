//! `skip_link` — "skip to content" link detector.
//!
//! Mirror of `src/skipLink.ts`. WCAG 2.4.1 Level A. Findings:
//!
//!   * `skip.missing`              warn
//!   * `skip.broken-target`        strict
//!   * `skip.not-first-focusable`  warn
//!   * `skip.permanently-hidden`   strict
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval — locates a skip-link candidate via two heuristics
/// (text/class match, then landmark-target match) and returns its
/// observable properties.
pub const SKIP_LINK_JS: &str = r##"(() => {
    const isSkipLinkText = function(el) {
      const t = (el.textContent || '').trim().toLowerCase();
      if (/skip/.test(t)) return true;
      if (/jump.{0,4}content/.test(t)) return true;
      const cls = (el.className || '').toString().toLowerCase();
      if (/skip/.test(cls)) return true;
      return false;
    };

    const focusableSel = 'a[href], button:not([disabled]), input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
    const allFocusable = Array.from(document.querySelectorAll(focusableSel))
      .filter(function(el) {
        const cs = window.getComputedStyle(el);
        if (cs.display === 'none' || cs.visibility === 'hidden') return false;
        return true;
      });
    const firstFoc = allFocusable[0] || null;

    const anchors = Array.from(document.querySelectorAll('a[href^="#"]'));
    let candidate = null;
    for (const a of anchors) {
      if (isSkipLinkText(a)) { candidate = a; break; }
    }
    if (!candidate) {
      for (const a of anchors.slice(0, 3)) {
        const href = a.getAttribute('href') || '';
        if (href.length > 1) {
          const id = href.slice(1);
          const ref = document.getElementById(id);
          if (ref && (ref.tagName === 'MAIN' || ref.getAttribute('role') === 'main' || /^main(-content)?$/i.test(id) || /^content$/i.test(id))) {
            candidate = a;
            break;
          }
        }
      }
    }

    if (!candidate) {
      return { found: false, href: '', text: '', targetExists: false, firstFocusable: false, permanentlyHidden: false };
    }
    const href = candidate.getAttribute('href') || '';
    const text = (candidate.textContent || '').trim().slice(0, 60);
    const targetId = href.startsWith('#') ? href.slice(1) : '';
    const targetExists = targetId.length > 0 && document.getElementById(targetId) !== null;
    const firstFocusable = candidate === firstFoc;
    const cs = window.getComputedStyle(candidate);
    const permanentlyHidden = cs.display === 'none' || cs.visibility === 'hidden';
    return { found: true, href: href, text: text, targetExists: targetExists, firstFocusable: firstFocusable, permanentlyHidden: permanentlyHidden };
})()"##;

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SkipLinkSnapshot {
    /// Page URL.
    pub page_url: String,
    /// True iff a candidate was found.
    pub found: bool,
    /// Candidate href (empty if not found).
    pub href: String,
    /// Candidate visible text.
    pub text: String,
    /// True iff the href fragment resolves to an element on the page.
    pub target_exists: bool,
    /// True iff the candidate is the first focusable element.
    pub first_focusable: bool,
    /// True iff display:none / visibility:hidden in unfocused state.
    pub permanently_hidden: bool,
}

#[must_use]
pub fn detect_skip_link_issues(snap: &SkipLinkSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    if !snap.found {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "skip.missing".to_owned(),
            detail: "No \"skip to content\" link found on the page. Keyboard / screen-reader users must tab through every nav item to reach content. WCAG 2.4.1 (Bypass Blocks, A). Add an <a href=\"#main\">Skip to main content</a> as the first focusable element, visually hidden until focused.".to_owned(),
        });
        return out;
    }

    if snap.permanently_hidden {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "skip.permanently-hidden".to_owned(),
            detail: format!(
                "Skip link found ('{}' → {}) but it has display:none or visibility:hidden — keyboard users can never focus it. Use the canonical \"visually hidden until focused\" pattern (clip + absolute positioning) instead.",
                snap.text, snap.href
            ),
        });
    }

    if !snap.target_exists {
        let target_id = snap.href.strip_prefix('#').unwrap_or(&snap.href);
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "skip.broken-target".to_owned(),
            detail: format!(
                "Skip link '{}' targets {} but no element with that id exists on the page. The link is dead — pressing Enter does nothing. Add id=\"{target_id}\" to your <main> element.",
                snap.text, snap.href
            ),
        });
    }

    if !snap.first_focusable {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "skip.not-first-focusable".to_owned(),
            detail: format!(
                "Skip link '{}' isn't the first focusable element on the page. WCAG technique G1: the bypass mechanism must be reachable on the very first tab press. Move it before any other focusable element.",
                snap.text
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> SkipLinkSnapshot {
        SkipLinkSnapshot {
            page_url: "http://t/".to_owned(),
            found: true,
            href: "#main".to_owned(),
            text: "Skip to main content".to_owned(),
            target_exists: true,
            first_focusable: true,
            permanently_hidden: false,
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(
            SKIP_LINK_JS.matches('(').count(),
            SKIP_LINK_JS.matches(')').count()
        );
        assert_eq!(
            SKIP_LINK_JS.matches('{').count(),
            SKIP_LINK_JS.matches('}').count()
        );
    }

    #[test]
    fn clean_no_findings() {
        assert!(detect_skip_link_issues(&good()).is_empty());
    }

    #[test]
    fn missing_warn() {
        let mut s = good();
        s.found = false;
        s.href.clear();
        s.text.clear();
        s.target_exists = false;
        s.first_focusable = false;
        let f = detect_skip_link_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "skip.missing");
        assert_eq!(f[0].severity, crate::AxisSeverity::Warn);
    }

    #[test]
    fn broken_target_strict() {
        let mut s = good();
        s.target_exists = false;
        let f = detect_skip_link_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "skip.broken-target" && x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn not_first_focusable_warn() {
        let mut s = good();
        s.first_focusable = false;
        let f = detect_skip_link_issues(&s);
        assert!(f.iter().any(
            |x| x.kind == "skip.not-first-focusable" && x.severity == crate::AxisSeverity::Warn
        ));
    }

    #[test]
    fn permanently_hidden_strict() {
        let mut s = good();
        s.permanently_hidden = true;
        let f = detect_skip_link_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "skip.permanently-hidden"
                && x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn missing_short_circuits() {
        let mut s = good();
        s.found = false;
        s.target_exists = false;
        s.first_focusable = false;
        let f = detect_skip_link_issues(&s);
        assert_eq!(f.len(), 1, "{:?}", f);
    }

    #[test]
    fn hidden_and_broken_both_fire() {
        let mut s = good();
        s.permanently_hidden = true;
        s.target_exists = false;
        let f = detect_skip_link_issues(&s);
        assert!(f.iter().any(|x| x.kind == "skip.permanently-hidden"));
        assert!(f.iter().any(|x| x.kind == "skip.broken-target"));
    }
}
