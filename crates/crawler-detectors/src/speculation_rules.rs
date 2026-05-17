//! `speculation_rules` — Speculation Rules API audit. T76 port of
//! `src/speculationRules.ts`.
//!
//! The Speculation Rules API (W3C Editor's Draft 2024, shipping in
//! Chromium 119+) lets pages declare URLs the browser should
//! speculatively prefetch or prerender. Prerendering downloads +
//! parses + EXECUTES the target page in a hidden tab BEFORE the
//! user opts in to navigation — a privacy hazard when the target
//! is cross-origin.
//!
//! Findings (mirror TS byte-for-byte):
//!
//!   * `speculation-rules.invalid-json`                          warn
//!   * `speculation-rules.empty-rule-set`                        warn
//!   * `speculation-rules.cross-origin-prerender-no-anonymous-ip` STRICT
//!   * `speculation-rules.legacy-cross-origin-urls-form`          warn
//!   * `speculation-rules.eager-eagerness`                        warn
//!
//! SUPERSOCIETY: prerendering is exactly the kind of "browser
//! helpfully fetches things before the user asked" feature that
//! lets a malicious or sloppy page leak the user's browsing
//! pattern to third parties. The strict finding fires when a
//! cross-origin prerender lacks the `requires:
//! ["anonymous-client-ip-when-cross-origin"]` opt-out.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured `<script type="speculationrules">` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CapturedSpecRulesBlock {
    /// Raw text content of the `<script>` tag.
    pub raw_text: String,
    /// True iff JSON parsed.
    pub parsed_ok: bool,
    /// Parse error message if `parsed_ok` is false.
    pub parse_error: Option<String>,
    /// Number of rules under `prefetch`.
    pub prefetch_count: usize,
    /// Number of rules under `prerender`.
    pub prerender_count: usize,
    /// True iff any rule's eagerness is `"eager"`.
    pub has_eager: bool,
    /// True iff any rule uses the legacy `urls` list form with a
    /// cross-origin URL.
    pub has_legacy_cross_origin_urls: bool,
    /// True iff any prerender rule targets a cross-origin URL AND
    /// fails to declare `requires:
    /// ["anonymous-client-ip-when-cross-origin"]`.
    pub has_unshielded_cross_origin_prerender: bool,
    /// First 5 unshielded examples for the audit trail.
    pub unshielded_cross_origin_examples: Vec<String>,
    /// First 5 legacy-form examples.
    pub legacy_cross_origin_url_examples: Vec<String>,
    /// True iff parsed JSON has prefetch/prerender keys but every rule list is empty.
    pub is_empty_rule_set: bool,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SpeculationRulesSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// `<scheme>://<host>` of the page.
    pub page_origin: String,
    /// Zero or more `<script type="speculationrules">` blocks.
    pub blocks: Vec<CapturedSpecRulesBlock>,
}

