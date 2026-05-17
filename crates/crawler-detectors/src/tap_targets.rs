//! `tap_targets` — touch target size detector.
//!
//! WCAG 2.2 Success Criterion 2.5.8 (Target Size Minimum, AA): the
//! touch target for pointer inputs must be at least 24×24 CSS px,
//! except where:
//!   * the target is inline within a sentence
//!   * an equivalent target on the same page meets the size minimum
//!   * the target is essential to the information being conveyed
//!   * the size is determined by the user agent and not modified by
//!     the author
//!
//! WCAG 2.1 Success Criterion 2.5.5 (Target Size, AAA): 44×44 CSS px
//! (also Apple HIG, also Material Design recommendation).
//!
//! Findings:
//!   * `tap.too-small`         strict   < 24×24, no inline-text exception
//!   * `tap.below-recommended` warn     24-43px, AAA recommends ≥44
//!
//! Mirrors `src/tapTargets.ts` — the JS string and the typed structs
//! must stay byte-equivalent so TS and Rust crawlers produce the same
//! findings on the same page.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on enums.
//! * Pure detector function; no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Captures every plausibly-clickable element with
/// its bounding box, role, accessible name, and an `inline` flag for
/// the WCAG 2.5.8 inline-in-sentence exception.
pub const TAP_TARGETS_JS: &str = r##"(() => {
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

    const isInlineInSentence = function(el) {
      const tag = el.tagName.toLowerCase();
      if (tag !== 'a') return false;
      const cs = window.getComputedStyle(el);
      const display = cs.display;
      if (display !== 'inline' && display !== 'inline-block') return false;
      const blockTags = ['p', 'li', 'td', 'th', 'blockquote', 'dd', 'dt',
                         'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'figcaption',
                         'caption', 'cite', 'em', 'span'];
      let parent = el.parentElement;
      let hops = 0;
      while (parent && hops < 4) {
        const ptag = parent.tagName.toLowerCase();
        if (blockTags.indexOf(ptag) >= 0) {
          const siblings = parent.childNodes;
          let textBefore = false;
          let textAfter = false;
          let foundEl = false;
          for (let i = 0; i < siblings.length; i++) {
            const n = siblings[i];
            if (n === el) { foundEl = true; continue; }
            if (n.nodeType === 3 && (n.textContent || '').trim().length > 0) {
              if (foundEl) textAfter = true;
              else textBefore = true;
            }
          }
          if (textBefore || textAfter) return true;
        }
        parent = parent.parentElement;
        hops += 1;
      }
      return false;
    };

    const accessibleName = function(el) {
      const aria = el.getAttribute('aria-label');
      if (aria && aria.trim()) return aria.trim();
      const text = (el.textContent || '').trim();
      if (text) return text.slice(0, 60);
      const title = el.getAttribute('title');
      if (title && title.trim()) return title.trim().slice(0, 60);
      return '';
    };

    const sel = [
      'a[href]',
      'button',
      'input[type=button]',
      'input[type=submit]',
      'input[type=reset]',
      'input[type=checkbox]',
      'input[type=radio]',
      'input[type=image]',
      'input[type=file]',
      'select',
      'summary',
      '[role=button]',
      '[role=link]',
      '[role=checkbox]',
      '[role=radio]',
      '[role=menuitem]',
      '[role=tab]',
      '[role=switch]',
      '[onclick]',
    ].join(',');

    const out = [];
    const els = document.querySelectorAll(sel);
    for (let i = 0; i < els.length; i++) {
      const el = els[i];
      if (!isVisible(el)) continue;
      if (el.getAttribute('data-tap') === 'compact') continue;
      const r = el.getBoundingClientRect();
      out.push({
        selector: selectorOf(el),
        tag: el.tagName.toLowerCase(),
        role: el.getAttribute('role') || '',
        width: Math.round(r.width),
        height: Math.round(r.height),
        inline: isInlineInSentence(el),
        accessibleName: accessibleName(el),
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      viewportHeight: window.innerHeight,
      targets: out,
    };
})()"##;

/// One captured tap target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedTapTarget {
    /// Best-effort CSS selector.
    pub selector: String,
    /// Lowercased tag name.
    pub tag: String,
    /// `role` attribute or empty.
    pub role: String,
    /// Bounding-box width in CSS px (rounded).
    pub width: u32,
    /// Bounding-box height in CSS px (rounded).
    pub height: u32,
    /// True if this is an inline anchor inside a sentence (WCAG
    /// 2.5.8 exception applies).
    pub inline: bool,
    /// Accessible name (first 60 chars).
    pub accessible_name: String,
}

/// Snapshot of the page's tap-target landscape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TapTargetsSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// Viewport width (px) at capture.
    pub viewport_width: u32,
    /// Viewport height (px) at capture.
    pub viewport_height: u32,
    /// Captured targets.
    pub targets: Vec<CapturedTapTarget>,
}

/// WCAG 2.2 AA minimum: 24×24 CSS px.
const STRICT_MIN_PX: u32 = 24;
/// WCAG 2.1 AAA recommendation: 44×44 CSS px.
const RECOMMENDED_MIN_PX: u32 = 44;

