//! `web_vitals` — Core Web Vitals capture.
//!
//! Different shape than other axes:
//!
//! 1. The `web-vitals` library (Google's IIFE) is loaded into
//!    every page via `addInitScript` BEFORE navigation. The
//!    library itself isn't embedded here — it's a third-party
//!    blob loaded from `node_modules/web-vitals/dist/web-vitals.iife.js`
//!    in the TS Crawler, and from the equivalent path in the
//!    Rust Crawler.
//! 2. `WIRE_CALLBACKS_JS` runs AFTER the library is loaded; it
//!    registers `onLCP/onCLS/onINP/onTTFB/onFCP` handlers that
//!    write to `window.__lfiVitals`.
//! 3. `COLLECT_JS` runs at journey end; reads `window.__lfiVitals`
//!    and returns the snapshot for serialization.
//!
//! Banding (good / needs-improvement / poor) is computed
//! Rust-side via [`band_lcp`] / [`band_cls`] / [`band_inp`].

use serde::{Deserialize, Serialize};

/// Wire `onLCP/onCLS/onINP/onTTFB/onFCP` to `window.__lfiVitals`.
/// Concatenate this AFTER the web-vitals IIFE source — the IIFE
/// installs the global `webVitals` object that this snippet uses.
pub const WIRE_CALLBACKS_JS: &str = r##"(function() {
      window.__lfiVitals = window.__lfiVitals || {};
      try {
        webVitals.onLCP(m => { window.__lfiVitals.lcp = m.value; });
        webVitals.onCLS(m => { window.__lfiVitals.cls = m.value; });
        webVitals.onINP(m => { window.__lfiVitals.inp = m.value; });
        webVitals.onTTFB(m => { window.__lfiVitals.ttfb = m.value; });
        webVitals.onFCP(m => { window.__lfiVitals.fcp = m.value; });
      } catch (e) { /* web-vitals API surface changed - non-fatal */ }
})()"##;

/// Read accumulated vitals from `window.__lfiVitals`. Run this at
/// journey end (INP / CLS finalize on page unload or interactions).
pub const COLLECT_JS: &str = r##"(window.__lfiVitals || {})"##;

/// WCAG / Core Web Vitals band per Google's published thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum Band {
    /// Within Google's "good" target.
    Good,
    /// Between "good" and "poor".
    NeedsImprovement,
    /// Above "poor" threshold.
    Poor,
}

/// LCP band: < 2500 good, 2500–4000 needs-improvement, > 4000 poor.
#[must_use]
pub fn band_lcp(value: f64) -> Band {
    if value < 2500.0 {
        Band::Good
    } else if value < 4000.0 {
        Band::NeedsImprovement
    } else {
        Band::Poor
    }
}

/// CLS band: < 0.1 good, 0.1–0.25 needs-improvement, > 0.25 poor.
#[must_use]
pub fn band_cls(value: f64) -> Band {
    if value < 0.1 {
        Band::Good
    } else if value < 0.25 {
        Band::NeedsImprovement
    } else {
        Band::Poor
    }
}

/// INP band (ms): < 200 good, 200–500 needs-improvement, > 500 poor.
#[must_use]
pub fn band_inp(value: f64) -> Band {
    if value < 200.0 {
        Band::Good
    } else if value < 500.0 {
        Band::NeedsImprovement
    } else {
        Band::Poor
    }
}

/// Banded vital — value plus its band.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BandedVital {
    /// Raw measurement (ms for LCP/INP/TTFB/FCP, dimensionless for CLS).
    pub value: f64,
    /// Band per Google CWV thresholds.
    pub band: Band,
}

/// Final snapshot ready for the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VitalSnapshot {
    /// Largest Contentful Paint (ms). Banded.
    pub lcp: Option<BandedVital>,
    /// Cumulative Layout Shift. Banded.
    pub cls: Option<BandedVital>,
    /// Interaction to Next Paint (ms). Banded.
    pub inp: Option<BandedVital>,
    /// Time to First Byte (ms). Not banded — informational.
    pub ttfb: Option<f64>,
    /// First Contentful Paint (ms). Not banded.
    pub fcp: Option<f64>,
    /// Wall time of capture (ms since Unix epoch).
    pub captured_at: u64,
}

