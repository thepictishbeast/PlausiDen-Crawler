//! `iframe_title` — `<iframe>` accessible-name detector.
//!
//! WCAG 2.1 Success Criterion 4.1.2 (Name, Role, Value, A) and
//! 2.4.1 (Bypass Blocks, A): every `<iframe>` must have a programm-
//! atically determinable accessible name so assistive tech can
//! announce the frame's purpose before the user enters it. The
//! HTML spec lists `title` as the canonical attribute for this
//! (aria-label and aria-labelledby are also accepted).
//!
//! Distinct from `iframe_sandbox` (security-oriented; sandbox
//! attribute audit). This detector is accessibility-oriented:
//! does the frame carry an accessible name, not how restricted
//! is its execution context.
//!
//! HEURISTIC
//! ---------
//! For each visible `<iframe>` on the page:
//!
//! 1. Check `title` attribute presence + non-empty.
//! 2. If missing, check `aria-label` non-empty.
//! 3. If missing, check `aria-labelledby` resolves to a non-empty
//!    element on the page.
//! 4. If none of the above: emit a strict finding.
//!
//! Additionally flag `title=""` and `aria-label=""` as warn — they're
//! syntactically present but semantically empty, which is the
//! "screen reader announces 'frame'" failure mode.
//!
//! Exempt: presentation iframes (`role="presentation"` or
//! `aria-hidden="true"`) — those are explicitly removed from the
//! AT tree so no accessible name is needed.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on every public enum / result struct.
//! * Pure functions; JS string is the only side-effect channel.

use serde::{Deserialize, Serialize};

/// Page-side eval.
pub const IFRAME_TITLE_JS: &str = r##"(() => {
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
      const op = parseFloat(cs.opacity);
      if (!isNaN(op) && op === 0) return false;
      return true;
    };

    // Resolves an aria-labelledby IDREF list to concatenated text.
    const resolveLabelledBy = function(labelledby) {
      if (!labelledby) return '';
      const ids = labelledby.split(/\s+/).filter(function(s) { return s.length > 0; });
      const parts = [];
      for (let i = 0; i < ids.length; i++) {
        const ref = document.getElementById(ids[i]);
        if (ref) {
          parts.push((ref.textContent || '').trim());
        }
      }
      return parts.join(' ').trim();
    };

    const iframes = document.querySelectorAll('iframe');
    let scanned = 0;
    const missing = [];
    const empty = [];

    for (let i = 0; i < iframes.length; i++) {
      const el = iframes[i];
      scanned += 1;
      if (!isVisible(el)) continue;
      // Presentation / aria-hidden exempt.
      const role = el.getAttribute('role');
      if (role === 'presentation' || role === 'none') continue;
      if (el.getAttribute('aria-hidden') === 'true') continue;

      const title = el.getAttribute('title');
      const ariaLabel = el.getAttribute('aria-label');
      const labelledby = el.getAttribute('aria-labelledby');
      const labelledbyText = resolveLabelledBy(labelledby);

      const hasTitle = title !== null && title.trim().length > 0;
      const hasAriaLabel = ariaLabel !== null && ariaLabel.trim().length > 0;
      const hasLabelledby = labelledbyText.length > 0;

      const accessibleName = hasTitle || hasAriaLabel || hasLabelledby;

      if (!accessibleName) {
        // Check whether the attributes exist but are empty — that's
        // a distinct severity (warn) vs. fully missing (strict).
        const presentButEmpty = (title !== null && title.trim().length === 0)
          || (ariaLabel !== null && ariaLabel.trim().length === 0)
          || (labelledby !== null && labelledbyText.length === 0);
        const row = {
          selector: selectorOf(el),
          src: el.getAttribute('src') || '',
          srcdoc: el.hasAttribute('srcdoc'),
          srcEmpty: !el.hasAttribute('src') && !el.hasAttribute('srcdoc')
        };
        if (presentButEmpty) {
          empty.push(row);
        } else {
          missing.push(row);
        }
      }
    }

    return {
      scanned: scanned,
      missing: missing.slice(0, 50),
      missingCount: missing.length,
      empty: empty.slice(0, 50),
      emptyCount: empty.length
    };
})()"##;

