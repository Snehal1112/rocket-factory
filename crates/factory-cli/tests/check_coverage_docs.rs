use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).expect("read doc file")
}

#[test]
fn reference_documents_the_override_rule() {
    let text = read("docs/reference/project-toml-reference.md");
    assert!(text.contains(
        "A project that sets `[[settings.checks]]` or `unchecked_paths` replaces the preset value, so copy the preset entries it still wants."
    ));
    assert!(text.contains("paths = [\"Cargo.toml\", \"Cargo.lock\"]"));
}

#[test]
fn software_factory_mentions_the_root_manifest_rule() {
    let text = read("docs/reference/software-factory.md");
    assert!(text.contains(
        "The rust preset covers the root `Cargo.toml` and `Cargo.lock` with `cargo check -j4`."
    ));
}

#[test]
fn readme_and_getting_started_carry_the_check_coverage_note() {
    for rel in ["README.md", "docs/guides/getting-started.md"] {
        let text = read(rel);
        assert!(text.contains("## Check coverage"), "{rel} has the heading");
        assert!(
            text.contains("factory --project <name> status"),
            "{rel} names status"
        );
        assert!(
            text.contains("[[settings.checks]]"),
            "{rel} names the override"
        );
    }
}

#[test]
fn releasing_has_the_paste_step() {
    let text = read("docs/operations/releasing.md");
    assert!(text.contains(
        "Paste the \"Check coverage\" note from `README.md` (Upgrading) into the draft release."
    ));
}

#[test]
fn check_coverage_note_comes_after_the_upgrade_steps() {
    let text = read("README.md");
    let steps = text
        .find("factory upgrade --dry-run")
        .expect("upgrade steps");
    let note = text.find("## Check coverage").expect("note heading");
    assert!(note > steps);
}
