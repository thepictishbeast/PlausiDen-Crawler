//! `mixed_content` — HTTPS-page-loads-HTTP-resource detector.
//!
//! Mirror of `src/mixedContent.ts`. SECURITY-flavoured. Findings:
//!
//!   * `mixed-content.active`       strict   script/css/iframe/embed
//!                                           /object over http on
//!                                           an https page (browsers
//!                                           BLOCK these).
//!   * `mixed-content.passive`      warn     img/audio/video/srcset
//!                                           over http on an https
//!                                           page (browsers may
//!                                           auto-upgrade or block).
//!   * `mixed-content.form-action`  strict   `<form action="http://…">`
//!                                           on an https page —
//!                                           credentials/PII over the
//!                                           wire in the clear.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use serde::{Deserialize, Serialize};

/// Page-side eval. Returns assets only when the page itself is
/// https — mixed-content concept doesn't apply to http pages.
pub const MIXED_CONTENT_JS: &str = r##"(() => {
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
    const isHttp = function(u) {
      if (!u) return false;
      const s = u.trim().toLowerCase();
      return s.startsWith('http://');
    };
    const targets = [
      { sel: 'script[src]',                attr: 'src',    cls: 'active'  },
      { sel: 'link[rel="stylesheet"][href]', attr: 'href', cls: 'active'  },
      { sel: 'link[rel="preload"][href]',  attr: 'href',   cls: 'active'  },
      { sel: 'iframe[src]',                attr: 'src',    cls: 'active'  },
      { sel: 'embed[src]',                 attr: 'src',    cls: 'active'  },
      { sel: 'object[data]',               attr: 'data',   cls: 'active'  },
      { sel: 'img[src]',                   attr: 'src',    cls: 'passive' },
      { sel: 'audio[src]',                 attr: 'src',    cls: 'passive' },
      { sel: 'video[src]',                 attr: 'src',    cls: 'passive' },
      { sel: 'source[src]',                attr: 'src',    cls: 'passive' },
      { sel: 'video[poster]',              attr: 'poster', cls: 'passive' },
      { sel: 'form[action]',               attr: 'action', cls: 'form'    },
    ];
    const out = [];
    for (let t = 0; t < targets.length; t++) {
      const cfg = targets[t];
      const els = document.querySelectorAll(cfg.sel);
      for (let i = 0; i < els.length; i++) {
        const el = els[i];
        const url = el.getAttribute(cfg.attr) || '';
        if (!isHttp(url)) continue;
        out.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          url: url.slice(0, 200),
          attribute: cfg.attr,
          classification: cfg.cls,
        });
      }
    }
    const srcsetEls = document.querySelectorAll('img[srcset], source[srcset]');
    for (let i = 0; i < srcsetEls.length; i++) {
      const el = srcsetEls[i];
      const raw = el.getAttribute('srcset') || '';
      const parts = raw.split(',').map(function(p) { return p.trim().split(/\s+/)[0] || ''; });
      for (let p = 0; p < parts.length; p++) {
        if (!isHttp(parts[p])) continue;
        out.push({
          selector: selectorOf(el),
          tag: el.tagName.toLowerCase(),
          url: parts[p].slice(0, 200),
          attribute: 'srcset',
          classification: 'passive',
        });
      }
    }
    return { assets: out };
})()"##;

/// Captured mixed-content asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedMixedAsset {
    /// CSS selector.
    pub selector: String,
    /// Lowercased tag name.
    pub tag: String,
    /// http:// URL.
    pub url: String,
    /// Attribute that carried the URL: src/href/action/data/srcset/poster.
    pub attribute: String,
    /// active | passive | form.
    pub classification: String,
}

/// Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct MixedContentSnapshot {
    /// Page URL.
    pub page_url: String,
    /// True iff page itself was loaded over https.
    pub page_is_https: bool,
    /// Mixed-content assets captured (only populated when page_is_https).
    pub assets: Vec<CapturedMixedAsset>,
}