/// Pure detector: snapshot → findings.
pub fn detect_speculation_rules_issues(snap: &SpeculationRulesSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();

    for block in &snap.blocks {
        if !block.parsed_ok {
            let err = block.parse_error.as_deref().unwrap_or("(unknown)");
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "speculation-rules.invalid-json".into(),
                detail: format!(
                    "A <script type=\"speculationrules\"> block contains invalid JSON: {err}. The browser silently drops the ENTIRE block — every rule inside fails. A typo here looks like \"performance fine but features somehow broken\" with no console error visible to the operator. Validate the JSON."
                ),
            });
            continue;
        }

        if block.is_empty_rule_set {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "speculation-rules.empty-rule-set".into(),
                detail: "A <script type=\"speculationrules\"> block parses to JSON with prefetch/prerender keys but every rule list is empty — almost always a copy-paste error. Either remove the block or populate at least one rule.".into(),
            });
        }

        if block.has_unshielded_cross_origin_prerender {
            let ex = block
                .unshielded_cross_origin_examples
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            out.push(AxisFinding {
                severity: AxisSeverity::Strict,
                kind: "speculation-rules.cross-origin-prerender-no-anonymous-ip".into(),
                detail: format!(
                    "Prerender rule targets a cross-origin URL WITHOUT declaring \"requires\": [\"anonymous-client-ip-when-cross-origin\"]. The browser will fetch + parse + execute the target page in a hidden tab BEFORE the user opts in to navigation, leaking IP + browser fingerprint + Accept-Language to the cross-origin server with no user interaction. Add the requires clause OR switch to \"prefetch\" (no JS execution). Examples: {ex}"
                ),
            });
        }

        if block.has_legacy_cross_origin_urls {
            let ex = block
                .legacy_cross_origin_url_examples
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "speculation-rules.legacy-cross-origin-urls-form".into(),
                detail: format!(
                    "A rule uses the legacy {{\"urls\": [...]}} list form with a cross-origin URL. The 2024 W3C draft restricts cross-origin targets to the document-rules form with \"where\" predicates + explicit referrer-policy controls; the legacy form is being phased out. Migrate to {{\"where\": {{...}}, \"referrer_policy\": \"strict-origin-when-cross-origin\"}} or similar. Examples: {ex}"
                ),
            });
        }

        if block.has_eager {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "speculation-rules.eager-eagerness".into(),
                detail: "A rule has \"eagerness\": \"eager\" — the browser starts the prefetch/prerender as soon as the rule is seen, before any user interaction. On metered connections or large assets this burns the user's mobile data budget without their knowledge. Prefer \"moderate\" (start on hover, the default) or \"conservative\" (start on pointerdown).".into(),
            });
        }
    }

    out
}

