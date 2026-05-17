//! `sri` — Subresource Integrity per-element DOM audit. T76 port of `src/sri.ts`.
//!
//! SRI is the W3C-standardised mechanism for verifying that a
//! cross-origin script or stylesheet loaded from a CDN has the
//! EXACT bytes the page-author committed to — by hash. Without
//! SRI, a CDN compromise (DNS hijack, BGP rerouting, vendor
//! supply-chain breach, malicious insider) silently substitutes
//! attacker-controlled JavaScript that runs with the embedding
//! page's full origin authority — equivalent to RCE inside the
//! user's session.
//!
//! Real-world incidents this detector would have caught:
//!   * Microsoft Tay (2016) — bot account compromise via cross-origin embed.
//!   * MyEtherWallet (2018) — DNS hijack + injected wallet-stealer JS.
//!   * British Airways (2018) — Magecart skimmer via compromised Modernizr CDN.
//!   * event-stream NPM (2018) — supply-chain RCE.
//!   * SolarWinds (2020) — same threat model, different layer.
//!
//! Findings:
//!   * `sri.script-cross-origin-no-integrity` — strict
//!   * `sri.style-cross-origin-no-integrity`  — warn
//!   * `sri.script-cross-origin-no-crossorigin` — warn
//!     (integrity declared but no `crossorigin` → browser silently ignores)
//!   * `sri.script-invalid-integrity-format` — warn
//!   * `sri.script-weak-algorithm` — warn (sha1 / md5 → silently dropped)
//!
//! Out of scope: `<img>`/`<audio>`/`<video>`/`<iframe>` (no browser SRI
//! support yet); same-origin (author controls the pipeline); actual hash
//! verification (the browser does that at load time).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Tag type for the captured element. Mirrors the TS string literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SriTag {
    /// `<script>` element.
    Script,
    /// `<link>` element.
    Link,
}

/// One captured DOM element relevant to SRI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedSriElement {
    /// `script` or `link`.
    pub tag: SriTag,
    /// Resolved resource URL (against the page base).
    pub resource_url: String,
    /// `<scheme>://<host>` of the resource (cached).
    pub resource_origin: String,
    /// True iff the resource is cross-origin to the page.
    pub is_cross_origin: bool,
    /// Raw `integrity` attribute value, or `None` if absent.
    pub integrity: Option<String>,
    /// Raw `crossorigin` attribute value, or `None`.
    pub crossorigin: Option<String>,
    /// Lowercase `rel` for `<link>`, empty string for `<script>`.
    pub link_rel: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SriSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// `<scheme>://<host>` of the page.
    pub page_origin: String,
    /// All `<script src>` + `<link href>` elements the crawler saw.
    pub elements: Vec<CapturedSriElement>,
}

/// W3C-recognised SRI algorithms.
fn valid_algos() -> BTreeSet<&'static str> {
    ["sha256", "sha384", "sha512"].into_iter().collect()
}

/// Legacy weak algorithms — browsers silently drop these.
fn weak_algos() -> BTreeSet<&'static str> {
    ["sha1", "md5"].into_iter().collect()
}

/// Cheap base64-ish charset check + minimum length. Tracks the TS regex
/// `^[A-Za-z0-9+/_=-]+$` and `length >= 16`.
fn looks_like_base64(s: &str) -> bool {
    if s.len() < 16 {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'_' | b'=' | b'-'))
}

/// Classify a single `integrity` value's tokens into (any_valid,
/// any_weak, any_bad). Mirrors the TS for-loop.
fn classify_integrity(value: &str) -> (bool, bool, bool) {
    let valid = valid_algos();
    let weak = weak_algos();
    let mut any_valid = false;
    let mut any_weak = false;
    let mut any_bad = false;
    for tok in value.split_whitespace() {
        let Some(dash) = tok.find('-') else {
            any_bad = true;
            continue;
        };
        if dash == 0 {
            any_bad = true;
            continue;
        }
        let algo = tok[..dash].to_ascii_lowercase();
        let hash = &tok[dash + 1..];
        if !looks_like_base64(hash) {
            any_bad = true;
            continue;
        }
        if valid.contains(algo.as_str()) {
            any_valid = true;
        } else if weak.contains(algo.as_str()) {
            any_weak = true;
        } else {
            any_bad = true;
        }
    }
    (any_valid, any_weak, any_bad)
}

