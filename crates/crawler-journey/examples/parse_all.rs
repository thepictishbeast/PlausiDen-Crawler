//! Smoke test: parse every shipping journey JSON. Exits non-zero
//! on the first failure so CI can wire it as a guard.
fn main() -> std::io::Result<()> {
    let mut ok = 0usize;
    let mut bad: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir("journeys")? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match crawler_journey::load(&path) {
            Ok(j) => {
                println!("  OK   {} ({} steps)", path.display(), j.steps.len());
                ok += 1;
            }
            Err(e) => {
                bad.push((path.display().to_string(), format!("{e}")));
            }
        }
    }
    println!("\nparsed {ok} ok, {} bad", bad.len());
    for (p, e) in &bad {
        println!("  FAIL {p}\n    {e}");
    }
    std::process::exit(if bad.is_empty() { 0 } else { 1 });
}
