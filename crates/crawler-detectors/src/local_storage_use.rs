//! `local_storage_use` — privacy + tracking-surface audit for
//! `localStorage` / `sessionStorage` / `IndexedDB` writes.
//!
//! Per the GDPR / ePrivacy / PIPL guidance baked into
//! `region-adaptation`'s ComplianceRegime helpers: any client-side
//! persistent identifier the site sets is subject to the same
//! consent rules as cookies. Pages that silently write to
//! `localStorage` BEFORE the operator declares a cookie banner
//! produce regulatory liability that the detector flags loud.
//!
//! Beyond compliance, persistent client-side storage is the
//! primary surface advertisers + fingerprinters use to track
//! readers across visits (replacing the cookie they were forced
//! to disclose). The detector also reports the count + first-N
//! keys for the operator to review.
//!
//! Findings:
//!   * `local-storage.first-paint`    warn   any write before
//!                                            first user interaction
//!                                            (likely
//!                                            consent-less tracking)
//!   * `local-storage.large`          warn   single key value
//!                                            > 32 KiB (state-sync
//!                                            risk, not tracking)
//!   * `local-storage.high-count`     warn   > 25 keys total
//!                                            (high cardinality
//!                                            fingerprint surface)
//!   * `indexed-db.present`           warn   any IDB database open
//!                                            (deeper persistence
//!                                            surface)
//!
//! AVP-2 INVARIANTS
//! ----------------
//! * `unsafe_code = "deny"`.
//! * `#[non_exhaustive]` on snapshot types.
//! * Pure detector function; no I/O.

use crate::{AxisFinding, AxisSeverity};
use serde::{Deserialize, Serialize};

/// Large-value threshold in bytes.
pub const VALUE_LARGE_BYTES: usize = 32 * 1024;

/// High-count threshold (per the Storage Access API guidance
/// "small set" cutoff).
pub const KEY_COUNT_HIGH: usize = 25;

/// One captured key entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StorageKey {
    /// Key name.
    pub key: String,
    /// Approximate value byte length (UTF-8 encoded).
    pub bytes: usize,
}

/// Captured client-side storage state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct LocalStorageSnapshot {
    /// Page URL.
    pub page_url: String,
    /// `localStorage` keys at observation time.
    pub keys: Vec<StorageKey>,
    /// Whether any `localStorage` write was observed BEFORE first
    /// user input (click / keydown). Detector consumer captures
    /// this; default `false` if not measured.
    pub wrote_before_interaction: bool,
    /// Whether any `IndexedDB` database was open at observation
    /// time.
    pub has_indexed_db: bool,
}

/// Page-side eval. Captures `localStorage` keys + sizes + an
/// `indexedDB.databases()` count (where supported).
pub const LOCAL_STORAGE_USE_JS: &str = r##"(async () => {
    const keys = [];
    try {
        for (let i = 0; i < localStorage.length; i++) {
            const k = localStorage.key(i);
            if (k == null) continue;
            const v = localStorage.getItem(k) || '';
            keys.push({ key: k, bytes: new Blob([v]).size });
        }
    } catch (e) {}
    let hasIdb = false;
    try {
        if (typeof indexedDB !== 'undefined' && indexedDB.databases) {
            const dbs = await indexedDB.databases();
            hasIdb = Array.isArray(dbs) && dbs.length > 0;
        }
    } catch (e) {}
    // "wroteBeforeInteraction" is provided by the runner harness;
    // page-side JS can't observe its own first-paint vs first-input
    // ordering reliably without prior instrumentation. Default
    // false here.
    return {
        pageUrl: window.location.href,
        keys: keys,
        wroteBeforeInteraction: false,
        hasIndexedDb: hasIdb
    };
})()"##;

