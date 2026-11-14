//! Only `compat.rs` and `intake.rs` may reach the unguarded tracker, so a step added later gets
//! the guard through `tracker_for`.

use std::path::{Path, PathBuf};

const ALLOWED: &[&str] = &["compat.rs", "intake.rs"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn only_intake_uses_the_raw_tracker() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty(), "no source files found");
    let hits: Vec<String> = files
        .iter()
        .filter(|f| {
            let rel = f.strip_prefix(&src).expect("under src");
            !ALLOWED.iter().any(|a| rel == Path::new(a))
        })
        .filter(|f| {
            std::fs::read_to_string(f)
                .expect("read")
                .contains("raw_tracker")
        })
        .map(|f| f.display().to_string())
        .collect();
    assert!(
        hits.is_empty(),
        "raw_tracker used outside intake.rs: {hits:?}"
    );
}
