//! `third_party_script_count` — flags excessive third-party-
//! origin script spread.
//!
//! Each unique third-party origin serving `<script>` adds:
//!
//! 1. **DNS lookup** + **TCP handshake** + **TLS handshake** —
//!    typically 100-300ms before the first byte arrives.
//! 2. **HTTP/2 connection** that can't share the page-origin
//!    connection.
//! 3. **CSP attack surface** + **supply-chain risk** — each
//!    new origin can serve any JS, including malicious.
//! 4. **Ad-blocker / privacy-extension breakage** when those
//!    origins are blocked.
//!
//! Defect class: a marketing site accumulates Segment +
//! Google Tag Manager + HubSpot + Intercom + Heap + Hotjar +
//! Drift + Mixpanel + LinkedIn Insight + Facebook Pixel +
//! TikTok Pixel = 11 third-party origins. Each kills a chunk
//! of LCP budget. Real-world: 60% of Alexa-100 sites ship > 5
//! third-party origins (HTTP Archive).
//!
//! Distinct from [`crate::render_blocking_resources`] which
//! flags in-head scripts WITHOUT async/defer — this axis
//! counts unique third-party origins regardless of where they
//! are or how they're loaded.
//!
//! ## Heuristic
//!
//! The JS collects every `<script src="…">` URL, extracts the
//! origin (scheme + host), excludes the page origin. Same-
//! origin and protocol-relative are both treated as
//! same-origin.
//!
//! ## Severity
//!
//! * **Strict** — `total_third_party_origins > 10`. Extreme
//!   third-party spread; first-paint becomes a function of
//!   network conditions, not page content.
//! * **Warn** — `total_third_party_origins > 5`. Common
//!   marketing-stack pattern; document the trade-off + audit
//!   what each origin contributes.
//!
//! Per-origin counts surface in the example list so operators
//! can see which origins are doing the heavy lifting.
//!
//! AVP-2 invariants: `unsafe_code = "deny"`, pure detector,
//! no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// One captured third-party origin + script count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ThirdPartyScriptHit {
    /// The third-party origin (`"https://example.com"`).
    pub origin: String,
    /// Number of `<script>` elements served from this origin.
    pub script_count: u32,
}

/// Captured page state the detector consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ThirdPartyScriptCountSnapshot {
    /// Top-level page URL.
    pub page_url: String,
    /// Viewport width at capture time (CSS px).
    pub viewport_width: u32,
    /// The host page's origin — third-party means "not this
    /// origin." Captured as a string for transparency in
    /// findings.
    pub page_origin: String,
    /// Third-party origins serving scripts, with per-origin
    /// script count.
    pub hits: Vec<ThirdPartyScriptHit>,
    /// Total unique third-party origins. Convenience field
    /// (== hits.len() in practice).
    pub total_third_party_origins: u32,
}

/// Origin count threshold above which the finding is Strict.
pub const STRICT_ORIGIN_COUNT: u32 = 10;

/// Origin count threshold above which the finding is Warn.
pub const WARN_ORIGIN_COUNT: u32 = 5;

/// Max examples reported per finding.
pub const MAX_EXAMPLES: usize = 5;

/// Pure detector: snapshot → findings.
#[must_use]
pub fn detect_third_party_script_count(snap: &ThirdPartyScriptCountSnapshot) -> Vec<AxisFinding> {
    let n = snap.total_third_party_origins;
    if n <= WARN_ORIGIN_COUNT {
        return Vec::new();
    }
    let severity = if n > STRICT_ORIGIN_COUNT {
        AxisSeverity::Strict
    } else {
        AxisSeverity::Warn
    };
    let kind = if n > STRICT_ORIGIN_COUNT {
        "third-party-script-count.extreme"
    } else {
        "third-party-script-count.high"
    };
    // Sort hits by descending script_count then alphabetical
    // origin for stable, signal-first output.
    let mut sorted: Vec<&ThirdPartyScriptHit> = snap.hits.iter().collect();
    sorted.sort_by(|a, b| {
        b.script_count
            .cmp(&a.script_count)
            .then_with(|| a.origin.cmp(&b.origin))
    });
    let examples: Vec<String> = sorted
        .iter()
        .take(MAX_EXAMPLES)
        .map(|h| format!("{} ({}×)", h.origin, h.script_count))
        .collect();
    let detail_intro = if severity == AxisSeverity::Strict {
        format!(
            "{} third-party origins serving <script> — extreme spread. Each origin costs DNS + TCP + TLS (~100-300ms) before any byte arrives, adds CSP/supply-chain surface, and breaks under ad-blockers. Consolidate: self-host critical analytics, drop redundant pixels, defer non-essential.",
            n
        )
    } else {
        format!(
            "{} third-party origins serving <script> — common marketing-stack accretion. Each origin costs DNS + TCP + TLS before any byte arrives. Audit which contribute observable value and drop the rest.",
            n
        )
    };
    vec![AxisFinding {
        severity,
        kind: kind.to_owned(),
        detail: format!(
            "{} Top origins by script count: {}. Page origin (excluded from count): {}.",
            detail_intro,
            examples.join("; "),
            snap.page_origin
        ),
    }]
}

