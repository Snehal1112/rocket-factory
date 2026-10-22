mod common;

use std::path::{Path, PathBuf};

use clap::Parser;
use factory_cli::install::GLOBAL_COMMAND;
use factory_cli::{project, upgrade, Cli, Command};
use factory_core::version::{read_marker, FACTORY_VERSION};
use factory_core::{preset, Home};
use git2::{Repository, RepositoryInitOptions, Signature};

const LEGACY_BUILDER: &str =
    include_str!("../../factory-core/presets/legacy/rust-factory-builder.md");

fn make_repo(parent: &Path, folder: &str) -> PathBuf {
    let path = parent.join(folder);
    std::fs::create_dir_all(&path).expect("mkdir");
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(&path, &opts).expect("init");
    std::fs::write(path.join("a.txt"), "a").expect("write");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new("a.txt")).expect("add");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let sig = Signature::now("t", "t@t").expect("sig");
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .expect("commit");
    path
}

fn project_run(home: &Home, args: &[&str]) -> Result<String, factory_cli::CliError> {
    let mut full = vec!["factory", "project"];
    full.extend_from_slice(args);
    match Cli::try_parse_from(full).expect("parse").command {
        Command::Project { cmd } => project::run(home, cmd),
        _ => unreachable!(),
    }
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf8")
}

struct Env {
    tmp: tempfile::TempDir,
    home: Home,
    commands: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let commands = tmp.path().join("commands");
    Env {
        tmp,
        home,
        commands,
    }
}

/// Registers `demo` (rust) on a repo that commits a repo-layer builder, and adds a project-layer builder.
fn demo_with_overrides(e: &Env) -> PathBuf {
    let repo = make_repo(e.tmp.path(), "demo-repo");
    common::commit_file(
        &repo,
        ".factory/agents/factory-builder.md",
        "# repo override\n",
    );
    project_run(
        &e.home,
        &[
            "add",
            s(&repo),
            "--preset",
            "rust",
            "--name",
            "demo",
            "--base",
            "main",
        ],
    )
    .expect("add");
    let layer = e.home.project_dir("demo").join("agents/factory-builder.md");
    std::fs::create_dir_all(layer.parent().expect("parent")).expect("mkdir");
    std::fs::write(&layer, "# project override\n").expect("write");
    repo
}

fn builder(e: &Env) -> PathBuf {
    e.home.presets_dir().join("rust/agents/factory-builder.md")
}

fn old_command(e: &Env) -> PathBuf {
    std::fs::create_dir_all(&e.commands).expect("mkdir");
    let cmd = e.commands.join("factory.md");
    std::fs::write(&cmd, "# old\n").expect("old");
    cmd
}

#[test]
fn codex_skill_backup_failure_does_not_block_claude_upgrade() {
    let e = env();
    let commands = factory_core::claude_commands_dir(e.tmp.path());
    std::fs::create_dir_all(&commands).expect("commands folder");
    let claude = commands.join("factory.md");
    std::fs::write(&claude, "# old Claude command\n").expect("old Claude command");
    let skill = factory_core::codex_skills_dir(e.tmp.path())
        .join("factory")
        .join("SKILL.md");
    std::fs::create_dir_all(skill.parent().expect("skill folder")).expect("skill folder");
    std::fs::write(&skill, "custom Codex skill").expect("skill");
    std::fs::create_dir(skill.with_file_name("SKILL.md.bak")).expect("block backup");

    let err = upgrade::run(&e.home, &commands, false)
        .expect_err("partial upgrade reports Codex failure")
        .to_string();
    assert!(err.contains("Claude upgrade completed"), "{err}");
    assert!(err.contains("Codex skill not upgraded"), "{err}");
    assert_eq!(std::fs::read_to_string(claude).expect("Claude command"), GLOBAL_COMMAND);
    assert_eq!(std::fs::read_to_string(skill).expect("Codex skill"), "custom Codex skill");
    assert_eq!(read_marker(&e.home).expect("marker").as_deref(), Some(FACTORY_VERSION));
}

#[test]
fn upgrade_reports_shadowing_overrides() {
    let e = env();
    demo_with_overrides(&e);
    std::fs::write(builder(&e), LEGACY_BUILDER).expect("old text");
    // `project add` already stamped the marker, so remove it to see upgrade write it.
    std::fs::remove_file(e.home.version_file()).expect("rm marker");
    let out = upgrade::run(&e.home, &e.commands, false).expect("upgrade");
    assert!(
        out.contains(&format!("updated: {}", builder(&e).display())),
        "{out}"
    );
    assert!(
        out.contains("shadowed: demo project-layer factory-builder.md"),
        "{out}"
    );
    assert!(
        out.contains("shadowed: demo repo-layer factory-builder.md"),
        "{out}"
    );
    assert_eq!(
        out.lines().last().expect("last"),
        format!("version marker: written {FACTORY_VERSION}")
    );
    assert_eq!(
        std::fs::read_to_string(e.home.project_dir("demo").join("agents/factory-builder.md"))
            .expect("read"),
        "# project override\n"
    );
}

