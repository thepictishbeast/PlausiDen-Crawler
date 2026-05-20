//! `time_datetime` — `<time>` element machine-readability audit.
//!
//! Forward step on rolling Crawler axes (#117). Pairs with
//! `doc_title` / `heading_order` / `iso_8601` — the semantic-HTML
//! family.
//!
//! ## The bug class
//!
//! HTML5's `<time>` element exists to make dates / times machine-
//! readable. Three common operator failures:
//!
//! 1. **`<time>March 15, 2024</time>` without `datetime` attr.**
//!    Visible to humans; opaque to assistive tech, search engines,
//!    and SEO crawlers that look for structured dates. The whole
//!    point of the `<time>` element is to be parsable.
//! 2. **`<time datetime="March 15, 2024">`.** The `datetime`
//!    attribute MUST be one of the formats in the HTML living
//!    spec: ISO 8601 date / time / datetime / duration / etc.
//!    Free-form strings render but the parsing fails — same as
//!    not having the attribute at all, but more confusing
//!    because the developer THOUGHT they set it.
//! 3. **`<time datetime="2024-03-15"></time>`.** Datetime is
//!    machine-readable but the human-readable content is empty;
//!    the visible page has a blank where a date should be.
//!
//! ## Findings
//!
//! * `time-datetime.missing-attribute` warn — `<time>` with no
//!   `datetime` attr; visible text only. Warn-only because
//!   sometimes the text content IS valid ISO format (e.g.
//!   `<time>2024-03-15</time>` works per spec — the browser
//!   uses the text as the machine value).
//! * `time-datetime.invalid-attribute` strict — `datetime` attr
//!   is present but doesn't match any of the spec-allowed
//!   formats. Operator THOUGHT they did the right thing; in
//!   practice the attribute is dead.
//! * `time-datetime.empty-text` warn — `datetime` is valid but
//!   text content is empty. Visible page shows a blank where
//!   the date would render.
//!
//! Out of scope:
//!
//! * Full HTML-living-spec format validation (all 9 valid forms):
//!   we check the common shapes (ISO date, ISO datetime,
//!   year-month, time of day). Edge formats (week, year, duration)
//!   left to a follow-up detector if needed.
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"` (inherited).
//! * `#[non_exhaustive]` on snapshot + entry structs.
//! * Pure detector function; JS const is the only side-effect channel.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured `<time>` element.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TimeDatetimeEntry {
    /// CSS-ish selector pointing at the element.
    pub selector: String,
    /// Visible text content (whitespace-trimmed; capped at
    /// 100 chars for context).
    pub text: String,
    /// `datetime` attribute value if present.
    pub datetime: Option<String>,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TimeDatetimeSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Every `<time>` on the page.
    pub entries: Vec<TimeDatetimeEntry>,
}

