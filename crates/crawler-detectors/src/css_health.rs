//! `css_health` — multi-eval CSS-loading health detector.
//!
//! Unlike the other axes, cssHealth runs **multiple small
//! `page.evaluate` calls** instead of one big IIFE. Each numbered
//! `pub const &str` below is one such call. The Rust runner
//! sequences them, then assembles a `CssHealthSnapshot`.
//!
//! Order matters:
//!
//! 1. `DECLARED_HREFS_JS`        — pull resolved-absolute
//!    `<link rel="stylesheet">` hrefs.
//! 2. `BRACE_COUNTS_JS`          — same-origin fetch each href
//!    inside the page; count `{` and `}` per sheet. Returned
//!    object's keys are the sheet URLs; the `_close` sub-map
//!    holds close-brace counts (TS workaround for parallel maps).
//! 3. `INLINE_STYLE_BLOCK_COUNT_JS`  — `<style>` element count.
//! 4. `COMPUTED_STYLES_JS`       — body + html computed styles
//!    (background, color, font-family, font-size, margin).
//! 5. `BODY_VISIBLE_TEXT_LENGTH_JS` — body innerText length.
//! 6. `APPLIED_RULE_COUNT_JS`    — recursive walk of
//!    `document.styleSheets[*].cssRules` (handles
//!    @media/@supports nesting).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// (1) Pull declared `<link rel="stylesheet">` hrefs, resolved.
pub const DECLARED_HREFS_JS: &str = r##"(() => {
    const out = [];
    document.querySelectorAll('link[rel="stylesheet"]').forEach((el) => {
      const href = el.href;
      if (href) out.push(href);
    });
    return out;
})()"##;

/// (2) Same-origin fetch of each sheet from inside the page;
/// count `{` and `}` per URL. Takes a JSON-encoded URL list as
/// input via `URLS_JSON` substitution at call time. The Rust
/// runner builds the call by replacing `URLS_JSON` with the JSON
/// array of URLs.
///
/// BUG ASSUMPTION: cross-origin sheets fail the fetch and return
/// `null` for that URL. The crawler-detector caller MUST tolerate
/// nulls — they don't indicate a broken sheet, just an unobservable
/// one (CORS).
pub const BRACE_COUNTS_JS_TEMPLATE: &str = r##"(async () => {
    const urls = URLS_JSON;
    const out = {};
    const close = {};
    for (const u of urls) {
      try {
        const r = await fetch(u, { cache: 'no-store' });
        if (!r.ok) {
          out[u] = null;
          continue;
        }
        const text = await r.text();
        out[u] = (text.match(/\{/g) || []).length;
        close[u] = (text.match(/\}/g) || []).length;
      } catch {
        out[u] = null;
      }
    }
    out._close = close;
    return out;
})()"##;

/// Build the brace-counts call by substituting the URL list.
/// Caller serializes its `Vec<String>` of URLs to JSON and the
/// helper inlines it into the template.
#[must_use]
pub fn brace_counts_js(urls: &[String]) -> String {
    let urls_json = serde_json::to_string(urls).unwrap_or_else(|_| "[]".to_owned());
    BRACE_COUNTS_JS_TEMPLATE.replace("URLS_JSON", &urls_json)
}

/// (3) `<style>` block count.
pub const INLINE_STYLE_BLOCK_COUNT_JS: &str = r##"document.querySelectorAll('style').length"##;

/// (4) body + html computed styles.
pub const COMPUTED_STYLES_JS: &str = r##"(() => {
    const bs = window.getComputedStyle(document.body);
    const hs = window.getComputedStyle(document.documentElement);
    return {
      body: {
        backgroundColor: bs.backgroundColor,
        color: bs.color,
        fontFamily: bs.fontFamily,
        fontSize: bs.fontSize,
        margin: bs.margin,
      },
      html: {
        backgroundColor: hs.backgroundColor,
      },
    };
})()"##;

/// (5) body visible text length (whitespace-collapsed).
pub const BODY_VISIBLE_TEXT_LENGTH_JS: &str =
    r##"(document.body.innerText || '').replace(/\s+/g, ' ').trim().length"##;

/// (6) Estimated count of CSS rules ACTUALLY applied (recursive
/// walk of `styleSheets[*].cssRules` + nested `@media` / `@supports`).
pub const APPLIED_RULE_COUNT_JS: &str = r##"(() => {
    let total = 0;
    const stack = [];
    for (const sheet of Array.from(document.styleSheets)) {
      try {
        const rules = sheet.cssRules;
        if (rules) stack.push(rules);
      } catch {
        // cross-origin or otherwise unreadable
      }
    }
    while (stack.length > 0) {
      const rules = stack.pop();
      for (let i = 0; i < rules.length; i++) {
        total += 1;
        const inner = rules[i].cssRules;
        if (inner && inner.length) stack.push(inner);
      }
    }
    return total;
})()"##;

