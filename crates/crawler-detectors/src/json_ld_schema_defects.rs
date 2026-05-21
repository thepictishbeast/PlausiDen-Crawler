//! `json_ld_schema_defects` — flags broken
//! `<script type="application/ld+json">` schema.org blocks.
//!
//! Search engines (Google / Bing) consume JSON-LD structured
//! data for rich-snippet display. Defects silently break the
//! enhanced search result (recipe cards, breadcrumb trails,
//! product pricing, article author display, organization
//! cards). The page still ranks; just without the rich UI.
//!
//! Four defect classes:
//!
//! 1. **Invalid JSON** (Strict). Trailing comma / unquoted key
//!    / smart-quote substitution / unclosed brace. Parsers
//!    silently ignore the entire block.
//!
//! 2. **Missing `@context`** (Strict). Schema.org requires
//!    `@context: "https://schema.org"`. Without it the block
//!    has no vocabulary anchor and is meaningless.
//!
//! 3. **Missing `@type`** (Strict). Each schema.org item must
//!    declare its type (`"Article"`, `"Product"`,
//!    `"Organization"`, …). Without it consumers can't
//!    interpret the fields.
//!
//! 4. **Wrong `@context` URL** (Warn). Accepts both
//!    `https://schema.org` and `http://schema.org` (legacy);
//!    flag anything else. Common bug: `"@context":
//!    "schema.org"` (missing scheme) is technically still
//!    valid per JSON-LD 1.1 with a base URL but is fragile
//!    and inconsistent across consumers.
//!
//! Honors `data-jsonld-allow="true"` opt-out on the `<script>`
//! for measured edge cases (custom vocabularies, JSON-LD 1.1
//! advanced @context forms that the simple check rejects).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector, no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending JSON-LD block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct JsonLdSchemaDefectHit {
    /// CSS-ish path of the offending `<script>`.
    pub selector: String,
    /// First 120 chars of the offending body (trimmed).
    pub body_excerpt: String,
    /// Defect kind — one of `"invalid-json"`,
    /// `"missing-context"`, `"missing-type"`,
    /// `"wrong-context-url"`.
    pub defect_kind: String,
    /// Free-text detail (the parse error / wrong @context
    /// value / etc.) — capped at 120 chars.
    pub defect_detail: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct JsonLdSchemaDefectSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending JSON-LD blocks.
    pub hits: Vec<JsonLdSchemaDefectHit>,
    /// Total `<script type="application/ld+json">` blocks.
    pub scanned_blocks: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_json_ld_schema_defects(snap: &JsonLdSchemaDefectSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut invalid_json: Vec<&JsonLdSchemaDefectHit> = Vec::new();
    let mut missing_context: Vec<&JsonLdSchemaDefectHit> = Vec::new();
    let mut missing_type: Vec<&JsonLdSchemaDefectHit> = Vec::new();
    let mut wrong_context: Vec<&JsonLdSchemaDefectHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "invalid-json" => invalid_json.push(h),
            "missing-context" => missing_context.push(h),
            "missing-type" => missing_type.push(h),
            "wrong-context-url" => wrong_context.push(h),
            _ => {} // defensive: unknown kinds dropped
        }
    }

    let format_example = |h: &JsonLdSchemaDefectHit| -> String {
        format!("{} — {}", h.selector, h.defect_detail)
    };

    let mut out = Vec::new();
    if !invalid_json.is_empty() {
        let examples: Vec<String> = invalid_json
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "json-ld.invalid-json".to_owned(),
            detail: format!(
                "{} JSON-LD block(s) failed to parse — search engines silently ignore. Validate with the Google Rich Results Test before shipping. Examples: {}",
                invalid_json.len(),
                examples.join("; ")
            ),
        });
    }
    if !missing_context.is_empty() {
        let examples: Vec<String> = missing_context
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "json-ld.missing-context".to_owned(),
            detail: format!(
                "{} JSON-LD block(s) missing `@context` — no vocabulary anchor, the block has no meaning. Add `\"@context\": \"https://schema.org\"`. Examples: {}",
                missing_context.len(),
                examples.join("; ")
            ),
        });
    }
    if !missing_type.is_empty() {
        let examples: Vec<String> = missing_type
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "json-ld.missing-type".to_owned(),
            detail: format!(
                "{} JSON-LD block(s) missing `@type` — consumers cannot interpret the fields. Declare `\"@type\": \"Article\"` (or `Product` / `Organization` / etc.). Examples: {}",
                missing_type.len(),
                examples.join("; ")
            ),
        });
    }
    if !wrong_context.is_empty() {
        let examples: Vec<String> = wrong_context
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "json-ld.wrong-context-url".to_owned(),
            detail: format!(
                "{} JSON-LD block(s) declare a non-schema.org `@context` URL — fragile across consumers. Use `\"https://schema.org\"` (or `\"http://schema.org\"` for legacy). Examples: {}",
                wrong_context.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every
/// `<script type="application/ld+json">`, tries to parse, then
/// inspects @context / @type. Multi-defect blocks emit one hit
/// per kind so each finding bucket counts independently.
pub const JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.head && node !== document.body && depth < 6) {
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
      return 'head > ' + parts.join(' > ');
    };

    const SCHEMA_ORG_CONTEXTS = new Set([
      'https://schema.org',
      'http://schema.org',
      'https://schema.org/',
      'http://schema.org/'
    ]);

    // Walks a parsed JSON-LD node (object or array) and checks
    // each item for @context / @type / wrong-context-url.
    // Returns array of defects.
    const inspect = function(parsed) {
      const defects = [];
      const items = Array.isArray(parsed) ? parsed : [parsed];
      for (const item of items) {
        if (typeof item !== 'object' || item === null) continue;
        const ctx = item['@context'];
        const typ = item['@type'];
        if (ctx == null) {
          defects.push({ kind: 'missing-context', detail: 'no @context key' });
        } else if (typeof ctx === 'string' && !SCHEMA_ORG_CONTEXTS.has(ctx)) {
          defects.push({ kind: 'wrong-context-url', detail: 'got `' + ctx.substring(0, 100) + '`' });
        }
        if (typ == null) {
          defects.push({ kind: 'missing-type', detail: 'no @type key' });
        }
      }
      return defects;
    };

    const hits = [];
    let scanned = 0;
    const blocks = document.querySelectorAll('script[type="application/ld+json"]');
    for (const block of blocks) {
      if (block.getAttribute && block.getAttribute('data-jsonld-allow') === 'true') continue;
      scanned += 1;
      const body = (block.textContent || '').trim();
      const excerpt = body.substring(0, 120);
      let parsed;
      try {
        parsed = JSON.parse(body);
      } catch (e) {
        const msg = (e && e.message) ? e.message.substring(0, 120) : 'parse error';
        hits.push({
          selector: selectorOf(block),
          bodyExcerpt: excerpt,
          defectKind: 'invalid-json',
          defectDetail: msg
        });
        continue;
      }
      const defects = inspect(parsed);
      for (const d of defects) {
        hits.push({
          selector: selectorOf(block),
          bodyExcerpt: excerpt,
          defectKind: d.kind,
          defectDetail: d.detail
        });
      }
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedBlocks: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(selector: &str, defect_kind: &str, defect_detail: &str) -> JsonLdSchemaDefectHit {
        JsonLdSchemaDefectHit {
            selector: selector.into(),
            body_excerpt: "{\"@type\": \"Article\"}".into(),
            defect_kind: defect_kind.into(),
            defect_detail: defect_detail.into(),
        }
    }

    fn snap(hits: Vec<JsonLdSchemaDefectHit>) -> JsonLdSchemaDefectSnapshot {
        JsonLdSchemaDefectSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_blocks: 3,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_json_ld_schema_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn invalid_json_is_strict() {
        let s = snap(vec![hit(
            "head > script:nth-of-type(2)",
            "invalid-json",
            "Unexpected token } at line 4",
        )]);
        let findings = detect_json_ld_schema_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "json-ld.invalid-json");
        assert!(findings[0].detail.contains("silently ignore"));
        assert!(findings[0].detail.contains("Unexpected token }"));
    }

    #[test]
    fn missing_context_is_strict() {
        let s = snap(vec![hit(
            "head > script:nth-of-type(1)",
            "missing-context",
            "no @context key",
        )]);
        let findings = detect_json_ld_schema_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "json-ld.missing-context");
        assert!(findings[0].detail.contains("vocabulary anchor"));
    }

    #[test]
    fn missing_type_is_strict() {
        let s = snap(vec![hit(
            "head > script:nth-of-type(1)",
            "missing-type",
            "no @type key",
        )]);
        let findings = detect_json_ld_schema_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "json-ld.missing-type");
        assert!(findings[0].detail.contains("cannot interpret"));
    }

    #[test]
    fn wrong_context_url_is_warn() {
        let s = snap(vec![hit(
            "head > script:nth-of-type(1)",
            "wrong-context-url",
            "got `schema.org`",
        )]);
        let findings = detect_json_ld_schema_defects(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "json-ld.wrong-context-url");
        assert!(findings[0].detail.contains("fragile across consumers"));
    }

    #[test]
    fn all_four_defects_emit_four_findings() {
        let s = snap(vec![
            hit(".a", "invalid-json", "parse error"),
            hit(".b", "missing-context", "no @context key"),
            hit(".c", "missing-type", "no @type key"),
            hit(".d", "wrong-context-url", "got `foo`"),
        ]);
        let findings = detect_json_ld_schema_defects(&s);
        assert_eq!(findings.len(), 4);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"json-ld.invalid-json"));
        assert!(kinds.contains(&"json-ld.missing-context"));
        assert!(kinds.contains(&"json-ld.missing-type"));
        assert!(kinds.contains(&"json-ld.wrong-context-url"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(".x", "future-defect", "x")]);
        let findings = detect_json_ld_schema_defects(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".s-{i}"),
                "invalid-json",
                "parse error",
            ));
        }
        let s = snap(hits);
        let findings = detect_json_ld_schema_defects(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 JSON-LD block(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + defect-kind contract.
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("hits"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("scannedBlocks"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("defectKind"));
        // All four defect-kind strings.
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'invalid-json'"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'missing-context'"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'missing-type'"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'wrong-context-url'"));
        // Selector contract.
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'script[type=\"application/ld+json\"]'"));
        // Schema.org context whitelist sample.
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'https://schema.org'"));
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("'http://schema.org'"));
        // Opt-out contract.
        assert!(JSON_LD_SCHEMA_DEFECTS_DOM_CAPTURE_JS.contains("data-jsonld-allow"));
    }
}
