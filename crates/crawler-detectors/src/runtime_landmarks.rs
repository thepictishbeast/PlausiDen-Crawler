//! `runtime_landmarks` — landmark uniqueness + nesting detector.
//!
//! WCAG 1.3.1 + ARIA 1.2 require landmarks to be unique
//! per-page and not nested. Specifically:
//!
//! 1. **Exactly one `<main>`** (or `[role="main"]`). Multiple mains
//!    confuse screen-reader landmark navigation; zero leaves the
//!    page without a primary content region.
//! 2. **At most one top-level banner** (`<header>` outside any
//!    `<article>`/`<section>` OR `[role="banner"]`). Sectioning
//!    `<header>` inside articles is fine — those are not banners.
//! 3. **At most one top-level contentinfo** (`<footer>` outside
//!    any `<article>`/`<section>` OR `[role="contentinfo"]`).
//! 4. **Landmarks must not be nested** at the same role
//!    (no `<main><main>`, no `<nav><nav>`).
//!
//! What this DOES NOT enforce (out of scope):
//!   * Landmark labelling (`aria-label` / `aria-labelledby`) —
//!     that's a different axis (axe handles the static case).
//!   * Order of landmarks — pages can put nav before or after
//!     main; ordering doesn't break a11y.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on enums.
//! * Pure functions; no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Counts every landmark + flags nested ones.
pub const RUNTIME_LANDMARKS_JS: &str = r##"(() => {
    const isLandmark = function(el, role) {
      // Native landmark elements that map to the role.
      // (Header/footer only count as banner/contentinfo when
      // NOT inside an article or section — that's the HTML spec.)
      const tag = el.tagName.toLowerCase();
      if (role === 'main') {
        return tag === 'main' || el.getAttribute('role') === 'main';
      }
      if (role === 'banner') {
        if (el.getAttribute('role') === 'banner') return true;
        if (tag !== 'header') return false;
        // <header> is banner ONLY if not nested in article/section.
        let p = el.parentElement;
        while (p && p !== document.body) {
          const pt = p.tagName.toLowerCase();
          if (pt === 'article' || pt === 'section' || pt === 'aside' || pt === 'nav') {
            return false;
          }
          p = p.parentElement;
        }
        return true;
      }
      if (role === 'contentinfo') {
        if (el.getAttribute('role') === 'contentinfo') return true;
        if (tag !== 'footer') return false;
        let p = el.parentElement;
        while (p && p !== document.body) {
          const pt = p.tagName.toLowerCase();
          if (pt === 'article' || pt === 'section' || pt === 'aside' || pt === 'nav') {
            return false;
          }
          p = p.parentElement;
        }
        return true;
      }
      if (role === 'navigation') {
        return tag === 'nav' || el.getAttribute('role') === 'navigation';
      }
      if (role === 'complementary') {
        return tag === 'aside' || el.getAttribute('role') === 'complementary';
      }
      return false;
    };

    const collect = function(role) {
      // Walk every element; native + role-attribute alternatives.
      const out = [];
      const all = document.body ? document.body.querySelectorAll('*') : [];
      for (let i = 0; i < all.length; i++) {
        const el = all[i];
        if (isLandmark(el, role)) out.push(el);
      }
      return out;
    };

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

    // Detect same-role nesting (e.g. <main><main>).
    const nestedSameRole = [];
    const checkNesting = function(role) {
      const els = collect(role);
      for (let i = 0; i < els.length; i++) {
        const a = els[i];
        for (let j = 0; j < els.length; j++) {
          if (i === j) continue;
          const b = els[j];
          if (a.contains(b)) {
            nestedSameRole.push({
              role: role,
              outer: selectorOf(a),
              inner: selectorOf(b),
            });
          }
        }
      }
    };
    for (const r of ['main', 'banner', 'contentinfo', 'navigation', 'complementary']) {
      checkNesting(r);
    }

    return {
      pageUrl: window.location.href,
      mainCount: collect('main').length,
      bannerCount: collect('banner').length,
      contentinfoCount: collect('contentinfo').length,
      navigationCount: collect('navigation').length,
      complementaryCount: collect('complementary').length,
      nestedSameRole: nestedSameRole,
    };
})()"##;

/// One nested-landmark observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedLandmark {
    /// ARIA role of the nesting (main / banner / etc.).
    pub role: String,
    /// Outer landmark's selector.
    pub outer: String,
    /// Inner (nested) landmark's selector.
    pub inner: String,
}

/// Eval result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLandmarksSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// Count of `<main>` / `[role=main]`.
    pub main_count: u32,
    /// Count of top-level `<header>` / `[role=banner]`.
    pub banner_count: u32,
    /// Count of top-level `<footer>` / `[role=contentinfo]`.
    pub contentinfo_count: u32,
    /// Count of `<nav>` / `[role=navigation]`.
    pub navigation_count: u32,
    /// Count of `<aside>` / `[role=complementary]`.
    pub complementary_count: u32,
    /// Same-role nesting violations (e.g. `<main><main>`).
    pub nested_same_role: Vec<NestedLandmark>,
}