/// Page-side eval. Walks every `<time>` element + captures text +
/// datetime attribute.
pub const TIME_DATETIME_JS: &str = r##"(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      const parts = [];
      let node = el;
      let depth = 0;
      while (node && node.nodeType === 1 && node !== document.body && depth < 6) {
        const tag = node.tagName.toLowerCase();
        const parent = node.parentElement;
        if (parent) {
          const sameTag = Array.from(parent.children).filter(function(c) { return c.tagName === node.tagName; });
          if (sameTag.length > 1) {
            const idx = sameTag.indexOf(node) + 1;
            parts.unshift(tag + ':nth-of-type(' + idx + ')');
          } else { parts.unshift(tag); }
        } else { parts.unshift(tag); }
        node = parent;
        depth += 1;
      }
      return 'body > ' + parts.join(' > ');
    };
    const truncate = function(s) { return (s || '').replace(/\s+/g, ' ').trim().slice(0, 100); };
    const entries = [];
    const times = document.querySelectorAll('time');
    for (let i = 0; i < times.length; i++) {
      const el = times[i];
      entries.push({
        selector: selectorOf(el),
        text: truncate(el.textContent || ''),
        datetime: el.getAttribute('datetime')
      });
    }
    return {
      pageUrl: location.href,
      entries: entries
    };
  })()"##;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_time_datetime(snap: &TimeDatetimeSnapshot) -> Vec<AxisFinding> {
    let mut findings = Vec::new();
    for entry in &snap.entries {
        let text_empty = entry.text.trim().is_empty();
        match entry.datetime.as_deref() {
            None => {
                // No datetime attr. Per HTML spec, the text content
                // itself can be the machine value IF it parses as
                // a valid datetime. We don't second-guess the text
                // here (could be valid); just warn that the explicit
                // attr is missing.
                if !text_empty {
                    findings.push(AxisFinding {
                        severity: AxisSeverity::Warn,
                        kind: "time-datetime.missing-attribute".to_owned(),
                        detail: format!(
                            "{} <time>{}</time> has no `datetime` attribute. If the visible text IS valid ISO-8601, the browser parses the text — but explicit `datetime=` makes the contract unambiguous. Add `datetime=\"...\"` matching the visible text.",
                            entry.selector, entry.text
                        ),
                    });
                }
                // Empty text + no datetime → just a degenerate
                // empty <time></time> element; flag.
                else {
                    findings.push(AxisFinding {
                        severity: AxisSeverity::Warn,
                        kind: "time-datetime.empty-element".to_owned(),
                        detail: format!(
                            "{} <time></time> is empty (no text + no datetime). The element is semantically meaningless; remove it or populate it.",
                            entry.selector
                        ),
                    });
                }
            }
            Some(dt) => {
                if !is_plausible_datetime(dt.trim()) {
                    findings.push(AxisFinding {
                        severity: AxisSeverity::Strict,
                        kind: "time-datetime.invalid-attribute".to_owned(),
                        detail: format!(
                            "{} <time datetime=\"{dt}\">{}</time> — `datetime` attribute doesn't match the HTML-living-spec shape (ISO 8601 date / datetime / year-month / time-of-day). Operator likely wrote a human-readable form; the attribute is dead. Use ISO 8601: 2024-03-15, 2024-03-15T14:30, 2024-03, 14:30, etc.",
                            entry.selector, entry.text
                        ),
                    });
                }
                if text_empty {
                    findings.push(AxisFinding {
                        severity: AxisSeverity::Warn,
                        kind: "time-datetime.empty-text".to_owned(),
                        detail: format!(
                            "{} <time datetime=\"{dt}\"></time> has machine-readable datetime but no visible text. The page shows a blank where the date would render. Add visible text content.",
                            entry.selector
                        ),
                    });
                }
            }
        }
    }
    findings
}

