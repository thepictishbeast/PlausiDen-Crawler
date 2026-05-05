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
#[serde(rename_all = "camelCase")]
pub struct ComputedHtml {
    /// background-color shorthand.
    pub background_color: String,
}

/// Combined snapshot. Assembled by the Rust runner after each of
/// the 6 evals returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// Matches the TS USER_AGENT_DEFAULTS sets in cssHealth.ts:95.
// "Browser-default" detection: if computedBody.backgroundColor /
// fontFamily / margin all live in these sets AND html bg too,
// the visitor sees no applied CSS.
const UA_BG: &[&str] = &[
    "rgba(0, 0, 0, 0)",
    "rgb(255, 255, 255)",
    "transparent",
    "initial",
];
const UA_FONT: &[&str] = &[
    "Times",
    "\"Times New Roman\"",
    "Times New Roman",
    "serif",
    "\"Times New Roman\", Times, serif",
];
const UA_MARGIN: &[&str] = &["8px", "0px 8px"];

fn looks_like_ua_default_body(body: &ComputedBody) -> bool {
    UA_BG.contains(&body.background_color.as_str())
        && UA_FONT.contains(&body.font_family.as_str())
        && UA_MARGIN.contains(&body.margin.as_str())
}

fn looks_like_ua_default_html(html: &ComputedHtml) -> bool {
    UA_BG.contains(&html.background_color.as_str())
}

fn sheet_failed_network(s: &StylesheetObservation) -> bool {
    s.error_text.is_some() || (s.from_network && (s.status == 0 || s.status >= 400))
}

fn sheet_loaded_ok(s: &StylesheetObservation) -> bool {
    s.from_network && s.status >= 200 && s.status < 400 && s.body_bytes >= 50
}

