//! `outbound_links` — outbound-link safety detector.
//!
//! Mirror of `src/outboundLinks.ts`. SECURITY-flavoured. Findings:
//!
//!   * `link.tabnab-vulnerable`        strict   _blank, no noopener
//!   * `link.opener-explicit`          strict   rel=opener (worst)
//!   * `link.outbound-no-noreferrer`   warn     leaks Referer
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Captures every outbound `<a href>` (different
/// origin from `window.location.origin`) along with its target +
/// rel tokens. Skips non-http(s) schemes (mailto:, tel:, etc.).
pub const OUTBOUND_LINKS_JS: &str = r##"(() => {
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

    const pageOrigin = window.location.origin;
    const out = [];
    const anchors = document.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i++) {
      const a = anchors[i];
      const href = a.getAttribute('href') || '';
      let absoluteUrl;
      try {
        absoluteUrl = new URL(href, document.baseURI);
      } catch (e) {
        continue;
      }
      if (absoluteUrl.protocol !== 'http:' && absoluteUrl.protocol !== 'https:') continue;
      const outbound = absoluteUrl.origin !== pageOrigin;
      if (!outbound) continue;
      const target = (a.getAttribute('target') || '').toLowerCase();
      const relRaw = (a.getAttribute('rel') || '').toLowerCase();
      const rel = relRaw ? relRaw.split(/\s+/).filter(Boolean) : [];
      out.push({
        selector: selectorOf(a),
        href: absoluteUrl.href,
        target: target,
        rel: rel,
        outbound: true,
      });
    }
    return { pageOrigin: pageOrigin, links: out };
})()"##;

/// Captured outbound link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedOutboundLink {
    /// Best-effort CSS selector.
    pub selector: String,
    /// Absolute URL.
    pub href: String,
    /// `target` attribute, lowercased.
    pub target: String,
    /// Lowercased `rel` tokens.
    pub rel: Vec<String>,
    /// True iff different origin from page (always true for entries
    /// in this snapshot — kept for round-trip with TS shape).
    pub outbound: bool,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct OutboundLinksSnapshot {
    /// Page URL.
    pub page_url: String,
    /// Page origin (scheme + host + port).
    pub page_origin: String,
    /// Captured outbound links (already filtered by capture-side).
    pub links: Vec<CapturedOutboundLink>,
}