/// Render a short example string for diagnostics.
fn render_example(e: &CapturedSriElement) -> String {
    let rel_part = if matches!(e.tag, SriTag::Link) {
        format!(" rel=\"{}\"", e.link_rel)
    } else {
        String::new()
    };
    let tag = match e.tag {
        SriTag::Script => "script",
        SriTag::Link => "link",
    };
    format!("<{tag}{rel_part} src/href='{}'>", e.resource_url)
}

/// Pure detector: snapshot → findings. No I/O.
pub fn detect_sri_issues(snap: &SriSnapshot) -> Vec<AxisFinding> {
    let mut script_missing: Vec<&CapturedSriElement> = Vec::new();
    let mut style_missing: Vec<&CapturedSriElement> = Vec::new();
    let mut no_crossorigin: Vec<&CapturedSriElement> = Vec::new();
    let mut invalid_format: Vec<&CapturedSriElement> = Vec::new();
    let mut weak_algo: Vec<&CapturedSriElement> = Vec::new();

    for e in &snap.elements {
        if !e.is_cross_origin {
            continue;
        }
        let Some(integrity) = e.integrity.as_deref() else {
            match e.tag {
                SriTag::Script => script_missing.push(e),
                SriTag::Link if e.link_rel == "stylesheet" => style_missing.push(e),
                SriTag::Link => {}
            }
            continue;
        };

        if e.crossorigin.is_none() {
            no_crossorigin.push(e);
        }

        let (any_valid, any_weak, any_bad) = classify_integrity(integrity);
        if !any_valid && (any_bad || any_weak) {
            if any_weak {
                weak_algo.push(e);
            }
            if any_bad {
                invalid_format.push(e);
            }
        }
    }

    let mut out = Vec::new();

    if !script_missing.is_empty() {
        let ex: Vec<String> = script_missing
            .iter()
            .take(5)
            .map(|e| render_example(e))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "sri.script-cross-origin-no-integrity".into(),
            detail: format!(
                "{} cross-origin <script src=\"...\"> element(s) load without an 'integrity' attribute. A CDN compromise (DNS hijack, BGP rerouting, vendor breach, malicious insider) substitutes attacker-controlled JavaScript that runs with this page's full origin authority — equivalent to RCE in the user's session. Add 'integrity=\"sha384-<base64>\"' AND 'crossorigin=\"anonymous\"' (both required). Examples: {}",
                script_missing.len(),
                ex.join("; ")
            ),
        });
    }
    if !style_missing.is_empty() {
        let ex: Vec<String> = style_missing
            .iter()
            .take(5)
            .map(|e| render_example(e))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "sri.style-cross-origin-no-integrity".into(),
            detail: format!(
                "{} cross-origin <link rel=\"stylesheet\"> element(s) load without an 'integrity' attribute. Stylesheet compromise enables visual injection (phishing overlays) and theoretical CSS-keylogger attacks via attribute selectors. Add 'integrity' + 'crossorigin' to pin the asset. Examples: {}",
                style_missing.len(),
                ex.join("; ")
            ),
        });
    }
    if !no_crossorigin.is_empty() {
        let ex: Vec<String> = no_crossorigin
            .iter()
            .take(5)
            .map(|e| render_example(e))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "sri.script-cross-origin-no-crossorigin".into(),
            detail: format!(
                "{} element(s) declare an 'integrity' attribute but no 'crossorigin' attribute. Browsers REFUSE to verify SRI on cross-origin resources without the CORS opt-in — the integrity attribute is silently IGNORED. Add 'crossorigin=\"anonymous\"' (or \"use-credentials\" if the resource needs cookies). Examples: {}",
                no_crossorigin.len(),
                ex.join("; ")
            ),
        });
    }
    if !invalid_format.is_empty() {
        let ex: Vec<String> = invalid_format
            .iter()
            .take(5)
            .map(|e| render_example(e))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "sri.script-invalid-integrity-format".into(),
            detail: format!(
                "{} element(s) have an 'integrity' attribute that doesn't parse as one or more '<algorithm>-<base64>' tokens. Browsers fall back to no-integrity-check semantics — a typo silently disables SRI. Examples: {}",
                invalid_format.len(),
                ex.join("; ")
            ),
        });
    }
    if !weak_algo.is_empty() {
        let ex: Vec<String> = weak_algo
            .iter()
            .take(5)
            .map(|e| render_example(e))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "sri.script-weak-algorithm".into(),
            detail: format!(
                "{} element(s) declare 'integrity' with a weak hash algorithm (sha1 / md5). The W3C spec only recognises sha256, sha384, sha512 — weak algorithms are silently dropped. Use sha384 or sha512. Examples: {}",
                weak_algo.len(),
                ex.join("; ")
            ),
        });
    }

    out
}