/// One iframe lacking an accessible name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct IframeOffender {
    /// CSS selector.
    pub selector: String,
    /// `src` attribute value (empty if absent).
    pub src: String,
    /// `true` if the iframe uses `srcdoc` instead of `src`.
    pub srcdoc: bool,
    /// `true` if both `src` and `srcdoc` are absent (the frame
    /// loads about:blank — still requires a title).
    pub src_empty: bool,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct IframeTitleSnapshot {
    /// Total iframes scanned (including exempt ones — exempt iframes
    /// aren't reported but they still count toward the scan total).
    pub scanned: u32,
    /// Iframes with no title / aria-label / aria-labelledby at all.
    pub missing: Vec<IframeOffender>,
    /// Total missing-accessible-name iframes (may exceed
    /// `missing.len()` if truncated at 50).
    pub missing_count: u32,
    /// Iframes that have one of the attributes but it's empty.
    pub empty: Vec<IframeOffender>,
    /// Total empty-attribute iframes (may exceed `empty.len()`).
    pub empty_count: u32,
}

/// Apply detection rules. Pure function.
///
/// Emits up to TWO findings per snapshot:
/// * `iframe-title.missing` — strict, when missing > 0.
/// * `iframe-title.empty` — warn, when empty > 0.
#[must_use]
pub fn detect_iframe_title_issues(snap: &IframeTitleSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::new();
    if !snap.missing.is_empty() {
        let first = &snap.missing[0];
        let src_desc = if first.src_empty {
            "no src/srcdoc"
        } else if first.srcdoc {
            "srcdoc"
        } else if first.src.is_empty() {
            "empty src"
        } else {
            first.src.as_str()
        };
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "iframe-title.missing".to_owned(),
            detail: format!(
                "{} <iframe> element(s) lack an accessible name (no title / aria-label / aria-labelledby). WCAG 2.1 SC 4.1.2 + 2.4.1. First offender: {} (src={}). Add `title=\"<purpose>\"` describing what the frame contains.",
                snap.missing_count,
                first.selector,
                src_desc,
            ),
        });
    }
    if !snap.empty.is_empty() {
        let first = &snap.empty[0];
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "iframe-title.empty".to_owned(),
            detail: format!(
                "{} <iframe> element(s) have a title / aria-label attribute present but empty — screen readers announce 'frame' with no further context. First: {}.",
                snap.empty_count,
                first.selector,
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AxisSeverity;

    #[test]
    fn js_balanced() {
        assert_eq!(
            IFRAME_TITLE_JS.matches('(').count(),
            IFRAME_TITLE_JS.matches(')').count()
        );
        assert_eq!(
            IFRAME_TITLE_JS.matches('{').count(),
            IFRAME_TITLE_JS.matches('}').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(IFRAME_TITLE_JS.starts_with("(() => {"));
        assert!(IFRAME_TITLE_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "scanned",
            "missing",
            "missingCount",
            "empty",
            "emptyCount",
            "selector",
            "src",
            "srcdoc",
            "srcEmpty",
        ] {
            assert!(IFRAME_TITLE_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn js_implements_exempt_list() {
        assert!(IFRAME_TITLE_JS.contains("aria-hidden"));
        assert!(IFRAME_TITLE_JS.contains("'presentation'"));
        assert!(IFRAME_TITLE_JS.contains("'none'"));
    }

    #[test]
    fn js_resolves_aria_labelledby() {
        assert!(IFRAME_TITLE_JS.contains("resolveLabelledBy"));
        assert!(IFRAME_TITLE_JS.contains("getElementById"));
    }

    #[test]
    fn clean_page_emits_no_finding() {
        let snap = IframeTitleSnapshot {
            scanned: 3,
            missing: vec![],
            missing_count: 0,
            empty: vec![],
            empty_count: 0,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert!(findings.is_empty());
    }

    #[test]
    fn missing_emits_strict() {
        let snap = IframeTitleSnapshot {
            scanned: 2,
            missing: vec![IframeOffender {
                selector: "body > main > iframe".to_owned(),
                src: "https://example.com/embed".to_owned(),
                srcdoc: false,
                src_empty: false,
            }],
            missing_count: 1,
            empty: vec![],
            empty_count: 0,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert_eq!(findings[0].kind, "iframe-title.missing");
        assert!(findings[0].detail.contains("WCAG 2.1 SC 4.1.2"));
        assert!(findings[0].detail.contains("https://example.com/embed"));
        assert!(findings[0].detail.contains(r#"title="<purpose>""#));
    }

    #[test]
    fn empty_emits_warn() {
        let snap = IframeTitleSnapshot {
            scanned: 1,
            missing: vec![],
            missing_count: 0,
            empty: vec![IframeOffender {
                selector: "body > footer > iframe".to_owned(),
                src: "https://maps.google.com/x".to_owned(),
                srcdoc: false,
                src_empty: false,
            }],
            empty_count: 1,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, AxisSeverity::Warn));
        assert_eq!(findings[0].kind, "iframe-title.empty");
        assert!(findings[0]
            .detail
            .contains("screen readers announce 'frame'"));
    }

    #[test]
    fn missing_and_empty_emit_two_findings() {
        let snap = IframeTitleSnapshot {
            scanned: 5,
            missing: vec![IframeOffender {
                selector: "a".to_owned(),
                src: "x".to_owned(),
                srcdoc: false,
                src_empty: false,
            }],
            missing_count: 1,
            empty: vec![IframeOffender {
                selector: "b".to_owned(),
                src: "y".to_owned(),
                srcdoc: false,
                src_empty: false,
            }],
            empty_count: 1,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert_eq!(findings.len(), 2);
        // Severity ordering: strict missing, warn empty.
        assert!(matches!(findings[0].severity, AxisSeverity::Strict));
        assert!(matches!(findings[1].severity, AxisSeverity::Warn));
    }

    #[test]
    fn srcdoc_iframe_surfaces_in_detail() {
        let snap = IframeTitleSnapshot {
            scanned: 1,
            missing: vec![IframeOffender {
                selector: "x".to_owned(),
                src: String::new(),
                srcdoc: true,
                src_empty: false,
            }],
            missing_count: 1,
            empty: vec![],
            empty_count: 0,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert!(findings[0].detail.contains("srcdoc"));
    }

    #[test]
    fn no_src_no_srcdoc_explicitly_called_out() {
        let snap = IframeTitleSnapshot {
            scanned: 1,
            missing: vec![IframeOffender {
                selector: "x".to_owned(),
                src: String::new(),
                srcdoc: false,
                src_empty: true,
            }],
            missing_count: 1,
            empty: vec![],
            empty_count: 0,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert!(findings[0].detail.contains("no src/srcdoc"));
    }

    #[test]
    fn truncated_count_surfaces_above_array_len() {
        let snap = IframeTitleSnapshot {
            scanned: 100,
            missing: vec![IframeOffender {
                selector: "x".to_owned(),
                src: "y".to_owned(),
                srcdoc: false,
                src_empty: false,
            }],
            missing_count: 73,
            empty: vec![],
            empty_count: 0,
        };
        let findings = detect_iframe_title_issues(&snap);
        assert!(findings[0].detail.contains("73 <iframe>"));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let snap = IframeTitleSnapshot {
            scanned: 4,
            missing: vec![IframeOffender {
                selector: "x".to_owned(),
                src: "y".to_owned(),
                srcdoc: false,
                src_empty: false,
            }],
            missing_count: 1,
            empty: vec![IframeOffender {
                selector: "z".to_owned(),
                src: String::new(),
                srcdoc: true,
                src_empty: false,
            }],
            empty_count: 1,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        assert!(json.contains("\"scanned\":4"));
        assert!(json.contains("\"missingCount\":1"));
        assert!(json.contains("\"srcdoc\":true"));
        let back: IframeTitleSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.missing.len(), 1);
        assert_eq!(back.empty.len(), 1);
        assert!(back.empty[0].srcdoc);
    }
}