/// Raw shape returned by COLLECT_JS — fields may be absent.
#[derive(Debug, Default, Deserialize)]
pub struct RawVitals {
    /// Raw LCP value.
    pub lcp: Option<f64>,
    /// Raw CLS value.
    pub cls: Option<f64>,
    /// Raw INP value.
    pub inp: Option<f64>,
    /// Raw TTFB value.
    pub ttfb: Option<f64>,
    /// Raw FCP value.
    pub fcp: Option<f64>,
}

/// Convert raw deserialised vitals into a banded `VitalSnapshot`.
#[must_use]
pub fn classify(raw: RawVitals, captured_at_ms: u64) -> VitalSnapshot {
    VitalSnapshot {
        lcp: raw.lcp.map(|v| BandedVital {
            value: v,
            band: band_lcp(v),
        }),
        cls: raw.cls.map(|v| BandedVital {
            value: v,
            band: band_cls(v),
        }),
        inp: raw.inp.map(|v| BandedVital {
            value: v,
            band: band_inp(v),
        }),
        ttfb: raw.ttfb,
        fcp: raw.fcp,
        captured_at: captured_at_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_callbacks_balanced() {
        assert_eq!(
            WIRE_CALLBACKS_JS.matches('(').count(),
            WIRE_CALLBACKS_JS.matches(')').count()
        );
    }

    #[test]
    fn wire_callbacks_registers_all_five() {
        for fn_name in ["onLCP", "onCLS", "onINP", "onTTFB", "onFCP"] {
            assert!(
                WIRE_CALLBACKS_JS.contains(fn_name),
                "wire missing: {fn_name}"
            );
        }
    }

    #[test]
    fn band_lcp_thresholds() {
        assert_eq!(band_lcp(2499.9), Band::Good);
        assert_eq!(band_lcp(2500.0), Band::NeedsImprovement);
        assert_eq!(band_lcp(3999.9), Band::NeedsImprovement);
        assert_eq!(band_lcp(4000.0), Band::Poor);
    }

    #[test]
    fn band_cls_thresholds() {
        assert_eq!(band_cls(0.099), Band::Good);
        assert_eq!(band_cls(0.1), Band::NeedsImprovement);
        assert_eq!(band_cls(0.249), Band::NeedsImprovement);
        assert_eq!(band_cls(0.25), Band::Poor);
    }

    #[test]
    fn band_inp_thresholds() {
        assert_eq!(band_inp(199.9), Band::Good);
        assert_eq!(band_inp(200.0), Band::NeedsImprovement);
        assert_eq!(band_inp(499.9), Band::NeedsImprovement);
        assert_eq!(band_inp(500.0), Band::Poor);
    }

    #[test]
    fn classify_partial_raw() {
        // Real-world: TTFB + FCP fire on every page; LCP/CLS only
        // when paint actually happens; INP only on user input.
        let raw = RawVitals {
            lcp: Some(2400.0),
            cls: Some(0.05),
            inp: None,
            ttfb: Some(120.0),
            fcp: Some(800.0),
        };
        let snap = classify(raw, 1_700_000_000_000);
        assert_eq!(snap.lcp.as_ref().expect("lcp").band, Band::Good);
        assert_eq!(snap.cls.as_ref().expect("cls").band, Band::Good);
        assert!(snap.inp.is_none());
        assert_eq!(snap.ttfb, Some(120.0));
        assert_eq!(snap.captured_at, 1_700_000_000_000);
    }

    #[test]
    fn snapshot_round_trips() {
        let snap = VitalSnapshot {
            lcp: Some(BandedVital {
                value: 2400.0,
                band: Band::Good,
            }),
            cls: None,
            inp: None,
            ttfb: Some(100.0),
            fcp: Some(600.0),
            captured_at: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&snap).expect("ser");
        let back: VitalSnapshot = serde_json::from_str(&json).expect("de");
        assert_eq!(back.lcp.as_ref().expect("lcp").value, 2400.0);
    }
}
