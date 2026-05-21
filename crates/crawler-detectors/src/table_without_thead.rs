//! `table_without_thead` — flags data `<table>` elements
//! whose header semantics are missing or broken.
//!
//! Complements [`crate::table_without_caption`] (which audits
//! table NAMING via caption/aria-label). This axis audits
//! HEADER STRUCTURE — without `<th>` cells, screen readers
//! announce column/row values without their identifying
//! header, making tabular data unintelligible.
//!
//! WCAG 2.1 SC 1.3.1 (Info and Relationships, Level A) +
//! WCAG 1.3.2 (Meaningful Sequence) both require that table
//! structure be programmatically determinable.
//!
//! Two defect classes:
//!
//! 1. **Data table with NO `<th>` cells anywhere** (Strict).
//!    Screen readers announce `"row 3, $1,200, March"`
//!    without identifying which column is amount and which
//!    is date. Operator wrote `<tr><td>Amount</td><td>Date
//!    </td></tr>` for the visual header row instead of
//!    `<tr><th>Amount</th><th>Date</th></tr>`.
//!
//! 2. **Data table with `<th>` but no `<thead>` wrapping**
//!    (Warn). Works for accessibility but the semantic
//!    `<thead>` wrapper makes parser intent clearer +
//!    enables print-stylesheet header-repeat-per-page.
//!
//! Heuristic for "data table" (vs layout table):
//! ≥ 2 rows AND ≥ 2 columns AND no `role="presentation"` /
//! `role="none"` on the `<table>`. Layout tables exit the
//! scope of this axis (covered by `table_without_caption`
//! warn tier).
//!
//! Honors `data-table-allow="true"` opt-out.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured offending `<table>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TableWithoutTheadHit {
    /// CSS-ish path of the offending `<table>`.
    pub selector: String,
    /// Row count.
    pub row_count: u32,
    /// Column count (best-effort from first row cell count).
    pub column_count: u32,
    /// True iff the `<table>` contains a `<thead>` element.
    pub has_thead_element: bool,
    /// True iff any `<th>` cell appears in the table.
    pub has_th_anywhere: bool,
    /// True iff the first row uses `<td>` instead of `<th>`
    /// (signal that operator intended a header row but used
    /// wrong tag).
    pub first_row_uses_td: bool,
    /// Defect kind — one of `"no-th-anywhere"`,
    /// `"th-but-no-thead-wrapper"`.
    pub defect_kind: String,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TableWithoutTheadSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// Offending data tables.
    pub hits: Vec<TableWithoutTheadHit>,
    /// Total `<table>` elements walked.
    pub scanned_tables: u32,
}

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_table_without_thead(snap: &TableWithoutTheadSnapshot) -> Vec<AxisFinding> {
    if snap.hits.is_empty() {
        return Vec::new();
    }
    let mut no_th: Vec<&TableWithoutTheadHit> = Vec::new();
    let mut no_wrapper: Vec<&TableWithoutTheadHit> = Vec::new();
    for h in &snap.hits {
        match h.defect_kind.as_str() {
            "no-th-anywhere" => no_th.push(h),
            "th-but-no-thead-wrapper" => no_wrapper.push(h),
            _ => {} // defensive: unknown kinds dropped
        }
    }

    let format_example = |h: &TableWithoutTheadHit| -> String {
        let signal = if h.first_row_uses_td {
            " · first row uses <td> (likely intended as header)"
        } else {
            ""
        };
        format!(
            "{} ({} rows × {} cols{})",
            h.selector, h.row_count, h.column_count, signal
        )
    };

    let mut out = Vec::new();
    if !no_th.is_empty() {
        let examples: Vec<String> = no_th
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Strict,
            kind: "table-thead.no-th-anywhere".to_owned(),
            detail: format!(
                "{} data table(s) have no <th> cells anywhere — screen readers announce values without their column/row identifier. Wrap the header row's `<td>` cells in `<thead>` and convert them to `<th scope=\"col\">`. Examples: {}",
                no_th.len(),
                examples.join("; ")
            ),
        });
    }
    if !no_wrapper.is_empty() {
        let examples: Vec<String> = no_wrapper
            .iter()
            .take(MAX_EXAMPLES)
            .map(|h| format_example(h))
            .collect();
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "table-thead.no-thead-wrapper".to_owned(),
            detail: format!(
                "{} data table(s) have <th> cells but no <thead> wrapper. Works for a11y but loses parser-intent clarity + breaks print-stylesheet header-repeat-per-page. Wrap header row(s) in `<thead>`. Examples: {}",
                no_wrapper.len(),
                examples.join("; ")
            ),
        });
    }
    out
}

