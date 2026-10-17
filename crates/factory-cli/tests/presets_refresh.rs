use clap::Parser;
use factory_cli::{presets, Cli, Command};
use factory_core::{preset, Home};

fn run(home: &Home, args: &[&str]) -> String {
    let mut full = vec!["factory", "presets"];
    full.extend_from_slice(args);
    let cli = Cli::try_parse_from(full).expect("parse");
    match cli.command {
        Command::Presets { cmd } => presets::run(home, cmd).expect("run"),
        _ => unreachable!(),
    }
}

#[test]
fn the_command_reports_each_outcome_and_refused_paths() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = Home::at(tmp.path().join("home"));
    preset::seed(&home.presets_dir()).expect("seed");
    let edited = home.presets_dir().join("rust/config.toml");
    std::fs::write(&edited, "# mine\n").expect("edit");
    let removed = home.presets_dir().join("go/commit-context.md");
    std::fs::remove_file(&removed).expect("rm");

    let out = run(&home, &["refresh"]);
    assert!(out.contains(&format!("written: {}", removed.display())), "{out}");
    assert!(out.contains(&format!("refused: {}", edited.display())), "{out}");
    assert!(out.contains("unchanged: "), "{out}");
    assert!(out.contains("1 written, 0 updated, 10 unchanged, 1 refused"), "{out}");
    assert!(out.contains("--force"), "{out}");
    assert_eq!(std::fs::read_to_string(&edited).expect("read"), "# mine\n");

    let out = run(&home, &["refresh", "--force"]);
    assert!(out.contains(&format!("updated: {}", edited.display())), "{out}");
    assert!(out.contains("0 refused"), "{out}");
    assert!(!out.contains("--force"), "{out}");
}
