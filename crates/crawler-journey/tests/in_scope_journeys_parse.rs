//! Smoke test: every in-scope journey JSON parses cleanly.
//!
//! Catches regressions in the Step deserializer that would break a
//! valid journey, and catches stale journeys that accumulated
//! unknown fields after a schema bump.
//!
//! Out-of-scope families excluded:
//!   * `sacred-vote*` and `skillshots-poc*` — use step kinds
//!     (`assertCount`, `assertText`, `stress`, `fill`) the Rust
//!     port doesn't implement; ports tracked separately.
//!   * `*.whitelist.json` — not a journey but a finding-whitelist
//!     file colocated in `journeys/` by convention.

use std::path::PathBuf;

fn journeys_dir() -> PathBuf {
    // crawler-journey is at crates/crawler-journey/; CARGO_MANIFEST_DIR
    // resolves to that crate. Walk up two levels to the repo root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("journeys"))
        .expect("journeys dir resolvable from crate manifest")
}

fn is_in_scope(name: &str) -> bool {
    if name.ends_with(".whitelist.json") {
        return false;
    }
    if name.starts_with("sacred-vote") || name.starts_with("sacredvote") {
        return false;
    }
    if name.starts_with("skillshots-poc") {
        return false;
    }
    true
}

#[test]
fn every_in_scope_journey_parses_via_crawler_journey_load() {
    let dir = journeys_dir();
    assert!(
        dir.exists() && dir.is_dir(),
        "journeys dir must exist at {}",
        dir.display()
    );
    let mut tried = 0usize;
    let mut failed: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("read journeys dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<no-name>");
        if !is_in_scope(name) {
            continue;
        }
        tried += 1;
        if let Err(e) = crawler_journey::load(&path) {
            failed.push((name.to_owned(), format!("{e}")));
        }
    }
    assert!(tried > 0, "expected at least one in-scope journey to parse");
    if !failed.is_empty() {
        let mut msg = format!(
            "{} of {} in-scope journeys failed to parse:\n",
            failed.len(),
            tried
        );
        for (n, e) in &failed {
            msg.push_str(&format!("  {n}\n    {e}\n"));
        }
        panic!("{msg}");
    }
}