/// Apply detection rules to a CSS-health snapshot. Pure function.
/// Mirrors the TS `detectCSSHealthIssues` exactly (cssHealth.ts:253).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn detect_css_health_issues(snap: &CssHealthSnapshot) -> Vec<crate::AxisFinding> {
    let mut out = Vec::<crate::AxisFinding>::new();

    // Heuristic 5: zero stylesheets at all.
    if snap.declared_sheets.is_empty() && snap.inline_style_block_count == 0 {
        out.push(crate::AxisFinding {
            severity: crate::AxisSeverity::Warn,
            kind: "css.no-stylesheets-declared".to_owned(),
            detail: "Page has zero <link rel=\"stylesheet\"> tags AND zero <style> blocks. This is unusual; a real visitor will see browser-default styling.".to_owned(),
        });
        // No sheets means later heuristics don't apply.
        return out;
    }

    // Heuristic 1: declared-sheet network failures.
    if !snap.declared_sheets.is_empty() {
        let failed_count = snap
            .declared_sheets
            .iter()
            .filter(|s| sheet_failed_network(s))
            .count();
        let total = snap.declared_sheets.len();
        if failed_count == total {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Strict,
                kind: "css.all-sheets-failed-network".to_owned(),
                detail: format!(
                    "All {total} declared stylesheet(s) failed to load. The page renders with no CSS."
                ),
            });
        } else if failed_count > 0 {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Strict,
                kind: "css.some-sheets-failed-network".to_owned(),
                detail: format!("{failed_count} of {total} declared stylesheet(s) failed to load."),
            });
        }

        // Heuristic 2: wrong MIME type.
        for s in &snap.declared_sheets {
            if s.from_network && (200..400).contains(&s.status) {
                if let Some(ct) = &s.content_type {
                    let lower = ct.to_ascii_lowercase();
                    if !lower.is_empty() && !lower.starts_with("text/css") {
                        out.push(crate::AxisFinding {
                            severity: crate::AxisSeverity::Strict,
                            kind: "css.wrong-mime".to_owned(),
                            detail: format!(
                                "Stylesheet {} served with Content-Type \"{ct}\" — browsers refuse to apply non-text/css responses as CSS.",
                                s.url
                            ),
                        });
                    }
                }
            }
        }

        // Heuristic 3: stylesheet body suspiciously empty.
        for s in &snap.declared_sheets {
            if s.from_network && (200..400).contains(&s.status) && s.body_bytes < 50 {
                out.push(crate::AxisFinding {
                    severity: crate::AxisSeverity::Strict,
                    kind: "css.empty-or-tiny-body".to_owned(),
                    detail: format!(
                        "Stylesheet {} returned {} byte(s) — almost certainly empty or truncated.",
                        s.url, s.body_bytes
                    ),
                });
            }
        }
    }

    // Heuristics 4, A, B, brace-imbalance only fire if at least one
    // declared sheet loaded fine.
    let any_usable = snap.declared_sheets.iter().any(sheet_loaded_ok);
    if any_usable {
        // Heuristic 4: served-but-not-applied (UA defaults despite a
        // usable sheet).
        if looks_like_ua_default_body(&snap.computed_body)
            && looks_like_ua_default_html(&snap.computed_html)
        {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Strict,
                kind: "css.served-but-not-applied".to_owned(),
                detail: "CSS file(s) loaded successfully but the body still has user-agent default styling (white background, serif font, 8px margin). Likely a CSS parse error early in the file caused the browser to silently drop the rest.".to_owned(),
            });
        }

        // Rule-density heuristic A: bytes-per-rule.
        let total_sheet_bytes: u64 = snap
            .declared_sheets
            .iter()
            .filter(|s| s.from_network && (200..400).contains(&s.status))
            .map(|s| s.body_bytes)
            .sum();
        if total_sheet_bytes >= 100 {
            let bytes_per_rule = if snap.applied_rule_count_estimate > 0 {
                total_sheet_bytes as f64 / f64::from(snap.applied_rule_count_estimate)
            } else {
                f64::INFINITY
            };
            if bytes_per_rule > 500.0 {
                out.push(crate::AxisFinding {
                    severity: crate::AxisSeverity::Strict,
                    kind: "css.applied-rule-count-anomaly".to_owned(),
                    detail: format!(
                        "Browser reports only {} CSS rule(s) applied across {} byte(s) of stylesheet (~{} bytes/rule, healthy is < 200). Parse error likely dropped most rules.",
                        snap.applied_rule_count_estimate,
                        total_sheet_bytes,
                        bytes_per_rule.round() as u64
                    ),
                });
            }
        }

        // Rule-density heuristic B: declared opening braces vs applied
        // rules. Ratio < 0.7 with >= 5 declared braces indicates a
        // serious parser rejection.
        let total_declared_braces: u32 = snap
            .declared_sheets
            .iter()
            .map(|s| s.declared_brace_count.unwrap_or(0))
            .sum();
        let apply_ratio = if total_declared_braces > 0 {
            f64::from(snap.applied_rule_count_estimate) / f64::from(total_declared_braces)
        } else {
            1.0
        };
        if total_declared_braces >= 5 && apply_ratio < 0.7 {
            out.push(crate::AxisFinding {
                severity: crate::AxisSeverity::Strict,
                kind: "css.brace-vs-applied-mismatch".to_owned(),
                detail: format!(
                    "Stylesheet declares {} opening brace(s) but the browser applied only {} rule(s) ({}% applied; healthy is > 70%). Likely an unmatched brace, bad selector, or invalid at-rule early in the sheet.",
                    total_declared_braces,
                    snap.applied_rule_count_estimate,
                    (apply_ratio * 100.0).round() as u32
                ),
            });
        }

        // Brace-imbalance heuristic: open vs close should be equal.
        for s in &snap.declared_sheets {
            if let (Some(open), Some(close)) =
                (s.declared_brace_count, s.declared_close_brace_count)
            {
                let delta = i64::from(close) - i64::from(open);
                if delta.abs() > 1 {
                    out.push(crate::AxisFinding {
                        severity: crate::AxisSeverity::Strict,
                        kind: "css.brace-imbalance".to_owned(),
                        detail: format!(
                            "Stylesheet {} has {open} open brace(s) and {close} close brace(s) — a delta of {delta}. CSS is malformed; parser will mis-anchor and drop subsequent rules.",
                            s.url
                        ),
                    });
                }
            }
        }
    }

    out
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

    fn ua_default_body() -> ComputedBody {
        ComputedBody {
            background_color: "rgba(0, 0, 0, 0)".to_owned(),
            color: "rgb(0, 0, 0)".to_owned(),
            font_family: "Times".to_owned(),
            font_size: "16px".to_owned(),
            margin: "8px".to_owned(),
        }
    }

    fn styled_body() -> ComputedBody {
        ComputedBody {
            background_color: "rgb(15, 23, 42)".to_owned(),
            color: "rgb(248, 250, 252)".to_owned(),
            font_family: "\"Inter\", system-ui, sans-serif".to_owned(),
            font_size: "16px".to_owned(),
            margin: "0px".to_owned(),
        }
    }

    #[allow(clippy::too_many_arguments)] // intentional 1:1 mirror of StylesheetObservation fields
    fn sheet(
        url: &str,
        status: u16,
        ct: Option<&str>,
        bytes: u64,
        open: Option<u32>,
        close: Option<u32>,
        from_net: bool,
        err: Option<&str>,
    ) -> StylesheetObservation {
        StylesheetObservation {
            url: url.to_owned(),
            status,
            content_type: ct.map(ToOwned::to_owned),
            body_bytes: bytes,
            declared_brace_count: open,
            declared_close_brace_count: close,
            from_network: from_net,
            error_text: err.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn working_css_produces_no_findings() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/working".to_owned(),
            declared_sheets: vec![sheet(
                "http://t/style.css",
                200,
                Some("text/css"),
                5000,
                Some(80),
                Some(80),
                true,
                None,
            )],
            inline_style_block_count: 0,
            computed_body: styled_body(),
            computed_html: ComputedHtml {
                background_color: "rgb(15, 23, 42)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 80,
        };
        assert!(
            detect_css_health_issues(&snap).is_empty(),
            "expected zero findings"
        );
    }

    #[test]
    fn missing_css_triggers_all_failed() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/missing".to_owned(),
            declared_sheets: vec![sheet(
                "http://t/missing.css",
                404,
                Some("text/html"),
                0,
                None,
                None,
                true,
                None,
            )],
            inline_style_block_count: 0,
            computed_body: ua_default_body(),
            computed_html: ComputedHtml {
                background_color: "rgba(0, 0, 0, 0)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 0,
        };
        let findings = detect_css_health_issues(&snap);
        assert!(findings
            .iter()
            .any(|f| f.kind == "css.all-sheets-failed-network"));
    }

    #[test]
    fn empty_css_triggers_empty_or_tiny() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/empty".to_owned(),
            declared_sheets: vec![sheet(
                "http://t/empty.css",
                200,
                Some("text/css"),
                0,
                Some(0),
                Some(0),
                true,
                None,
            )],
            inline_style_block_count: 0,
            computed_body: ua_default_body(),
            computed_html: ComputedHtml {
                background_color: "rgba(0, 0, 0, 0)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 0,
        };
        let findings = detect_css_health_issues(&snap);
        assert!(findings.iter().any(|f| f.kind == "css.empty-or-tiny-body"));
    }

    #[test]
    fn wrong_mime_triggers_finding() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/wrongmime".to_owned(),
            declared_sheets: vec![sheet(
                "http://t/style.css",
                200,
                Some("text/html; charset=utf-8"),
                5000,
                Some(0),
                Some(0),
                true,
                None,
            )],
            inline_style_block_count: 0,
            computed_body: ua_default_body(),
            computed_html: ComputedHtml {
                background_color: "rgba(0, 0, 0, 0)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 0,
        };
        let findings = detect_css_health_issues(&snap);
        assert!(findings.iter().any(|f| f.kind == "css.wrong-mime"));
    }

    #[test]
    fn parsefail_triggers_served_or_anomaly() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/parsefail".to_owned(),
            declared_sheets: vec![sheet(
                "http://t/parsefail.css",
                200,
                Some("text/css"),
                5000,
                Some(1),
                Some(5),
                true,
                None,
            )],
            inline_style_block_count: 0,
            computed_body: ua_default_body(),
            computed_html: ComputedHtml {
                background_color: "rgba(0, 0, 0, 0)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 1,
        };
        let findings = detect_css_health_issues(&snap);
        assert!(
            findings
                .iter()
                .any(|f| f.kind == "css.served-but-not-applied"
                    || f.kind == "css.applied-rule-count-anomaly"
                    || f.kind == "css.brace-imbalance"),
            "got: {:?}",
            findings.iter().map(|f| &f.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_stylesheets_warn_only() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/no-css".to_owned(),
            declared_sheets: vec![],
            inline_style_block_count: 0,
            computed_body: ua_default_body(),
            computed_html: ComputedHtml {
                background_color: "rgba(0, 0, 0, 0)".to_owned(),
            },
            body_visible_text_length: 50,
            applied_rule_count_estimate: 0,
        };
        let findings = detect_css_health_issues(&snap);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "css.no-stylesheets-declared");
        assert_eq!(findings[0].severity, crate::AxisSeverity::Warn);
    }

    #[test]
    fn partial_failure_triggers_some_failed() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/partial".to_owned(),
            declared_sheets: vec![
                sheet(
                    "http://t/ok.css",
                    200,
                    Some("text/css"),
                    5000,
                    Some(80),
                    Some(80),
                    true,
                    None,
                ),
                sheet(
                    "http://t/404.css",
                    404,
                    Some("text/html"),
                    0,
                    None,
                    None,
                    true,
                    None,
                ),
            ],
            inline_style_block_count: 0,
            computed_body: styled_body(),
            computed_html: ComputedHtml {
                background_color: "rgb(15, 23, 42)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 80,
        };
        let findings = detect_css_health_issues(&snap);
        assert!(findings
            .iter()
            .any(|f| f.kind == "css.some-sheets-failed-network"));
    }

    #[test]
    fn brace_imbalance_triggers_finding() {
        let snap = CssHealthSnapshot {
            page_url: "http://t/imbalance".to_owned(),
            declared_sheets: vec![sheet(
                "http://t/imbalance.css",
                200,
                Some("text/css"),
                5000,
                Some(80),
                Some(85),
                true,
                None,
            )],
            inline_style_block_count: 0,
            computed_body: styled_body(),
            computed_html: ComputedHtml {
                background_color: "rgb(15, 23, 42)".to_owned(),
            },
            body_visible_text_length: 200,
            applied_rule_count_estimate: 80,
        };
        let findings = detect_css_health_issues(&snap);
        assert!(findings.iter().any(|f| f.kind == "css.brace-imbalance"));
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