#[must_use]
pub fn detect_mixed_content_issues(
    snap: &MixedContentSnapshot,
) -> Vec<crate::AxisFinding> {
    if !snap.page_is_https {
        return Vec::new();
    }
    let mut active = Vec::<&CapturedMixedAsset>::new();
    let mut passive = Vec::<&CapturedMixedAsset>::new();
    let mut form = Vec::<&CapturedMixedAsset>::new();
    for a in &snap.assets {
        match a.classification.as_str() {
            "active" => active.push(a),
            "passive" => passive.push(a),
            "form" => form.push(a),
            _ => {}
        }
    }
    let render_ex = |a: &&CapturedMixedAsset| -> String {
        format!("{} {}[{}={}]", a.selector, a.tag, a.attribute, a.url)
    };
    let mut out = Vec::<crate::AxisFinding>::new();
    if !active.is_empty() {
        let examples: Vec<String> = active.iter().take(5).map(render_ex).collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "mixed-content.active".to_owned(),
            detail: format!(
                "{} active mixed-content asset(s): script/stylesheet/iframe/embed/object loaded over http on this https page. Browsers BLOCK these — the page renders with missing functionality. Switch to https:// or use protocol-relative URLs ('//host/path'). Examples: {}",
                active.len(),
                examples.join("; ")
            ),
        });
    }
    if !passive.is_empty() {
        let examples: Vec<String> = passive.iter().take(5).map(render_ex).collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "mixed-content.passive".to_owned(),
            detail: format!(
                "{} passive mixed-content asset(s): img/audio/video/srcset over http on this https page. Some browsers auto-upgrade, others render with a 'not secure' indicator, others block. Switch to https://. Examples: {}",
                passive.len(),
                examples.join("; ")
            ),
        });
    }
    if !form.is_empty() {
        let examples: Vec<String> = form.iter().take(5).map(render_ex).collect();
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Strict,
            kind: "mixed-content.form-action".to_owned(),
            detail: format!(
                "{} form(s) submit to http:// on this https page. Form contents (credentials, PII, payment data) travel IN THE CLEAR. Switch action=\"https://...\". Examples: {}",
                form.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(is_https: bool, assets: Vec<CapturedMixedAsset>) -> MixedContentSnapshot {
        MixedContentSnapshot {
            page_url: if is_https { "https://t/".to_owned() } else { "http://t/".to_owned() },
            page_is_https: is_https,
            assets,
        }
    }

    fn asset(class_: &str, tag: &str) -> CapturedMixedAsset {
        CapturedMixedAsset {
            selector: format!("body > {tag}"),
            tag: tag.to_owned(),
            url: "http://other.example/x".to_owned(),
            attribute: "src".to_owned(),
            classification: class_.to_owned(),
        }
    }

    #[test]
    fn js_brackets_balanced() {
        assert_eq!(MIXED_CONTENT_JS.matches('(').count(), MIXED_CONTENT_JS.matches(')').count());
        assert_eq!(MIXED_CONTENT_JS.matches('{').count(), MIXED_CONTENT_JS.matches('}').count());
    }

    #[test]
    fn http_page_never_fires() {
        let f = detect_mixed_content_issues(&snap(false, vec![asset("active", "script")]));
        assert!(f.is_empty());
    }

    #[test]
    fn clean_https_no_findings() {
        assert!(detect_mixed_content_issues(&snap(true, vec![])).is_empty());
    }

    #[test]
    fn active_strict() {
        let f = detect_mixed_content_issues(&snap(true, vec![asset("active", "script")]));
        assert!(f.iter().any(|x| x.kind == "mixed-content.active"
            && x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn passive_warn() {
        let f = detect_mixed_content_issues(&snap(true, vec![asset("passive", "img")]));
        assert!(f.iter().any(|x| x.kind == "mixed-content.passive"
            && x.severity == crate::AxisSeverity::Warn));
    }

    #[test]
    fn form_strict() {
        let f = detect_mixed_content_issues(&snap(true, vec![asset("form", "form")]));
        assert!(f.iter().any(|x| x.kind == "mixed-content.form-action"
            && x.severity == crate::AxisSeverity::Strict));
    }

    #[test]
    fn three_classes_three_findings() {
        let f = detect_mixed_content_issues(&snap(true, vec![
            asset("active", "script"),
            asset("passive", "img"),
            asset("form", "form"),
        ]));
        assert_eq!(f.len(), 3, "{:?}", f);
    }

    #[test]
    fn aggregation() {
        let mut assets = Vec::new();
        for _ in 0..6 {
            assets.push(asset("active", "script"));
        }
        let f = detect_mixed_content_issues(&snap(true, assets));
        let active = f.iter().find(|x| x.kind == "mixed-content.active").expect("found");
        assert!(active.detail.starts_with("6 active"), "{}", active.detail);
    }

    #[test]
    fn examples_capped_at_five() {
        let mut assets = Vec::new();
        for _ in 0..10 {
            assets.push(asset("passive", "img"));
        }
        let f = detect_mixed_content_issues(&snap(true, assets));
        let passive = f.iter().find(|x| x.kind == "mixed-content.passive").expect("found");
        assert_eq!(passive.detail.matches("; ").count(), 4);
    }
}