/// Apply detection rules. Pure function; mirror of
/// `detectTapTargetIssues` in tapTargets.ts. The TS test suite is
/// the canonical fixture set — every test there must round-trip
/// here too.
#[must_use]
pub fn detect_tap_target_issues(snap: &TapTargetsSnapshot) -> Vec<crate::AxisFinding> {
    let mut too_small = Vec::<&CapturedTapTarget>::new();
    let mut below_recommended = Vec::<&CapturedTapTarget>::new();

    for t in &snap.targets {
        // Inline-in-sentence link → WCAG exception applies.
        if t.inline {
            continue;
        }
        // 0×0 = not actually rendered.
        if t.width == 0 || t.height == 0 {
            continue;
        }
        let min_dim = t.width.min(t.height);
        if min_dim < STRICT_MIN_PX {
            too_small.push(t);
        } else if min_dim < RECOMMENDED_MIN_PX {
            below_recommended.push(t);
        }
    }

    let mut out = Vec::<crate::AxisFinding>::new();

    if !too_small.is_empty() {
        let examples: Vec<String> = too_small
            .iter()
            .take(5)
            .map(|t| {
                let name = if t.accessible_name.is_empty() {
                    "(no name)".to_owned()
                } else {
                    t.accessible_name.clone()
                };
                format!("{} {}×{}px — '{name}'", t.selector, t.width, t.height)
            })
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "tap.too-small".to_owned(),
            detail: format!(
                "{} interactive target(s) are smaller than 24×24 CSS pixels. WCAG 2.2 SC 2.5.8 (Target Size Minimum, AA): touch targets must be ≥24×24 unless inline within a sentence or the function is duplicated by a larger target. Mobile / touchscreen users will misfire. Examples: {}",
                too_small.len(),
                examples.join("; ")
            ),
        });
    }

    if !below_recommended.is_empty() {
        let examples: Vec<String> = below_recommended
            .iter()
            .take(5)
            .map(|t| {
                let name = if t.accessible_name.is_empty() {
                    "(no name)".to_owned()
                } else {
                    t.accessible_name.clone()
                };
                format!("{} {}×{}px — '{name}'", t.selector, t.width, t.height)
            })
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "tap.below-recommended".to_owned(),
            detail: format!(
                "{} interactive target(s) are 24-43px on the smallest dimension. WCAG 2.1 SC 2.5.5 (AAA), Apple HIG, and Material Design all recommend ≥44×44. Acceptable for AA but increases mis-tap rate on touchscreens. Examples: {}",
                below_recommended.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(selector: &str, w: u32, h: u32, inline: bool) -> CapturedTapTarget {
        CapturedTapTarget {
            selector: selector.to_owned(),
            tag: "button".to_owned(),
            role: String::new(),
            width: w,
            height: h,
            inline,
            accessible_name: "Sign up".to_owned(),
        }
    }

    fn snap(targets: Vec<CapturedTapTarget>) -> TapTargetsSnapshot {
        TapTargetsSnapshot {
            page_url: "http://t/".to_owned(),
            viewport_width: 375,
            viewport_height: 667,
            targets,
        }
    }

    #[test]
    fn js_braces_balanced() {
        assert_eq!(
            TAP_TARGETS_JS.matches('(').count(),
            TAP_TARGETS_JS.matches(')').count()
        );
        assert_eq!(
            TAP_TARGETS_JS.matches('{').count(),
            TAP_TARGETS_JS.matches('}').count()
        );
    }

    #[test]
    fn clean_snapshot_no_findings() {
        let s = snap(vec![target("a", 44, 44, false), target("b", 80, 48, false)]);
        assert!(detect_tap_target_issues(&s).is_empty());
    }

    #[test]
    fn below_24_strict() {
        let s = snap(vec![target("a", 16, 16, false)]);
        let f = detect_tap_target_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "tap.too-small" && x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn between_24_and_44_warn() {
        let s = snap(vec![target("a", 32, 32, false)]);
        let f = detect_tap_target_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "tap.below-recommended" && x.severity == crate::AxisSeverity::Warn));
    }

    #[test]
    fn inline_link_exempt() {
        // A 8×12 inline link inside a sentence — WCAG exception.
        let s = snap(vec![target("a", 8, 12, true)]);
        assert!(detect_tap_target_issues(&s).is_empty());
    }

    #[test]
    fn zero_by_zero_filtered() {
        let s = snap(vec![target("a", 0, 0, false)]);
        assert!(detect_tap_target_issues(&s).is_empty());
    }

    #[test]
    fn min_dimension_governs() {
        // A 100×10 button is too thin even though wide.
        let s = snap(vec![target("a", 100, 10, false)]);
        let f = detect_tap_target_issues(&s);
        assert!(f.iter().any(|x| x.kind == "tap.too-small"));
    }

    #[test]
    fn boundary_24_passes_strict_warns_recommended() {
        let s = snap(vec![target("a", 24, 24, false)]);
        let f = detect_tap_target_issues(&s);
        assert!(!f.iter().any(|x| x.kind == "tap.too-small"));
        assert!(f.iter().any(|x| x.kind == "tap.below-recommended"));
    }

    #[test]
    fn boundary_44_passes_everything() {
        let s = snap(vec![target("a", 44, 44, false)]);
        assert!(detect_tap_target_issues(&s).is_empty());
    }

    #[test]
    fn examples_capped_at_five() {
        let mut targets = Vec::new();
        for i in 0..7 {
            targets.push(target(&format!("a:nth-of-type({i})"), 12, 12, false));
        }
        let s = snap(targets);
        let f = detect_tap_target_issues(&s);
        let strict = f
            .iter()
            .find(|x| x.kind == "tap.too-small")
            .expect("strict");
        // 5 example entries → 4 separators in the joined string.
        assert_eq!(strict.detail.matches("; ").count(), 4);
    }
}