/// Run the detector.
pub fn detect_local_storage_issues(snap: &LocalStorageSnapshot) -> Vec<AxisFinding> {
    let mut out = Vec::new();

    if snap.wrote_before_interaction {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "local-storage.first-paint".into(),
            detail: format!(
                "localStorage written before any user interaction ({} key{}); likely consent-less tracking",
                snap.keys.len(),
                if snap.keys.len() == 1 { "" } else { "s" }
            ),
        });
    }

    for k in &snap.keys {
        if k.bytes > VALUE_LARGE_BYTES {
            out.push(AxisFinding {
                severity: AxisSeverity::Warn,
                kind: "local-storage.large".into(),
                detail: format!(
                    "localStorage key {:?} holds {} bytes (> {} KiB threshold; consider IndexedDB or server state)",
                    k.key,
                    k.bytes,
                    VALUE_LARGE_BYTES / 1024
                ),
            });
        }
    }

    if snap.keys.len() > KEY_COUNT_HIGH {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "local-storage.high-count".into(),
            detail: format!(
                "{} localStorage keys present (> {} threshold; high fingerprint surface)",
                snap.keys.len(),
                KEY_COUNT_HIGH
            ),
        });
    }

    if snap.has_indexed_db {
        out.push(AxisFinding {
            severity: AxisSeverity::Warn,
            kind: "indexed-db.present".into(),
            detail: "IndexedDB database open; verify consent + retention policy applies to this surface too".into(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        keys: Vec<(&str, usize)>,
        wrote_before_interaction: bool,
        has_idb: bool,
    ) -> LocalStorageSnapshot {
        LocalStorageSnapshot {
            page_url: "https://example.com/".into(),
            keys: keys
                .into_iter()
                .map(|(k, b)| StorageKey {
                    key: k.to_string(),
                    bytes: b,
                })
                .collect(),
            wrote_before_interaction,
            has_indexed_db: has_idb,
        }
    }

    #[test]
    fn empty_storage_is_clean() {
        assert!(detect_local_storage_issues(&snap(vec![], false, false)).is_empty());
    }

    #[test]
    fn write_before_interaction_warns() {
        let f = detect_local_storage_issues(&snap(vec![("user_id", 36)], true, false));
        assert!(f.iter().any(|x| x.kind == "local-storage.first-paint"));
    }

    #[test]
    fn large_value_warns() {
        let f =
            detect_local_storage_issues(&snap(vec![("blob", VALUE_LARGE_BYTES + 1)], false, false));
        assert!(f.iter().any(|x| x.kind == "local-storage.large"));
    }

    #[test]
    fn small_value_does_not_warn_large() {
        let f = detect_local_storage_issues(&snap(vec![("blob", 1024)], false, false));
        assert!(!f.iter().any(|x| x.kind == "local-storage.large"));
    }

    #[test]
    fn high_count_warns() {
        let keys: Vec<(String, usize)> = (0..30).map(|i| (format!("k{i}"), 4)).collect();
        let keys_ref: Vec<(&str, usize)> = keys.iter().map(|(k, b)| (k.as_str(), *b)).collect();
        let f = detect_local_storage_issues(&snap(keys_ref, false, false));
        assert!(f.iter().any(|x| x.kind == "local-storage.high-count"));
    }

    #[test]
    fn idb_present_warns() {
        let f = detect_local_storage_issues(&snap(vec![], false, true));
        assert!(f.iter().any(|x| x.kind == "indexed-db.present"));
    }

    #[test]
    fn singular_vs_plural_in_first_paint_detail() {
        let f = detect_local_storage_issues(&snap(vec![("only", 4)], true, false));
        let detail = &f
            .iter()
            .find(|x| x.kind == "local-storage.first-paint")
            .unwrap()
            .detail;
        // singular " key " not " keys "
        assert!(detail.contains("1 key"));
        assert!(!detail.contains("keys"));
    }

    #[test]
    fn multiple_findings_compose() {
        let mut keys: Vec<(String, usize)> = (0..30).map(|i| (format!("k{i}"), 4)).collect();
        keys.push(("blob".to_string(), VALUE_LARGE_BYTES + 1));
        let keys_ref: Vec<(&str, usize)> = keys.iter().map(|(k, b)| (k.as_str(), *b)).collect();
        let f = detect_local_storage_issues(&snap(keys_ref, true, true));
        let kinds: Vec<&str> = f.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"local-storage.first-paint"));
        assert!(kinds.contains(&"local-storage.large"));
        assert!(kinds.contains(&"local-storage.high-count"));
        assert!(kinds.contains(&"indexed-db.present"));
    }
}