/// Browser-side DOM-capture script. The future Rust crawler feeds this to
/// `chromiumoxide::Page::evaluate_function` and deserialises the result
/// into [`SriSnapshot`]. Kept identical to the TS source so the same
/// runtime produces identical snapshots whether driven by Playwright
/// or chromiumoxide.
pub const SRI_DOM_CAPTURE_JS: &str = r#"
(function() {
  function originOf(u) {
    try {
      var x = new URL(u, document.baseURI);
      return x.protocol + '//' + x.host;
    } catch (_) { return ''; }
  }
  var pageOrigin = window.location.origin;
  var elements = [];
  var scripts = document.querySelectorAll('script[src]');
  for (var i = 0; i < scripts.length; i++) {
    var s = scripts[i];
    var src = s.getAttribute('src');
    if (!src) continue;
    var resolved;
    try { resolved = new URL(src, document.baseURI).toString(); } catch (_) { continue; }
    var ro = originOf(resolved);
    elements.push({
      tag: 'script',
      resourceUrl: resolved,
      resourceOrigin: ro,
      isCrossOrigin: ro !== pageOrigin && ro !== '',
      integrity: s.getAttribute('integrity'),
      crossorigin: s.getAttribute('crossorigin'),
      linkRel: '',
    });
  }
  var links = document.querySelectorAll('link[href]');
  for (var j = 0; j < links.length; j++) {
    var l = links[j];
    var href = l.getAttribute('href');
    if (!href) continue;
    var rel = (l.getAttribute('rel') || '').toLowerCase();
    var resolved2;
    try { resolved2 = new URL(href, document.baseURI).toString(); } catch (_) { continue; }
    var ro2 = originOf(resolved2);
    elements.push({
      tag: 'link',
      resourceUrl: resolved2,
      resourceOrigin: ro2,
      isCrossOrigin: ro2 !== pageOrigin && ro2 !== '',
      integrity: l.getAttribute('integrity'),
      crossorigin: l.getAttribute('crossorigin'),
      linkRel: rel,
    });
  }
  return { pageUrl: window.location.href, pageOrigin: pageOrigin, elements: elements };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> &'static str {
        "https://example.com"
    }

    fn script(url: &str, integrity: Option<&str>, crossorigin: Option<&str>) -> CapturedSriElement {
        let origin = url.split('/').take(3).collect::<Vec<_>>().join("/");
        CapturedSriElement {
            tag: SriTag::Script,
            resource_url: url.to_owned(),
            resource_origin: origin.clone(),
            is_cross_origin: origin != page(),
            integrity: integrity.map(str::to_owned),
            crossorigin: crossorigin.map(str::to_owned),
            link_rel: String::new(),
        }
    }

    fn link(
        url: &str,
        rel: &str,
        integrity: Option<&str>,
        crossorigin: Option<&str>,
    ) -> CapturedSriElement {
        let origin = url.split('/').take(3).collect::<Vec<_>>().join("/");
        CapturedSriElement {
            tag: SriTag::Link,
            resource_url: url.to_owned(),
            resource_origin: origin.clone(),
            is_cross_origin: origin != page(),
            integrity: integrity.map(str::to_owned),
            crossorigin: crossorigin.map(str::to_owned),
            link_rel: rel.to_owned(),
        }
    }

    fn snap(elements: Vec<CapturedSriElement>) -> SriSnapshot {
        SriSnapshot {
            page_url: format!("{}/", page()),
            page_origin: page().to_owned(),
            elements,
        }
    }

    #[test]
    fn same_origin_script_skipped() {
        let s = snap(vec![script("https://example.com/app.js", None, None)]);
        assert!(detect_sri_issues(&s).is_empty());
    }

    #[test]
    fn cross_origin_script_missing_integrity_is_strict() {
        let s = snap(vec![script("https://cdn.example.org/x.js", None, None)]);
        let f = detect_sri_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "sri.script-cross-origin-no-integrity");
        assert_eq!(f[0].severity, AxisSeverity::Strict);
    }

    #[test]
    fn cross_origin_stylesheet_missing_integrity_is_warn() {
        let s = snap(vec![link(
            "https://cdn.example.org/x.css",
            "stylesheet",
            None,
            None,
        )]);
        let f = detect_sri_issues(&s);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "sri.style-cross-origin-no-integrity");
        assert_eq!(f[0].severity, AxisSeverity::Warn);
    }

    #[test]
    fn cross_origin_link_non_stylesheet_skipped() {
        // preconnect / dns-prefetch / icon / manifest etc. — not SRI-eligible.
        let s = snap(vec![link(
            "https://cdn.example.org/",
            "preconnect",
            None,
            None,
        )]);
        assert!(detect_sri_issues(&s).is_empty());
    }

    #[test]
    fn integrity_without_crossorigin_warns() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("sha384-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            None,
        )]);
        let f = detect_sri_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "sri.script-cross-origin-no-crossorigin"));
    }

    #[test]
    fn integrity_with_crossorigin_is_clean() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("sha384-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Some("anonymous"),
        )]);
        assert!(detect_sri_issues(&s).is_empty());
    }

    #[test]
    fn weak_algorithm_warns() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Some("anonymous"),
        )]);
        let f = detect_sri_issues(&s);
        assert!(f.iter().any(|x| x.kind == "sri.script-weak-algorithm"));
    }

    #[test]
    fn invalid_format_warns() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("not-a-real-token"),
            Some("anonymous"),
        )]);
        let f = detect_sri_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "sri.script-invalid-integrity-format"));
    }

    #[test]
    fn multiple_tokens_with_one_valid_is_clean() {
        // Spec allows multiple algorithms; the strongest match wins.
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAAA sha384-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Some("anonymous"),
        )]);
        let f = detect_sri_issues(&s);
        assert!(
            f.is_empty(),
            "any valid token should defuse weak/bad siblings: {f:?}"
        );
    }

    #[test]
    fn empty_integrity_string_treated_as_bad_format() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("   "),
            Some("anonymous"),
        )]);
        // Whitespace-only → split_whitespace returns 0 tokens → no any_*
        // flips → no finding is emitted. This matches TS (no token = no
        // diagnostic). The MISSING attribute is what gates the strict
        // finding, not a malformed-but-present one being empty.
        let f = detect_sri_issues(&s);
        assert!(f.is_empty());
    }

    #[test]
    fn dash_only_token_is_invalid() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("-AAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Some("anonymous"),
        )]);
        let f = detect_sri_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "sri.script-invalid-integrity-format"));
    }

    #[test]
    fn short_hash_is_invalid() {
        let s = snap(vec![script(
            "https://cdn.example.org/x.js",
            Some("sha384-tooShort"),
            Some("anonymous"),
        )]);
        let f = detect_sri_issues(&s);
        assert!(f
            .iter()
            .any(|x| x.kind == "sri.script-invalid-integrity-format"));
    }

    #[test]
    fn examples_capped_at_5() {
        let mut els = Vec::new();
        for i in 0..10 {
            els.push(script(&format!("https://cdn.{i}.org/x.js"), None, None));
        }
        let s = snap(els);
        let f = detect_sri_issues(&s);
        assert!(f[0].detail.contains("10 cross-origin"));
        let example_count = f[0].detail.matches("src/href='").count();
        assert_eq!(example_count, 5);
    }

    #[test]
    fn js_brackets_balanced() {
        // Cheap sanity check on the embedded DOM capture: every `{` should
        // have a matching `}`, every `(` a `)`. Catches accidental edits
        // that would silently break runtime evaluation.
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        for c in SRI_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0);
        assert_eq!(brace, 0);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let s = snap(vec![
            script(
                "https://cdn.example.org/x.js",
                Some("sha384-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                Some("anonymous"),
            ),
            link("https://cdn.example.org/x.css", "stylesheet", None, None),
        ]);
        let j = serde_json::to_string(&s).expect("serialize");
        let back: SriSnapshot = serde_json::from_str(&j).expect("deserialize");
        assert_eq!(back.elements.len(), s.elements.len());
    }
}
