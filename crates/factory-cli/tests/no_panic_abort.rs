//! A `panic = "abort"` profile would turn the panic boundary of the soft steps off.

use std::path::{Path, PathBuf};

fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("Cargo.toml"), root.join("dashboard/Cargo.toml")];
    for entry in std::fs::read_dir(root.join("crates")).expect("read crates") {
        let path = entry.expect("entry").path().join("Cargo.toml");
        if path.is_file() {
            out.push(path);
        }
    }
    out
}

#[test]
fn no_panic_abort_profile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    for file in manifests(&root) {
        let text = std::fs::read_to_string(&file).expect("read manifest");
        if text.contains("panic = \"abort\"") {
            hits.push(file.display().to_string());
        }
    }
    assert!(hits.is_empty(), "panic = \"abort\" is set in: {hits:?}");
}