/// Apply detection rules to a runtime-landmarks snapshot.
#[must_use]
pub fn detect_runtime_landmarks_issues(
    snap: &RuntimeLandmarksSnapshot,
) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    if snap.main_count == 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "landmarks.no-main".to_owned(),
            detail: "Document has no <main> landmark. Screen-reader users cannot jump to primary content with the main-landmark shortcut.".to_owned(),
        });
    } else if snap.main_count > 1 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "landmarks.multiple-main".to_owned(),
            detail: format!(
                "Document has {} <main> elements; should have exactly 1.",
                snap.main_count
            ),
        });
    }

    if snap.banner_count > 1 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "landmarks.multiple-banner".to_owned(),
            detail: format!(
                "Document has {} top-level banner landmarks (header outside article/section, or [role=banner]); should have at most 1.",
                snap.banner_count
            ),
        });
    }
    if snap.contentinfo_count > 1 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "landmarks.multiple-contentinfo".to_owned(),
            detail: format!(
                "Document has {} top-level contentinfo landmarks; should have at most 1.",
                snap.contentinfo_count
            ),
        });
    }

    for nest in &snap.nested_same_role {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "landmarks.nested-same-role".to_owned(),
            detail: format!(
                "{role} landmark nested inside another {role} landmark: outer={outer}, inner={inner}",
                role = nest.role,
                outer = nest.outer,
                inner = nest.inner,
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_balanced() {
        assert_eq!(
            RUNTIME_LANDMARKS_JS.matches('(').count(),
            RUNTIME_LANDMARKS_JS.matches(')').count()
        );
    }

    #[test]
    fn js_iife_shape() {
        assert!(RUNTIME_LANDMARKS_JS.starts_with("(() => {"));
        assert!(RUNTIME_LANDMARKS_JS.ends_with("})()"));
    }

    #[test]
    fn js_returns_required_keys() {
        for k in [
            "mainCount",
            "bannerCount",
            "contentinfoCount",
            "navigationCount",
            "complementaryCount",
            "nestedSameRole",
        ] {
            assert!(RUNTIME_LANDMARKS_JS.contains(k), "missing key: {k}");
        }
    }

    fn snap(
        main: u32,
        banner: u32,
        contentinfo: u32,
        nav: u32,
        comp: u32,
        nested: Vec<NestedLandmark>,
    ) -> RuntimeLandmarksSnapshot {
        RuntimeLandmarksSnapshot {
            page_url: "http://t/".to_owned(),
            main_count: main,
            banner_count: banner,
            contentinfo_count: contentinfo,
            navigation_count: nav,
            complementary_count: comp,
            nested_same_role: nested,
        }
    }

    #[test]
    fn canonical_page_passes() {
        // 1 main + 1 banner + 1 contentinfo + 1 nav + 1 aside, no nesting.
        let s = snap(1, 1, 1, 1, 1, vec![]);
        assert!(detect_runtime_landmarks_issues(&s).is_empty());
    }

    #[test]
    fn no_main_fires_strict() {
        let s = snap(0, 1, 1, 0, 0, vec![]);
        let f = detect_runtime_landmarks_issues(&s);
        assert!(f.iter().any(|x| x.kind == "landmarks.no-main"));
    }

    #[test]
    fn multiple_main_fires_strict() {
        let s = snap(2, 1, 1, 0, 0, vec![]);
        let f = detect_runtime_landmarks_issues(&s);
        let m = f.iter().find(|x| x.kind == "landmarks.multiple-main");
        assert!(m.is_some());
        assert!(m.unwrap().detail.contains("2"));
    }

    #[test]
    fn multiple_banner_fires_strict() {
        let s = snap(1, 2, 1, 0, 0, vec![]);
        let f = detect_runtime_landmarks_issues(&s);
        assert!(f.iter().any(|x| x.kind == "landmarks.multiple-banner"));
    }

    #[test]
    fn multiple_contentinfo_fires_strict() {
        let s = snap(1, 1, 3, 0, 0, vec![]);
        let f = detect_runtime_landmarks_issues(&s);
        assert!(f.iter().any(|x| x.kind == "landmarks.multiple-contentinfo"));
    }

    #[test]
    fn multiple_nav_is_fine() {
        // ARIA permits multiple <nav> as long as each has a unique
        // aria-label. We don't audit the labelling here; just count.
        let s = snap(1, 1, 1, 3, 0, vec![]);
        let f = detect_runtime_landmarks_issues(&s);
        assert!(!f.iter().any(|x| x.kind.contains("navigation")));
    }

    #[test]
    fn nested_main_fires_strict() {
        let s = snap(
            2,
            1,
            1,
            0,
            0,
            vec![NestedLandmark {
                role: "main".to_owned(),
                outer: "body > main".to_owned(),
                inner: "body > main > main".to_owned(),
            }],
        );
        let f = detect_runtime_landmarks_issues(&s);
        let nest = f.iter().find(|x| x.kind == "landmarks.nested-same-role");
        assert!(nest.is_some());
        assert!(nest.unwrap().detail.contains("main"));
    }

    #[test]
    fn nested_nav_fires_strict() {
        let s = snap(
            1,
            1,
            1,
            2,
            0,
            vec![NestedLandmark {
                role: "navigation".to_owned(),
                outer: "body > nav".to_owned(),
                inner: "body > nav > nav".to_owned(),
            }],
        );
        let f = detect_runtime_landmarks_issues(&s);
        assert!(f.iter().any(|x| x.kind == "landmarks.nested-same-role"));
    }

    #[test]
    fn no_banner_no_contentinfo_is_fine() {
        // Some pages legitimately omit banner/contentinfo (settings,
        // chromeless modes). We only flag MULTIPLE, not zero.
        let s = snap(1, 0, 0, 0, 0, vec![]);
        let f = detect_runtime_landmarks_issues(&s);
        assert!(!f.iter().any(|x| x.kind.contains("banner")));
        assert!(!f.iter().any(|x| x.kind.contains("contentinfo")));
    }

    #[test]
    fn snapshot_round_trips() {
        let s = snap(1, 1, 1, 2, 1, vec![]);
        let json = serde_json::to_string(&s).expect("ser");
        let back: RuntimeLandmarksSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.main_count, 1);
        assert_eq!(back.navigation_count, 2);
    }
}