/// Browser-side DOM-capture script. Walks `script[src]`,
/// extracts the origin via URL parsing, excludes same-origin
/// + protocol-relative, aggregates per-origin script count.
pub const THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS: &str = r#"
(() => {
    const pageOrigin = window.location.origin;
    const counts = new Map();
    const scripts = document.querySelectorAll('script[src]');
    for (const s of scripts) {
      const src = (s.getAttribute('src') || '').trim();
      if (src === '') continue;
      // Protocol-relative or same-origin → skip.
      if (src.startsWith('//')) {
        // Protocol-relative: same-origin if host matches.
        // Resolve against pageOrigin's protocol.
        try {
          const url = new URL(window.location.protocol + src);
          if (url.origin === pageOrigin) continue;
          const o = url.origin;
          counts.set(o, (counts.get(o) || 0) + 1);
        } catch (_) { /* malformed — skip */ }
        continue;
      }
      // Absolute URL.
      if (/^https?:\/\//i.test(src)) {
        try {
          const url = new URL(src);
          if (url.origin === pageOrigin) continue;
          counts.set(url.origin, (counts.get(url.origin) || 0) + 1);
        } catch (_) { /* skip */ }
        continue;
      }
      // Path-relative — definitionally same-origin → skip.
    }

    const hits = [];
    for (const [origin, count] of counts.entries()) {
      hits.push({ origin: origin, scriptCount: count });
    }

    return {
      pageUrl: window.location.href,
      viewportWidth: window.innerWidth,
      pageOrigin: pageOrigin,
      hits: hits,
      totalThirdPartyOrigins: hits.length
    };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        page_origin: &str,
        hits_data: Vec<(&str, u32)>,
    ) -> ThirdPartyScriptCountSnapshot {
        let hits: Vec<ThirdPartyScriptHit> = hits_data
            .into_iter()
            .map(|(origin, count)| ThirdPartyScriptHit {
                origin: origin.into(),
                script_count: count,
            })
            .collect();
        let total = hits.len() as u32;
        ThirdPartyScriptCountSnapshot {
            page_url: "https://x".into(),
            viewport_width: 1280,
            page_origin: page_origin.into(),
            hits,
            total_third_party_origins: total,
        }
    }

    #[test]
    fn no_third_party_origins_returns_no_findings() {
        let s = snap("https://example.com", vec![]);
        let findings = detect_third_party_script_count(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn five_origins_below_warn_threshold_skipped() {
        let s = snap(
            "https://example.com",
            vec![
                ("https://gtm.example", 1),
                ("https://segment.io", 1),
                ("https://hubspot.com", 1),
                ("https://intercom.com", 1),
                ("https://heap.io", 1),
            ],
        );
        let findings = detect_third_party_script_count(&s);
        assert!(findings.is_empty());
    }

    #[test]
    fn six_origins_is_warn() {
        let s = snap(
            "https://example.com",
            vec![
                ("https://a.example", 1),
                ("https://b.example", 1),
                ("https://c.example", 1),
                ("https://d.example", 1),
                ("https://e.example", 1),
                ("https://f.example", 1),
            ],
        );
        let findings = detect_third_party_script_count(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Warn);
        assert_eq!(findings[0].kind, "third-party-script-count.high");
        assert!(findings[0].detail.contains("6 third-party origins"));
        assert!(findings[0].detail.contains("marketing-stack"));
    }

    #[test]
    fn eleven_origins_is_strict() {
        let mut hits = Vec::new();
        for i in 0..11 {
            hits.push((format!("https://o-{i}.example"), 1));
        }
        let s = snap(
            "https://example.com",
            hits.iter()
                .map(|(s, n)| (s.as_str(), *n))
                .collect(),
        );
        let findings = detect_third_party_script_count(&s);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, AxisSeverity::Strict);
        assert_eq!(findings[0].kind, "third-party-script-count.extreme");
        assert!(findings[0].detail.contains("11 third-party origins"));
        assert!(findings[0].detail.contains("extreme spread"));
    }

    #[test]
    fn examples_sorted_by_descending_count_then_alphabetical() {
        let s = snap(
            "https://example.com",
            vec![
                ("https://low.example", 1),
                ("https://high.example", 5),
                ("https://b-mid.example", 3),
                ("https://a-mid.example", 3),
                ("https://medium.example", 2),
                ("https://other.example", 1),
            ],
        );
        let findings = detect_third_party_script_count(&s);
        assert_eq!(findings.len(), 1);
        // Top origins by script count: high(5), a-mid(3), b-mid(3), medium(2), low(1)
        let detail = &findings[0].detail;
        let high = detail.find("high.example").expect("high");
        let a_mid = detail.find("a-mid.example").expect("a-mid");
        let b_mid = detail.find("b-mid.example").expect("b-mid");
        let medium = detail.find("medium.example").expect("medium");
        assert!(high < a_mid, "high(5) should come before a-mid(3)");
        assert!(a_mid < b_mid, "a-mid(3) before b-mid(3) by alpha");
        assert!(b_mid < medium, "b-mid(3) before medium(2)");
    }

    #[test]
    fn page_origin_appears_in_detail() {
        let mut hits = Vec::new();
        for i in 0..6 {
            hits.push((format!("https://o-{i}.example"), 1));
        }
        let s = snap(
            "https://prosperityclub.com",
            hits.iter()
                .map(|(s, n)| (s.as_str(), *n))
                .collect(),
        );
        let findings = detect_third_party_script_count(&s);
        assert!(findings[0]
            .detail
            .contains("Page origin (excluded from count): https://prosperityclub.com"));
    }

    #[test]
    fn dom_capture_js_is_iife_returning_object() {
        // Smoke: documented field shape + selector contract.
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("pageUrl"));
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("viewportWidth"));
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("pageOrigin"));
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("hits"));
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("totalThirdPartyOrigins"));
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("scriptCount"));
        // Selector contract.
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("'script[src]'"));
        // Protocol-relative handling.
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("startsWith('//')"));
        // Absolute URL detection.
        assert!(THIRD_PARTY_SCRIPT_COUNT_DOM_CAPTURE_JS.contains("/^https?:"));
    }
}
