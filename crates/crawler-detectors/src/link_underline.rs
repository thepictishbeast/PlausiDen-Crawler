//! `link_underline` — link-distinguishability detector.
//!
//! Mirror of `src/linkUnderline.ts`. WCAG 1.4.1 (Use of Color,
//! Level A). Single warn finding:
//!
//!   * `link.color-only-distinction`   warn
//!     Inline link inside running text (`<p>`/`<li>`/`<dd>`/
//!     `<blockquote>`/etc., NOT inside `<nav>`/`<header>`/`<footer>`/
//!     `<aside>`) where the only visual distinction from
//!     surrounding text is colour. Fails for ~8% of users
//!     (red-green colourblind) and many low-contrast
//!     environments.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Returns the candidate links that PASS the
/// "only distinguished by colour" filter — anything with an
/// underline / weight contrast / border / outline / different
/// background / box-shadow / italic / icon child is excluded
/// in the JS so the detect-side doesn't need the styles.
pub const LINK_UNDERLINE_JS: &str = r##"(() => {
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

    const isInsideRunningText = function(el) {
      const runningTags = ['P', 'LI', 'DD', 'BLOCKQUOTE', 'TD', 'TH'];
      let parent = el.parentElement;
      let hops = 0;
      let foundChrome = false;
      while (parent && hops < 8) {
        const tag = parent.tagName;
        if (tag === 'NAV' || tag === 'HEADER' || tag === 'FOOTER' || tag === 'ASIDE') {
          foundChrome = true;
        }
        if (runningTags.indexOf(tag) >= 0) {
          return !foundChrome;
        }
        parent = parent.parentElement;
        hops += 1;
      }
      return false;
    };

    const hasNonColorDistinction = function(el, cs, parentCs) {
      const td = cs.textDecorationLine || cs.textDecoration || '';
      if (td.indexOf('underline') >= 0) return true;
      const lw = parseFloat(cs.fontWeight) || 400;
      const pw = parseFloat(parentCs.fontWeight) || 400;
      if (Math.abs(lw - pw) >= 200) return true;
      const bw = parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth)
              + parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
      if (bw > 0) return true;
      const ow = parseFloat(cs.outlineWidth);
      if (ow > 0 && cs.outlineStyle && cs.outlineStyle !== 'none') return true;
      if (cs.backgroundColor && cs.backgroundColor !== 'rgba(0, 0, 0, 0)' &&
          cs.backgroundColor !== 'transparent' &&
          cs.backgroundColor !== parentCs.backgroundColor) {
        return true;
      }
      if (cs.boxShadow && cs.boxShadow !== 'none') return true;
      if (cs.fontStyle === 'italic' && parentCs.fontStyle !== 'italic') return true;
      if (el.querySelector('svg, img, i.icon, [class*="icon"]')) return true;
      return false;
    };

    const out = [];
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const el = anchors[i];
      if (!isVisible(el)) continue;
      if (!isInsideRunningText(el)) continue;
      const cs = window.getComputedStyle(el);
      const parentEl = el.parentElement;
      if (!parentEl) continue;
      const parentCs = window.getComputedStyle(parentEl);
      if (hasNonColorDistinction(el, cs, parentCs)) continue;
      const txt = (el.textContent || '').trim().slice(0, 60);
      const td = cs.textDecorationLine || cs.textDecoration || '';
      out.push({
        selector: selectorOf(el),
        text: txt,
        textDecoration: td,
        fontWeight: cs.fontWeight,
        parentFontWeight: parentCs.fontWeight,
        href: el.getAttribute('href') || '',
      });
    }
    return { candidates: out };
})()"##;

/// One captured candidate (link distinguished only by colour).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedColorOnlyLink {
    /// CSS selector.
    pub selector: String,
    /// Visible text, first 60 chars.
    pub text: String,
    /// Computed text-decoration-line value.
    pub text_decoration: String,
    /// Computed font-weight (string form, e.g. "400").
    pub font_weight: String,
    /// Parent font-weight for comparison.
    pub parent_font_weight: String,
    /// href value.
    pub href: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LinkUnderlineSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Captured candidates already filtered to only the
    /// "color-only" cases.
    pub candidates: Vec<CapturedColorOnlyLink>,
}

#[must_use]
pub fn detect_link_underline_issues(snap: &LinkUnderlineSnapshot) -> Vec<crate::AxisFinding> {
    if snap.candidates.is_empty() {
        return Vec::new();
    }
    let examples: Vec<String> = snap
        .candidates
        .iter()
        .take(5)
        .map(|c| {
            let t = if c.text.is_empty() { "(no text)".to_owned() } else { c.text.clone() };
            format!("{} '{t}' → {}", c.selector, c.href)
        })
        .collect();
    vec![crate::AxisFinding {
        severity: crate::AxisSeverity::Warn,
        kind: "link.color-only-distinction".to_owned(),
        detail: format!(
            "{} inline link(s) inside running text are distinguished from surrounding text ONLY by colour — text-decoration is none, font-weight matches the parent, no border / outline / background / icon. WCAG 1.4.1 (Use of Color, A): ~8% of users (red-green colourblind) and many low-contrast environments can't see the difference. Add 'text-decoration: underline' or another visual cue. Examples: {}",
            snap.candidates.len(),
            examples.join("; ")
        ),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(selector: &str, text: &str) -> CapturedColorOnlyLink {
        CapturedColorOnlyLink {
            selector: selector.to_owned(),
            text: text.to_owned(),
            text_decoration: "none".to_owned(),
            font_weight: "400".to_owned(),
            parent_font_weight: "400".to_owned(),
            href: "/x".to_owned(),
        }
    }

    fn snap(candidates: Vec<CapturedColorOnlyLink>) -> LinkUnderlineSnapshot {
        LinkUnderlineSnapshot {
            page_url: "http://t/".to_owned(),
            candidates,
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(LINK_UNDERLINE_JS.matches('(').count(), LINK_UNDERLINE_JS.matches(')').count());
        assert_eq!(LINK_UNDERLINE_JS.matches('{').count(), LINK_UNDERLINE_JS.matches('}').count());
    }

    #[test]
    fn no_candidates_no_findings() {
        assert!(detect_link_underline_issues(&snap(vec![])).is_empty());
    }

    #[test]
    fn one_candidate_warn() {
        let f = detect_link_underline_issues(&snap(vec![cand("a", "click")]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "link.color-only-distinction");
        assert_eq!(f[0].severity, crate::AxisSeverity::Warn);
    }

    #[test]
    fn aggregates_count() {
        let f = detect_link_underline_issues(&snap(vec![
            cand("a:nth-of-type(1)", "one"),
            cand("a:nth-of-type(2)", "two"),
            cand("a:nth-of-type(3)", "three"),
        ]));
        assert!(f[0].detail.starts_with("3 inline link(s)"));
    }

    #[test]
    fn examples_capped_at_five() {
        let mut cands = Vec::new();
        for i in 0..8 {
            cands.push(cand(&format!("a:nth-of-type({i})"), &format!("link{i}")));
        }
        let f = detect_link_underline_issues(&snap(cands));
        // 5 examples → 4 separators in the joined string.
        assert_eq!(f[0].detail.matches("; ").count(), 4);
    }

    #[test]
    fn no_text_uses_placeholder() {
        let mut c = cand("a", "");
        c.href = "/icon-only".to_owned();
        let f = detect_link_underline_issues(&snap(vec![c]));
        assert!(f[0].detail.contains("(no text)"));
    }
}