/// Browser-side DOM-capture script. Pinned to the TS source's
/// `SPECULATION_RULES_DOM_CAPTURE_JS` template literal so the
/// future chromiumoxide path produces identical snapshots to the
/// current Playwright path.
pub const SPECULATION_RULES_DOM_CAPTURE_JS: &str = r#"
(function() {
  function originOf(u) {
    try {
      var x = new URL(u, document.baseURI);
      return x.protocol + '//' + x.host;
    } catch (_) { return ''; }
  }
  var pageOrigin = window.location.origin;
  var nodes = document.querySelectorAll('script[type="speculationrules"]');
  var blocks = [];

  for (var i = 0; i < nodes.length; i++) {
    var raw = nodes[i].textContent || '';
    var parsed = null;
    var parseError = null;
    try {
      parsed = JSON.parse(raw);
    } catch (e) {
      parseError = (e && e.message) ? String(e.message) : 'parse failed';
    }

    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      blocks.push({
        rawText: raw,
        parsedOk: parseError === null && parsed !== null,
        parseError: parseError,
        prefetchCount: 0,
        prerenderCount: 0,
        hasEager: false,
        hasLegacyCrossOriginUrls: false,
        hasUnshieldedCrossOriginPrerender: false,
        unshieldedCrossOriginExamples: [],
        legacyCrossOriginUrlExamples: [],
        isEmptyRuleSet: false,
      });
      continue;
    }

    var prefetch = Array.isArray(parsed.prefetch) ? parsed.prefetch : [];
    var prerender = Array.isArray(parsed.prerender) ? parsed.prerender : [];
    var hasEager = false;
    var hasLegacy = false;
    var hasUnshielded = false;
    var legacyExamples = [];
    var unshieldedExamples = [];

    function inspectRule(rule, action) {
      if (!rule || typeof rule !== 'object') return;
      if (rule.eagerness === 'eager') hasEager = true;
      var requires = Array.isArray(rule.requires) ? rule.requires : [];
      var hasAnonymousIp = requires.indexOf(
        'anonymous-client-ip-when-cross-origin'
      ) >= 0;
      if (Array.isArray(rule.urls)) {
        for (var k = 0; k < rule.urls.length; k++) {
          var u = String(rule.urls[k]);
          var resolved;
          try {
            resolved = new URL(u, document.baseURI).toString();
          } catch (_) { continue; }
          var ro = originOf(resolved);
          var isCross = ro !== pageOrigin && ro !== '';
          if (isCross) {
            if (legacyExamples.length < 5) legacyExamples.push(resolved);
            hasLegacy = true;
            if (action === 'prerender' && !hasAnonymousIp) {
              if (unshieldedExamples.length < 5) unshieldedExamples.push(resolved);
              hasUnshielded = true;
            }
          }
        }
      }
      if (rule.where && typeof rule.where === 'object') {
        var hrefMatches = rule.where.href_matches;
        var arr = Array.isArray(hrefMatches) ? hrefMatches : (hrefMatches ? [hrefMatches] : []);
        for (var m = 0; m < arr.length; m++) {
          var pat = String(arr[m]);
          var schemeMatch = pat.match(/^https?:\/\/[^\/]+/);
          if (schemeMatch) {
            var pro = originOf(schemeMatch[0]);
            if (pro !== pageOrigin && pro !== '') {
              if (action === 'prerender' && !hasAnonymousIp) {
                if (unshieldedExamples.length < 5) unshieldedExamples.push(pat);
                hasUnshielded = true;
              }
            }
          }
        }
      }
    }

    for (var p = 0; p < prefetch.length; p++) inspectRule(prefetch[p], 'prefetch');
    for (var q = 0; q < prerender.length; q++) inspectRule(prerender[q], 'prerender');

    var isEmpty = prefetch.length === 0 && prerender.length === 0;

    blocks.push({
      rawText: raw,
      parsedOk: true,
      parseError: null,
      prefetchCount: prefetch.length,
      prerenderCount: prerender.length,
      hasEager: hasEager,
      hasLegacyCrossOriginUrls: hasLegacy,
      hasUnshieldedCrossOriginPrerender: hasUnshielded,
      unshieldedCrossOriginExamples: unshieldedExamples,
      legacyCrossOriginUrlExamples: legacyExamples,
      isEmptyRuleSet: isEmpty,
    });
  }

  return {
    pageUrl: window.location.href,
    pageOrigin: pageOrigin,
    blocks: blocks,
  };
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn block_default() -> CapturedSpecRulesBlock {
        CapturedSpecRulesBlock {
            raw_text: String::new(),
            parsed_ok: true,
            parse_error: None,
            prefetch_count: 0,
            prerender_count: 0,
            has_eager: false,
            has_legacy_cross_origin_urls: false,
            has_unshielded_cross_origin_prerender: false,
            unshielded_cross_origin_examples: Vec::new(),
            legacy_cross_origin_url_examples: Vec::new(),
            is_empty_rule_set: false,
        }
    }

    fn snap(blocks: Vec<CapturedSpecRulesBlock>) -> SpeculationRulesSnapshot {
        SpeculationRulesSnapshot {
            page_url: "https://example.com/".into(),
            page_origin: "https://example.com".into(),
            blocks,
        }
    }

    #[test]
    fn no_blocks_no_findings() {
        let s = snap(Vec::new());
        assert!(detect_speculation_rules_issues(&s).is_empty());
    }

    #[test]
    fn invalid_json_warns() {
        let mut b = block_default();
        b.parsed_ok = false;
        b.parse_error = Some("unexpected token at line 3".into());
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        assert!(f.iter().any(|x| x.kind == "speculation-rules.invalid-json"));
    }

    #[test]
    fn empty_rule_set_warns() {
        let mut b = block_default();
        b.is_empty_rule_set = true;
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        assert!(f
            .iter()
            .any(|x| x.kind == "speculation-rules.empty-rule-set"));
    }

    #[test]
    fn unshielded_cross_origin_prerender_is_strict() {
        let mut b = block_default();
        b.prerender_count = 1;
        b.has_unshielded_cross_origin_prerender = true;
        b.unshielded_cross_origin_examples = vec!["https://cdn.example.org/article".into()];
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        let strict = f
            .iter()
            .find(|x| x.kind == "speculation-rules.cross-origin-prerender-no-anonymous-ip")
            .expect("strict finding present");
        assert_eq!(strict.severity, AxisSeverity::Strict);
        assert!(strict.detail.contains("https://cdn.example.org/article"));
    }

    #[test]
    fn legacy_cross_origin_urls_form_warns() {
        let mut b = block_default();
        b.has_legacy_cross_origin_urls = true;
        b.legacy_cross_origin_url_examples = vec!["https://other.example/x".into()];
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        assert!(f
            .iter()
            .any(|x| x.kind == "speculation-rules.legacy-cross-origin-urls-form"));
    }

    #[test]
    fn eager_eagerness_warns() {
        let mut b = block_default();
        b.has_eager = true;
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        assert!(f
            .iter()
            .any(|x| x.kind == "speculation-rules.eager-eagerness"));
    }

    #[test]
    fn invalid_json_short_circuits_other_findings() {
        // A block that fails to parse should ONLY emit invalid-json,
        // not other findings (no parsed data to base them on).
        let mut b = block_default();
        b.parsed_ok = false;
        b.parse_error = Some("syntax error".into());
        // Set flags that would otherwise fire — they should be ignored
        b.has_eager = true;
        b.has_unshielded_cross_origin_prerender = true;
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, "speculation-rules.invalid-json");
    }

    #[test]
    fn same_origin_prerender_is_silent() {
        let mut b = block_default();
        b.prerender_count = 1;
        // has_unshielded_cross_origin_prerender is false by default
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        assert!(f.is_empty());
    }

    #[test]
    fn multiple_blocks_aggregate_independently() {
        let mut b1 = block_default();
        b1.has_eager = true;
        let mut b2 = block_default();
        b2.has_unshielded_cross_origin_prerender = true;
        b2.unshielded_cross_origin_examples = vec!["https://cdn.example.org/x".into()];
        let f = detect_speculation_rules_issues(&snap(vec![b1, b2]));
        assert!(f
            .iter()
            .any(|x| x.kind == "speculation-rules.eager-eagerness"));
        assert!(f
            .iter()
            .any(|x| { x.kind == "speculation-rules.cross-origin-prerender-no-anonymous-ip" }));
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let mut b = block_default();
        b.prefetch_count = 2;
        b.has_eager = true;
        let s = snap(vec![b]);
        let j = serde_json::to_string(&s).expect("ser");
        let back: SpeculationRulesSnapshot = serde_json::from_str(&j).expect("de");
        assert_eq!(back.blocks[0].prefetch_count, 2);
        assert!(back.blocks[0].has_eager);
    }

    #[test]
    fn examples_capped_at_three_in_strict_finding() {
        let mut b = block_default();
        b.has_unshielded_cross_origin_prerender = true;
        b.unshielded_cross_origin_examples = vec![
            "https://a.example/x".into(),
            "https://b.example/x".into(),
            "https://c.example/x".into(),
            "https://d.example/x".into(),
            "https://e.example/x".into(),
        ];
        let f = detect_speculation_rules_issues(&snap(vec![b]));
        let strict = f
            .iter()
            .find(|x| x.kind == "speculation-rules.cross-origin-prerender-no-anonymous-ip")
            .unwrap();
        // 3 examples → 2 commas in the joined string
        let comma_count = strict.detail.matches(",").count();
        // Be defensive — count only those after "Examples:"
        let after = &strict.detail[strict.detail.find("Examples:").unwrap_or(0)..];
        let comma_after = after.matches(", ").count();
        assert_eq!(
            comma_after, 2,
            "expected 3 examples (2 separators), comma_count={comma_count}, after={after}"
        );
    }

    #[test]
    fn js_brackets_balanced() {
        let mut paren: i32 = 0;
        let mut brace: i32 = 0;
        let mut bracket: i32 = 0;
        for c in SPECULATION_RULES_DOM_CAPTURE_JS.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        assert_eq!(paren, 0, "unbalanced parens");
        assert_eq!(brace, 0, "unbalanced braces");
        assert_eq!(bracket, 0, "unbalanced brackets");
    }
}