/// Loose plausibility check for HTML-spec datetime values.
/// Recognizes the four most-common shapes the spec allows:
///
/// * Date:           `2024-03-15`
/// * Datetime-local: `2024-03-15T14:30` (with seconds + fractions optional)
/// * Year-month:     `2024-03`
/// * Time:           `14:30` (with seconds + fractions optional)
///
/// Edge formats (week `2024-W12`, year `2024`, duration `P1Y2M`)
/// are accepted permissively — too rare to be worth full parsing.
#[must_use]
fn is_plausible_datetime(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Edge formats (week / year-only / duration) — accept permissively.
    if s.starts_with('P')
        || (s.contains("-W") && s.len() >= 7)
        || (s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()))
    {
        return true;
    }
    // ISO date / datetime / year-month / time-of-day shape check.
    // Pattern: digits + recognized separators, no free-form text.
    let permitted_chars =
        |c: char| -> bool { c.is_ascii_digit() || matches!(c, '-' | 'T' | ':' | '.' | '+' | 'Z') };
    if !s.chars().all(permitted_chars) {
        return false;
    }
    // Year present (4 digits at start) OR time-of-day (HH:MM).
    let bytes = s.as_bytes();
    let has_year = bytes.len() >= 4 && bytes[..4].iter().all(|b| b.is_ascii_digit());
    let has_time_only = bytes.len() >= 5
        && bytes[..2].iter().all(|b| b.is_ascii_digit())
        && bytes[2] == b':'
        && bytes[3..5].iter().all(|b| b.is_ascii_digit());
    has_year || has_time_only
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str, datetime: Option<&str>) -> TimeDatetimeEntry {
        TimeDatetimeEntry {
            selector: "body > time".to_owned(),
            text: text.to_owned(),
            datetime: datetime.map(str::to_owned),
        }
    }

    fn snap(entries: Vec<TimeDatetimeEntry>) -> TimeDatetimeSnapshot {
        TimeDatetimeSnapshot {
            page_url: "https://example.test/".to_owned(),
            entries,
        }
    }

    #[test]
    fn empty_snapshot_no_findings() {
        assert!(detect_time_datetime(&snap(Vec::new())).is_empty());
    }

    #[test]
    fn well_formed_time_with_datetime_is_silent() {
        let findings =
            detect_time_datetime(&snap(vec![entry("March 15, 2024", Some("2024-03-15"))]));
        assert!(findings.is_empty());
    }

    #[test]
    fn time_without_datetime_attr_is_warn() {
        let findings = detect_time_datetime(&snap(vec![entry("March 15, 2024", None)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "time-datetime.missing-attribute");
    }

    #[test]
    fn time_with_invalid_datetime_attr_is_strict() {
        // Operator wrote a human-readable date in the attribute.
        let findings = detect_time_datetime(&snap(vec![entry("Mar 15", Some("March 15, 2024"))]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "time-datetime.invalid-attribute");
    }

    #[test]
    fn time_with_valid_datetime_but_empty_text_is_warn() {
        let findings = detect_time_datetime(&snap(vec![entry("", Some("2024-03-15"))]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "time-datetime.empty-text");
    }

    #[test]
    fn empty_time_element_is_warn() {
        let findings = detect_time_datetime(&snap(vec![entry("", None)]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "time-datetime.empty-element");
    }

    #[test]
    fn plausible_datetime_recognizes_iso_date() {
        assert!(is_plausible_datetime("2024-03-15"));
        assert!(is_plausible_datetime("2024-03"));
        assert!(is_plausible_datetime("2024-03-15T14:30"));
        assert!(is_plausible_datetime("2024-03-15T14:30:00Z"));
        assert!(is_plausible_datetime("2024-03-15T14:30:00.500"));
        assert!(is_plausible_datetime("2024-03-15T14:30:00+02:00"));
    }

    #[test]
    fn plausible_datetime_recognizes_time_of_day() {
        assert!(is_plausible_datetime("14:30"));
        assert!(is_plausible_datetime("14:30:00"));
        assert!(is_plausible_datetime("14:30:00.500"));
    }

    #[test]
    fn plausible_datetime_recognizes_edge_formats() {
        assert!(is_plausible_datetime("2024-W12")); // week
        assert!(is_plausible_datetime("2024")); // year-only
        assert!(is_plausible_datetime("P1Y2M")); // duration
    }

    #[test]
    fn plausible_datetime_rejects_human_readable_strings() {
        assert!(!is_plausible_datetime("March 15, 2024"));
        assert!(!is_plausible_datetime("Mar 15"));
        assert!(!is_plausible_datetime("yesterday"));
        assert!(!is_plausible_datetime("two days ago"));
        assert!(!is_plausible_datetime(""));
    }

    #[test]
    fn multiple_time_elements_emit_one_finding_each() {
        let findings = detect_time_datetime(&snap(vec![
            entry("Good", Some("2024-03-15")), // silent
            entry("Bad", Some("invalid")),     // strict
            entry("No attr", None),            // warn
            entry("", Some("2024-03-15")),     // warn empty-text
        ]));
        assert_eq!(findings.len(), 3);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"time-datetime.invalid-attribute"));
        assert!(kinds.contains(&"time-datetime.missing-attribute"));
        assert!(kinds.contains(&"time-datetime.empty-text"));
    }

    #[test]
    fn snapshot_serde_camel_case() {
        let s = snap(vec![entry("2024-03-15", Some("2024-03-15"))]);
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"pageUrl\""));
        assert!(j.contains("\"datetime\""));
        let back: TimeDatetimeSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back.entries.len(), 1);
    }

    #[test]
    fn js_eval_const_walks_time_elements() {
        assert!(TIME_DATETIME_JS.contains("querySelectorAll('time')"));
        assert!(TIME_DATETIME_JS.contains("getAttribute('datetime')"));
        assert!(TIME_DATETIME_JS.contains("textContent"));
    }
}