#[test]
fn upgrade_updates_the_command_and_keeps_a_bak() {
    let e = env();
    let cmd = old_command(&e);
    let out = upgrade::run(&e.home, &e.commands, false).expect("upgrade");
    let bak = e.commands.join("factory.md.bak");
    assert_eq!(
        out.lines().next().expect("first"),
        format!(
            "updated {} (old copy saved as {})",
            cmd.display(),
            bak.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&bak).expect("bak"), "# old\n");
    assert_eq!(std::fs::read_to_string(&cmd).expect("cmd"), GLOBAL_COMMAND);
    assert!(
        out.contains("24 written, 0 updated, 0 unchanged, 0 refused"),
        "{out}"
    );
    assert_eq!(
        read_marker(&e.home)
            .expect("marker")
            .as_deref()
            .map(str::trim),
        Some(FACTORY_VERSION)
    );
}

#[test]
fn upgrade_updates_the_codex_skill_with_a_backup_under_the_standard_home() {
    let e = env();
    let commands = factory_core::claude_commands_dir(&e.home.dir);
    let skill = factory_core::codex_skills_dir(&e.home.dir)
        .join("factory")
        .join("SKILL.md");
    std::fs::create_dir_all(skill.parent().expect("skill dir")).expect("mkdir");
    std::fs::write(&skill, "# old Codex skill\n").expect("old skill");

    let dry = upgrade::run(&e.home, &commands, true).expect("dry run");
    assert!(
        dry.contains(&format!("would updated {}", skill.display())),
        "{dry}"
    );
    assert_eq!(
        std::fs::read_to_string(&skill).expect("old remains"),
        "# old Codex skill\n"
    );

    let out = upgrade::run(&e.home, &commands, false).expect("upgrade");

    let backup = skill.with_file_name("SKILL.md.bak");
    assert!(
        out.contains(&format!(
            "updated {} (old copy saved as {})",
            skill.display(),
            backup.display()
        )),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(backup).expect("backup"),
        "# old Codex skill\n"
    );
    let text = std::fs::read_to_string(skill).expect("new skill");
    assert!(
        text.starts_with("---\nname: factory\ndescription: "),
        "{text}"
    );
    assert!(text.contains("$factory"), "{text}");
}

#[test]
fn upgrade_dry_run_changes_nothing() {
    let e = env();
    let cmd = old_command(&e);
    preset::seed(&e.home.presets_dir()).expect("seed");
    std::fs::write(builder(&e), LEGACY_BUILDER).expect("old text");
    let manifest = e.home.presets_dir().join(".manifest");
    let before = std::fs::read(&manifest).ok();
    let out = upgrade::run(&e.home, &e.commands, true).expect("dry run");
    assert!(
        out.lines()
            .next()
            .expect("first")
            .starts_with("would updated "),
        "{out}"
    );
    assert!(
        out.contains(&format!("would updated: {}", builder(&e).display())),
        "{out}"
    );
    assert_eq!(
        out.lines().last().expect("last"),
        format!("would version marker: written {FACTORY_VERSION}")
    );
    assert_eq!(std::fs::read_to_string(&cmd).expect("cmd"), "# old\n");
    assert!(!e.commands.join("factory.md.bak").exists());
    assert_eq!(
        std::fs::read_to_string(builder(&e)).expect("b"),
        LEGACY_BUILDER
    );
    assert_eq!(std::fs::read(&manifest).ok(), before);
    assert!(!e.home.version_file().exists());
}

#[test]
fn upgrade_refuses_edited_presets_and_says_how_to_force() {
    let e = env();
    preset::seed(&e.home.presets_dir()).expect("seed");
    let config = e.home.presets_dir().join("rust/config.toml");
    std::fs::write(&config, "# mine\n").expect("edit");
    let out = upgrade::run(&e.home, &e.commands, false).expect("upgrade");
    assert!(
        out.contains(&format!("refused: {}", config.display())),
        "{out}"
    );
    assert!(
        out.contains(
            "refused files stay as they are: factory presets refresh --force overwrites them"
        ),
        "{out}"
    );
    assert!(!out.contains("use --force to overwrite them"), "{out}");
    assert_eq!(std::fs::read_to_string(&config).expect("read"), "# mine\n");
}