/// Per-stylesheet observation. Mirrors the TS shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct StylesheetObservation {
    /// Resolved absolute URL.
    pub url: String,
    /// HTTP status from the network capture (0 = not observed).
    pub status: u16,
    /// Response Content-Type (None if not observed).
    pub content_type: Option<String>,
    /// Body byte count from the network capture.
    pub body_bytes: u64,
    /// `{` count from same-origin fetch (None on CORS / failure).
    pub declared_brace_count: Option<u32>,
    /// `}` count from same-origin fetch.
    pub declared_close_brace_count: Option<u32>,
    /// True if the network observer caught this URL.
    pub from_network: bool,
    /// Error text from the network observer.
    pub error_text: Option<String>,
}

/// Body + html computed styles slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ComputedBody {
    /// `getComputedStyle(body).backgroundColor`.
    pub background_color: String,
    /// color.
    pub color: String,
    /// font-family.
    pub font_family: String,
    /// font-size.
    pub font_size: String,
    /// margin shorthand.
    pub margin: String,
}

/// Just html background.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct ComputedHtml {
    /// background-color shorthand.
    pub background_color: String,
}

/// Combined snapshot. Assembled by the Rust runner after each of
/// the 6 evals returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct CssHealthSnapshot {
    /// Page URL at capture time.
    pub page_url: String,
    /// Per-sheet observations (network meta + brace counts).
    pub declared_sheets: Vec<StylesheetObservation>,
    /// `<style>` block count.
    pub inline_style_block_count: u32,
    /// body computed styles slice.
    pub computed_body: ComputedBody,
    /// html computed styles slice.
    pub computed_html: ComputedHtml,
    /// Whitespace-collapsed body text length.
    pub body_visible_text_length: u32,
    /// Applied-rule count from recursive styleSheets walk.
    pub applied_rule_count_estimate: u32,
}

/// Brace-counts return shape (raw, before consolidating into
/// `StylesheetObservation`). Consumers typically call
/// `split_close_braces` to peel the `_close` map off and get
/// per-URL maps.
pub type BraceCountsRaw = HashMap<String, serde_json::Value>;

/// Split the raw brace-counts response into open + close maps.
/// The TS encoded close braces under `_close` to avoid two
/// page.evaluate round-trips; same encoding here.
#[must_use]
pub fn split_close_braces(
    raw: BraceCountsRaw,
) -> (HashMap<String, Option<u32>>, HashMap<String, u32>) {
    let mut closes: HashMap<String, u32> = HashMap::new();
    let mut opens: HashMap<String, Option<u32>> = HashMap::new();
    for (k, v) in raw {
        if k == "_close" {
            if let serde_json::Value::Object(map) = v {
                for (url, count) in map {
                    if let Some(n) = count.as_u64() {
                        closes.insert(url, n as u32);
                    }
                }
            }
            continue;
        }
        opens.insert(
            k,
            match v {
                serde_json::Value::Number(n) => n.as_u64().map(|x| x as u32),
                serde_json::Value::Null => None,
                _ => None,
            },
        );
    }
    (opens, closes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_hrefs_js_balanced() {
        assert_eq!(
            DECLARED_HREFS_JS.matches('(').count(),
            DECLARED_HREFS_JS.matches(')').count()
        );
    }

    #[test]
    fn brace_counts_template_substitutes() {
        let urls = vec!["https://a/x.css".to_owned(), "https://b/y.css".to_owned()];
        let js = brace_counts_js(&urls);
        assert!(!js.contains("URLS_JSON"), "template not substituted");
        assert!(js.contains("https://a/x.css"));
        assert!(js.contains("https://b/y.css"));
    }

    #[test]
    fn applied_rule_count_balanced() {
        assert_eq!(
            APPLIED_RULE_COUNT_JS.matches('(').count(),
            APPLIED_RULE_COUNT_JS.matches(')').count()
        );
    }

    #[test]
    fn computed_styles_js_returns_required_keys() {
        for k in [
            "backgroundColor",
            "color",
            "fontFamily",
            "fontSize",
            "margin",
        ] {
            assert!(COMPUTED_STYLES_JS.contains(k), "missing key: {k}");
        }
    }

    #[test]
    fn split_close_braces_basic() {
        let mut raw: BraceCountsRaw = HashMap::new();
        raw.insert("/x.css".to_owned(), serde_json::json!(42));
        raw.insert("/y.css".to_owned(), serde_json::Value::Null);
        raw.insert("_close".to_owned(), serde_json::json!({ "/x.css": 41 }));
        let (opens, closes) = split_close_braces(raw);
        assert_eq!(opens.get("/x.css").copied(), Some(Some(42)));
        assert_eq!(opens.get("/y.css").copied(), Some(None));
        assert_eq!(closes.get("/x.css").copied(), Some(41));
    }

    #[test]
    fn snapshot_round_trips() {
        let snap = CssHealthSnapshot {
            page_url: "http://x/".to_owned(),
            declared_sheets: vec![],
            inline_style_block_count: 0,
            computed_body: ComputedBody {
                background_color: "rgb(255,255,255)".to_owned(),
                color: "rgb(0,0,0)".to_owned(),
                font_family: "system-ui".to_owned(),
                font_size: "16px".to_owned(),
                margin: "0px".to_owned(),
            },
            computed_html: ComputedHtml {
                background_color: "rgb(255,255,255)".to_owned(),
            },
            body_visible_text_length: 100,
            applied_rule_count_estimate: 200,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: CssHealthSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.body_visible_text_length, 100);
    }
}