/// Browser-side DOM-capture script. Walks data `<table>`
/// elements (≥ 2 rows × ≥ 2 cols, not `role="presentation"`)
/// and classifies into one of the two defect buckets.
pub const TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS: &str = r#"
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

    const hits = [];
    let scanned = 0;
    const tables = document.querySelectorAll('table');
    for (const t of tables) {
      if (t.getAttribute && t.getAttribute('data-table-allow') === 'true') continue;
      const role = (t.getAttribute('role') || '').trim().toLowerCase();
      if (role === 'presentation' || role === 'none') continue;
      scanned += 1;

      const rows = t.querySelectorAll(':scope > tbody > tr, :scope > thead > tr, :scope > tr');
      const rowCount = rows.length;
      const firstRow = rows[0];
      const colCount = firstRow ? firstRow.querySelectorAll('td, th').length : 0;

      // Layout-table heuristic — out of scope. table_without_
      // caption's warn tier covers this.
      if (rowCount < 2 || colCount < 2) continue;

      const theadEl = t.querySelector(':scope > thead');
      const hasTheadElement = theadEl != null;
      const ths = t.querySelectorAll('th');
      const hasThAnywhere = ths.length > 0;
      const firstRowTds = firstRow.querySelectorAll(':scope > td').length;
      const firstRowUsesTd = firstRowTds > 0 && firstRow.querySelectorAll(':scope > th').length === 0;

      let defectKind = null;
      if (!hasThAnywhere) {
        defectKind = 'no-th-anywhere';
      } else if (!hasTheadElement) {
        defectKind = 'th-but-no-thead-wrapper';
      }
      if (defectKind === null) continue;

      hits.push({
        selector: selectorOf(t),
        rowCount: rowCount,
        columnCount: colCount,
        hasTheadElement: hasTheadElement,
        hasThAnywhere: hasThAnywhere,
        firstRowUsesTd: firstRowUsesTd,
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
        has_thead: bool,
        has_th: bool,
        first_row_td: bool,
        defect_kind: &str,
    ) -> TableWithoutTheadHit {
        TableWithoutTheadHit {
            selector: selector.into(),
            row_count: rows,
            column_count: cols,
            has_thead_element: has_thead,
            has_th_anywhere: has_th,
            first_row_uses_td: first_row_td,
            defect_kind: defect_kind.into(),
        }
    }

    fn snap(hits: Vec<TableWithoutTheadHit>) -> TableWithoutTheadSnapshot {
        TableWithoutTheadSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            hits,
            scanned_tables: 5,
        }
    }

    #[test]
    fn empty_snapshot_returns_no_findings() {
        let s = snap(vec![]);
        let findings = detect_table_without_thead(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_th_anywhere_is_strict() {
        let s = snap(vec![hit(".finance", 5, 4, false, false, true, "no-th-anywhere")]);
        let findings = detect_table_without_thead(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "table-thead.no-th-anywhere");
        assert!(findings[0].detail.contains("5 rows × 4 cols"));
        assert!(findings[0].detail.contains("first row uses <td>"));
    }

    #[test]
    fn no_thead_wrapper_is_warn() {
        let s = snap(vec![hit(
            ".schedule",
            6,
            3,
            false,
            true,
            false,
            "th-but-no-thead-wrapper",
        )]);
        let findings = detect_table_without_thead(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "table-thead.no-thead-wrapper");
        assert!(findings[0].detail.contains("print-stylesheet"));
    }

    #[test]
    fn mixed_emits_two_findings() {
        let s = snap(vec![
            hit(".a", 5, 4, false, false, true, "no-th-anywhere"),
            hit(".b", 6, 3, false, true, false, "th-but-no-thead-wrapper"),
        ]);
        let findings = detect_table_without_thead(&s);
        assert_eq!(findings.len(), 2);
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"table-thead.no-th-anywhere"));
        assert!(kinds.contains(&"table-thead.no-thead-wrapper"));
    }

    #[test]
    fn first_row_uses_td_signal_suppressed_when_false() {
        let s = snap(vec![hit(
            ".x",
            5,
            4,
            false,
            false,
            false,
            "no-th-anywhere",
        )]);
        let findings = detect_table_without_thead(&s);
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].detail.contains("first row uses <td>"));
    }

    #[test]
    fn unknown_defect_kind_ignored_defensively() {
        let s = snap(vec![hit(".x", 5, 4, false, false, false, "future-defect")]);
        let findings = detect_table_without_thead(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn examples_capped_at_five_per_finding() {
        let mut hits = Vec::new();
        for i in 0..10 {
            hits.push(hit(
                &format!(".t-{i}"),
                5,
                4,
                false,
                false,
                true,
                "no-th-anywhere",
            ));
        }
        let s = snap(hits);
        let findings = detect_table_without_thead(&s);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains("10 data table(s)"));
        let separators = findings[0].detail.matches("; ").count();
        assert_eq!(separators, 4, "5 examples → 4 \"; \" separators");
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("hits"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("scannedTables"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("rowCount"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("columnCount"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("hasTheadElement"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("hasThAnywhere"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("firstRowUsesTd"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("defectKind"));
        // Both defect kinds.
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("'no-th-anywhere'"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("'th-but-no-thead-wrapper'"));
        // Selector + role contracts.
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("'table'"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("'presentation'"));
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("'none'"));
        // Opt-out contract (same as table_without_caption).
        assert!(TABLE_WITHOUT_THEAD_DOM_CAPTURE_JS.contains("data-table-allow"));
    }
}