#[test]
fn upgrade_stops_on_a_newer_marker() {
    let e = env();
    std::fs::create_dir_all(&e.home.dir).expect("mkdir");
    std::fs::write(e.home.version_file(), "999.0.0\n").expect("marker");
    let err = upgrade::run(&e.home, &e.commands, false).expect_err("newer marker");
    let text = err.to_string();
    assert!(text.contains("999.0.0"), "{text}");
    assert!(text.contains(FACTORY_VERSION), "{text}");
    assert!(!e.commands.join("factory.md").exists());
    assert!(!e.home.presets_dir().exists());
    assert_eq!(
        std::fs::read_to_string(e.home.version_file()).expect("read"),
        "999.0.0\n"
    );
}

#[test]
fn a_second_upgrade_changes_nothing() {
    let e = env();
    demo_with_overrides(&e);
    std::fs::write(builder(&e), LEGACY_BUILDER).expect("old text");
    upgrade::run(&e.home, &e.commands, false).expect("first");
    let out = upgrade::run(&e.home, &e.commands, false).expect("second");
    assert_eq!(
        out.lines().next().expect("first"),
        format!(
            "already installed: {}",
            e.commands.join("factory.md").display()
        )
    );
    assert!(
        out.contains("0 written, 0 updated, 24 unchanged, 0 refused"),
        "{out}"
    );
    assert!(!out.contains("shadowed:"), "{out}");
    assert!(!out.contains("would "), "{out}");
    assert_eq!(
        out.lines().last().expect("last"),
        format!("version marker: {FACTORY_VERSION}")
    );
}

#[test]
fn upgrade_stops_when_the_bak_cannot_be_written() {
    let e = env();
    let cmd = old_command(&e);
    let keep = e.commands.join("factory.md.bak/keep");
    std::fs::create_dir_all(keep.parent().expect("parent")).expect("mkdir");
    std::fs::write(&keep, "x").expect("keep");
    let err = upgrade::run(&e.home, &e.commands, false).expect_err("bak blocked");
    assert!(err.to_string().contains("cannot save"), "{err}");
    assert_eq!(std::fs::read_to_string(&cmd).expect("cmd"), "# old\n");
    assert!(!e.home.presets_dir().exists());
    assert!(!e.home.version_file().exists());
}

#[test]
fn upgrade_dry_run_reports_shadowing_from_the_files_it_would_update() {
    let e = env();
    demo_with_overrides(&e);
    std::fs::write(builder(&e), LEGACY_BUILDER).expect("old text");
    let out = upgrade::run(&e.home, &e.commands, true).expect("dry run");
    assert!(
        out.contains("shadowed: demo project-layer factory-builder.md"),
        "{out}"
    );
    assert!(
        out.contains("shadowed: demo repo-layer factory-builder.md"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_to_string(builder(&e)).expect("b"),
        LEGACY_BUILDER
    );
}

#[test]
fn upgrade_skips_a_project_whose_repo_is_gone() {
    let e = env();
    let demo_repo = demo_with_overrides(&e);
    let other = make_repo(e.tmp.path(), "other-repo");
    common::commit_file(
        &other,
        ".factory/agents/factory-builder.md",
        "# repo override\n",
    );
    project_run(
        &e.home,
        &[
            "add",
            s(&other),
            "--preset",
            "rust",
            "--name",
            "other",
            "--base",
            "main",
        ],
    )
    .expect("add other");
    std::fs::write(builder(&e), LEGACY_BUILDER).expect("old text");
    std::fs::remove_dir_all(&demo_repo).expect("rm repo");
    let out = upgrade::run(&e.home, &e.commands, false).expect("upgrade");
    let skipped: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("skipped: demo ("))
        .collect();
    assert_eq!(skipped.len(), 1, "{out}");
    assert!(
        !out.lines().any(|l| l.starts_with("shadowed: demo ")),
        "{out}"
    );
    assert!(
        out.contains("shadowed: other repo-layer factory-builder.md"),
        "{out}"
    );
}

#[test]
fn upgrade_without_home_fails_before_any_change() {
    let e = env();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_factory"))
        .arg("upgrade")
        .env_remove("HOME")
        .env("FACTORY_HOME", &e.home.dir)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("$HOME is not set"), "{err}");
    assert!(!e.home.version_file().exists());
    assert!(!e.home.presets_dir().exists());
}
