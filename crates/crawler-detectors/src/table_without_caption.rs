//! `table_without_caption` — flags data `<table>` elements
//! without `<caption>` or other accessible name.
//!
//! WCAG 2.1 SC 1.3.1 (Info and Relationships, Level A): data
//! tables must have programmatically-determinable structure.
//! `<caption>` is the canonical mechanism for naming a table;
//! `aria-label` / `aria-labelledby` are valid alternatives.
//! Without any of these, screen-reader users land in a
//! `"table with N rows and M columns"` announcement with no
//! idea what the table is about.
//!
//! Defect class: operator wrote `<table>` for tabular data
//! (financial figures, comparison matrices, schedules) and
//! never named it. Real impact: BAT (Blind Adult Tester)
//! studies consistently find unnamed tables in the top-3 a11y
//! complaints on financial / education / e-commerce sites.
//!
//! Two defect classes:
//!
//! 1. **Bare `<table>` without caption or label** (Strict).
//!    No `<caption>`, no `aria-label`, no `aria-labelledby`,
//!    no `role="presentation"`. Screen readers announce the
//!    table without identifying it.
//!
//! 2. **Layout `<table>` without `role="presentation"`**
//!    (Warn). Layout tables (rarer in 2026 but still
//!    present in email templates + legacy CMS exports) need
//!    `role="presentation"` to opt out of the data-table
//!    accessibility tree. Otherwise screen readers
//!    incorrectly announce structure.
//!
//!    Heuristic for "looks like layout table": 1 row,
//!    1-3 cells, no `<th>` headers, no `<thead>`. Imperfect
//!    but catches the common case.
//!
//! Honors `data-table-allow="true"` opt-out for measured
//! exceptions (e.g. a presentation-table the operator has
//! audited but doesn't want to rename).
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<table>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TableWithoutCaptionHit {
    /// CSS-ish path of the offending `<table>`.
    pub selector: String,
    /// Row count (counts `<tr>` direct descendants).
    pub row_count: u32,
    /// Column count (best-effort from first row cell count).
    pub column_count: u32,
    /// True iff a `<caption>` child is present.
    pub has_caption: bool,
    /// True iff `aria-label` non-empty.
    pub has_aria_label: bool,
    /// True iff `aria-labelledby` references a non-empty target.
    pub has_aria_labelledby: bool,
    /// True iff `<th>` header cells appear.
    pub has_th_headers: bool,
    /// True iff `role="presentation"` / `role="none"` set.
    pub has_presentation_role: bool,
    /// Defect kind — one of `"no-caption-or-label"`,
    /// `"layout-table-without-role"`.
    pub defect_kind: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TableWithoutCaptionSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending tables.
    pub hits: Vec<TableWithoutCaptionHit>,
    /// Total `<table>` elements walked.
    pub scanned_tables: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_table_without_caption(snap: &TableWithoutCaptionSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut no_caption: Vec<&TableWithoutCaptionHit> = Vec::new();
    let mut layout_no_role: Vec<&TableWithoutCaptionHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "no-caption-or-label" => no_caption.push(h),
            "layout-table-without-role" => layout_no_role.push(h),
            _ => {} // defensive: unknown kinds dropped
        }
    }

    let format_example = |h: &TableWithoutCaptionHit| -> String {
        format!(
            "{} ({} rows × {} cols)",
            h.selector, h.row_count, h.column_count
        )
    };

    let mut out = Vec::new();
    if !no_caption.is_empty() {
        let examples: Vec<String> = no_caption
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "table-caption.missing".to_owned(),
            detail: format!(
                "{} <table> element(s) without `<caption>`, `aria-label`, or `aria-labelledby` — screen readers announce them as `\"table with N rows and M columns\"` without identifying the subject. Add `<caption>Q3 financials</caption>` as the first child, or `aria-label=\"…\"`. Examples: {}",
                no_caption.len(),
                examples.join("; ")
            ),
        });
    }
    if !layout_no_role.is_empty() {
        let examples: Vec<String> = layout_no_role
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "table-caption.layout-no-role".to_owned(),
            detail: format!(
                "{} `<table>` element(s) appear to be layout tables (no `<th>`, ≤ 3 cells, no header rows) but lack `role=\"presentation\"`. Screen readers announce structure for these as if they were data tables. Either add `role=\"presentation\"` to opt out, or migrate to CSS grid. Examples: {}",
                layout_no_role.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks every `<table>`
/// (skipping `data-table-allow="true"` opt-out), classifies
/// each into one of the two defect buckets when applicable,
/// and emits a hit per offender.
pub const TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS: &str = r#"
(() => {
    const selectorOf = function(el) {
      if (!el || el === document.documentElement) return 'html';
      if (el.id) return '#' + el.id;
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

    // Resolves whether aria-labelledby points at a non-empty
    // target. Single id only — multi-token (per WAI-ARIA) is
    // valid but uncommon; flag conservatively when ALL tokens
    // resolve to empty.
    const ariaLabelledbyResolves = function(t) {
      const v = t.getAttribute && t.getAttribute('aria-labelledby');
      if (!v) return false;
      const ids = v.split(/\s+/).filter(function(s) { return s.length > 0; });
      for (const id of ids) {
        const ref = document.getElementById(id);
        if (ref && (ref.textContent || '').trim()) return true;
      }
      return false;
    };

    const hits = [];
    let scanned = 0;
    const tables = document.querySelectorAll('table');
    for (const t of tables) {
      if (t.getAttribute && t.getAttribute('data-table-allow') === 'true') continue;
      scanned += 1;

      const role = (t.getAttribute('role') || '').trim().toLowerCase();
      const hasPresentationRole = role === 'presentation' || role === 'none';
      const hasCaption = t.querySelector(':scope > caption') != null;
      const aria = (t.getAttribute('aria-label') || '').trim();
      const hasAriaLabel = aria.length > 0;
      const hasAriaLabelledby = ariaLabelledbyResolves(t);
      const ths = t.querySelectorAll('th');
      const hasThHeaders = ths.length > 0;
      const rows = t.querySelectorAll(':scope > tbody > tr, :scope > thead > tr, :scope > tr');
      const rowCount = rows.length;
      const firstRow = rows[0];
      const colCount = firstRow ? firstRow.querySelectorAll('td, th').length : 0;

      // Classification:
      // 1. presentation-role tables: out of scope entirely.
      // 2. tables with caption/aria-name: pass.
      // 3. tables that look like layout (≤3 cells, no <th>):
      //    flag as layout-table-without-role.
      // 4. everything else: flag as no-caption-or-label.
      if (hasPresentationRole) continue;
      if (hasCaption || hasAriaLabel || hasAriaLabelledby) continue;

      const looksLayout = !hasThHeaders && colCount > 0 && colCount <= 3 && rowCount <= 1;
      const defectKind = looksLayout
        ? 'layout-table-without-role'
        : 'no-caption-or-label';

      hits.push({
        selector: selectorOf(t),
        rowCount: rowCount,
        columnCount: colCount,
        hasCaption: hasCaption,
        hasAriaLabel: hasAriaLabel,
        hasAriaLabelledby: hasAriaLabelledby,
        hasThHeaders: hasThHeaders,
        hasPresentationRole: hasPresentationRole,
        defectKind: defectKind
      });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      hits: hits,
      scannedTables: scanned
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(
        selector: &str,
        rows: u32,
        cols: u32,
        has_th: bool,
        defect_kind: &str,
    ) -> TableWithoutCaptionHit {
        TableWithoutCaptionHit {
            selector: selector.into(),
            row_count: rows,
            column_count: cols,
            has_caption: false,
            has_aria_label: false,
            has_aria_labelledby: false,
            has_th_headers: has_th,
            has_presentation_role: false,
            defect_kind: defect_kind.into(),
        }
    }

    fn snap(hits: Vec<TableWithoutCaptionHit>) -> TableWithoutCaptionSnapshot {
        TableWithoutCaptionSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_tables: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_table_without_caption(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_caption_data_table_is_strict() {
        let s = snap(vec![hit(".finance", 10, 4, true, "no-caption-or-label")]);
        let findings = detect_table_without_caption(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "table-caption.missing");
        assert!(findings[0].detail.contains(".finance"));
        assert!(findings[0].detail.contains("10 rows × 4 cols"));
    }

    #[test]
    fn layout_table_without_role_is_warn() {
        let s = snap(vec![hit(
            ".layout",
            1,
            2,
            false,
            "layout-table-without-role",
        )]);
        let findings = detect_table_without_caption(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "table-caption.layout-no-role");
        assert!(findings[0].detail.contains("role=\"presentation\""));
    }

    #[test]
    fn mixed_emits_two_findings() {
        let s = snap(vec![
            hit(".data", 10, 4, true, "no-caption-or-label"),
            hit(".layout", 1, 2, false, "layout-table-without-role"),
        ]);
        let findings = detect_table_without_caption(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"table-caption.missing"));
        assert!(kinds.contains(&"table-caption.layout-no-role"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(".x", 5, 3, true, "future-defect")]);
        let findings = detect_table_without_caption(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".t-{i}"),
                5,
                3,
                true,
                "no-caption-or-label",
            ));
        }
        let s = snap(hits);
        let findings = detect_table_without_caption(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 <table> element(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("hits"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("scannedTables"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("rowCount"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("columnCount"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("hasCaption"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("hasAriaLabel"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("hasAriaLabelledby"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("defectKind"));
        // Selector contract.
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("'table'"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains(":scope > caption"));
        // Role contracts.
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("'presentation'"));
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("'none'"));
        // Opt-out contract.
        assert!(TABLE_WITHOUT_CAPTION_DOM_CAPTURE_JS.contains("data-table-allow"));
    }
}