/// Pure detector: snapshot → findings. Flags cross-origin `<a
/// target="_blank">` without `rel="noopener noreferrer"` — the
/// classic tab-nabbing + referer-leak SECURITY issue (covered by
/// MDN's "secure-by-default" guidance + OWASP A04 Insecure Design).
#[must_use]
pub fn detect_outbound_link_issues(snap: &OutboundLinksSnapshot) -> Vec<crate::AxisFinding> {
    let mut tabnab = Vec::<&CapturedOutboundLink>::new();
    let mut opener_explicit = Vec::<&CapturedOutboundLink>::new();
    let mut no_noreferrer = Vec::<&CapturedOutboundLink>::new();

    for l in &snap.links {
        if !l.outbound {
            continue;
        }
        let is_blank = l.target == "_blank";
        let has_noopener = l.rel.iter().any(|t| t == "noopener");
        let has_opener = l.rel.iter().any(|t| t == "opener");
        let has_noreferrer = l.rel.iter().any(|t| t == "noreferrer");

        if has_opener {
            opener_explicit.push(l);
        }
        if is_blank && !has_noopener && !has_opener {
            tabnab.push(l);
        }
        if !has_noreferrer {
            no_noreferrer.push(l);
        }
    }

    let mut out = Vec::<crate::AxisFinding>::new();

    if !tabnab.is_empty() {
        let examples: Vec<String> = tabnab
            .iter()
            .take(5)
            .map(|l| format!("{} → {}", l.selector, l.href))
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "link.tabnab-vulnerable".to_owned(),
            detail: format!(
                "{} outbound link(s) with target=\"_blank\" and no rel=\"noopener\". The destination page can navigate this tab to a phishing URL via window.opener (tabnabbing). Modern browsers default to noopener but older / embedded / downgraded clients do not. Add rel=\"noopener noreferrer\". Examples: {}",
                tabnab.len(),
                examples.join("; ")
            ),
        });
    }

    if !opener_explicit.is_empty() {
        let examples: Vec<String> = opener_explicit
            .iter()
            .take(5)
            .map(|l| format!("{} → {}", l.selector, l.href))
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "link.opener-explicit".to_owned(),
            detail: format!(
                "{} outbound link(s) explicitly set rel=\"opener\" — this OPTS BACK IN to the tabnabbing vulnerability that browser defaults are designed to prevent. Remove the 'opener' token. Examples: {}",
                opener_explicit.len(),
                examples.join("; ")
            ),
        });
    }

    if !no_noreferrer.is_empty() {
        let examples: Vec<String> = no_noreferrer
            .iter()
            .take(5)
            .map(|l| format!("{} → {}", l.selector, l.href))
            .collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "link.outbound-no-noreferrer".to_owned(),
            detail: format!(
                "{} outbound link(s) don't set rel=\"noreferrer\". The destination's analytics will see the current URL — leaking session tokens, internal paths, and traffic patterns to a third party. Add rel=\"noreferrer\" unless attribution is intentional. Examples: {}",
                no_noreferrer.len(),
                examples.join("; ")
            ),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(links: Vec<CapturedOutboundLink>) -> OutboundLinksSnapshot {
        OutboundLinksSnapshot {
            page_url: "https://example.com/".to_owned(),
            page_origin: "https://example.com".to_owned(),
            links,
        }
    }

    fn link(target: &str, rel: &[&str]) -> CapturedOutboundLink {
        CapturedOutboundLink {
            selector: "body > a".to_owned(),
            href: "https://other.example/".to_owned(),
            target: target.to_owned(),
            rel: rel.iter().map(|s| (*s).to_owned()).collect(),
            outbound: true,
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(
            OUTBOUND_LINKS_JS.matches('(').count(),
            OUTBOUND_LINKS_JS.matches(')').count()
        );
        assert_eq!(
            OUTBOUND_LINKS_JS.matches('{').count(),
            OUTBOUND_LINKS_JS.matches('}').count()
        );
    }

    #[test]
    fn blank_no_rel_strict_tabnab_and_warn_noreferrer() {
        let f = detect_outbound_link_issues(&snap(vec![link("_blank", &[])]));
        assert!(f.iter().any(
            |x| x.kind == "link.tabnab-vulnerable" && x.severity == crate::AxisSeverity::Strict
        ));
        assert!(f.iter().any(|x| x.kind == "link.outbound-no-noreferrer"));
    }

    #[test]
    fn blank_noopener_suppresses_tabnab() {
        let f = detect_outbound_link_issues(&snap(vec![link("_blank", &["noopener"])]));
        assert!(!f.iter().any(|x| x.kind == "link.tabnab-vulnerable"));
        assert!(f.iter().any(|x| x.kind == "link.outbound-no-noreferrer"));
    }

    #[test]
    fn blank_noopener_noreferrer_clean() {
        let f =
            detect_outbound_link_issues(&snap(vec![link("_blank", &["noopener", "noreferrer"])]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn opener_explicit_strict_no_double_tabnab() {
        let f = detect_outbound_link_issues(&snap(vec![link("_blank", &["opener"])]));
        assert!(
            f.iter()
                .any(|x| x.kind == "link.opener-explicit"
                    && x.severity == crate::AxisSeverity::Strict)
        );
        assert!(!f.iter().any(|x| x.kind == "link.tabnab-vulnerable"));
    }

    #[test]
    fn same_tab_no_tabnab_still_warns_noreferrer() {
        let f = detect_outbound_link_issues(&snap(vec![link("", &[])]));
        assert!(!f.iter().any(|x| x.kind == "link.tabnab-vulnerable"));
        assert!(f.iter().any(|x| x.kind == "link.outbound-no-noreferrer"));
    }

    #[test]
    fn same_tab_noreferrer_clean() {
        let f = detect_outbound_link_issues(&snap(vec![link("", &["noreferrer"])]));
        assert!(f.is_empty(), "{:?}", f);
    }

    #[test]
    fn aggregation_count() {
        let mut links = Vec::new();
        for _ in 0..7 {
            links.push(link("_blank", &[]));
        }
        let f = detect_outbound_link_issues(&snap(links));
        let tabnab = f
            .iter()
            .find(|x| x.kind == "link.tabnab-vulnerable")
            .expect("present");
        // count message contains the literal "7 outbound link(s)".
        assert!(
            tabnab.detail.starts_with("7 outbound link(s)"),
            "{}",
            tabnab.detail
        );
    }

    #[test]
    fn examples_capped_at_five() {
        let mut links = Vec::new();
        for _ in 0..12 {
            links.push(link("_blank", &[]));
        }
        let f = detect_outbound_link_issues(&snap(links));
        let tabnab = f
            .iter()
            .find(|x| x.kind == "link.tabnab-vulnerable")
            .expect("present");
        assert_eq!(tabnab.detail.matches("; ").count(), 4);
    }

    #[test]
    fn non_outbound_skipped() {
        let mut l = link("_blank", &[]);
        l.outbound = false;
        let f = detect_outbound_link_issues(&snap(vec![l]));
        assert!(f.is_empty(), "non-outbound link should be skipped");
    }
}
