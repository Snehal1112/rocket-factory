use std::process::Command;

use factory_cli::{guard_children, CliCtx};

#[test]
fn guard_children_registers_children() {
    let tmp = tempfile::tempdir().expect("tmp");
    let ctx = CliCtx::from_env(Some(tmp.path().to_path_buf())).expect("ctx");
    std::fs::create_dir_all(&ctx.layout.state_dir).expect("state dir");
    guard_children(&ctx);
    let mut child =
        factory_procgroup::spawn(Command::new("sleep").arg("30")).expect("spawn sleeper");
    let text = std::fs::read_to_string(ctx.layout.children_file(&ctx.owner)).expect("registry");
    let entries: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(entries[0]["pid"], child.id());
    child.kill().expect("kill");
}

#[test]
fn main_guards_children_for_every_project_command() {
    let main = include_str!("../src/main.rs");
    assert_eq!(main.matches("guard_children(").count(), 1);
    let built = main.find("let ctx = match built").expect("ctx block");
    let call = main.find("guard_children(").expect("call");
    assert!(
        call > built,
        "the guard must come after the context is built"
    );
}
