mod common;

use std::sync::{Arc, Mutex};

use common::{commit_file, committing_fixture, scripted, scripted_auto, Effect, Fixture};
use factory_core::{AgentFile, Stage};
use factory_engine::{EngineError, StageOutcome};
use git2::{Repository, StatusOptions};

const BUILDER: &str = ".claude/agents/factory-builder.md";

fn files() -> Vec<AgentFile> {
    vec![
        AgentFile {
            name: "factory-builder.md".into(),
            content: "BUILDER".into(),
        },
        AgentFile {
            name: "factory-reviewer.md".into(),
            content: "REVIEWER".into(),
        },
    ]
}

/// The prompt of `name` in the agents file of a recorded spec, `None` without an agents file.
fn agents_prompt(spec: &factory_agent::AgentSpec, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(spec.agents_file.as_ref()?).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json[name]["prompt"].as_str().map(str::to_string)
}

/// A builder that records the builder prompt of its agents file, then writes a change.
fn recording_effect(seen: Arc<Mutex<Vec<Option<String>>>>) -> Effect {
    Box::new(move |spec| {
        seen.lock()
            .expect("seen")
            .push(agents_prompt(spec, "factory-builder"));
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    })
}

#[test]
fn the_agents_file_exists_before_the_builder_runs_and_the_commit_holds_no_agent_files() {
    let fx = committing_fixture();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let runner = scripted_auto(vec![recording_effect(seen.clone())]);
    let out = fx
        .engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("BUILDER".to_string())]
    );

    let job = fx.job();
    let wt = std::path::PathBuf::from(job.worktree.clone().expect("worktree"));
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let repo = Repository::open(&wt).expect("open wt");
    let st = repo.statuses(Some(&mut opts)).expect("statuses");
    assert!(
        st.iter()
            .all(|e| !e.path().unwrap_or("").contains("factory-")),
        "no agent file may be in the worktree"
    );
    let head = repo
        .find_commit(git2::Oid::from_str(&job.head.expect("head")).expect("oid"))
        .expect("commit");
    let tree = head.tree().expect("tree");
    assert!(tree
        .get_path(std::path::Path::new(".claude/agents"))
        .is_err());
    assert!(tree.get_path(std::path::Path::new("a.txt")).is_ok());
}

#[test]
fn a_file_tracked_on_the_base_branch_wins_in_the_agents_file() {
    let fx = committing_fixture();
    commit_file(&fx.main, BUILDER, "REPO");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let runner = scripted_auto(vec![recording_effect(seen.clone())]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("REPO".to_string())]
    );
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(
        agents_prompt(&spec, "factory-reviewer").as_deref(),
        Some("REVIEWER")
    );
}

#[test]
fn no_files_means_no_agents_folder() {
    let fx = committing_fixture();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let runner = scripted_auto(vec![recording_effect(seen)]);
    fx.engine(&runner).build_stage().expect("build");
    let wt = std::path::PathBuf::from(fx.job().worktree.expect("worktree"));
    assert!(!wt.join(".claude/agents").exists());
}

const TWO_TASKS: &str = "# P\n\n### Task 1: first\n\nx\n\n### Task 2: second\n\ny\n";

fn write_effect(file: &'static str) -> Effect {
    Box::new(move |spec| {
        std::fs::write(spec.cwd.join(file), "x\n").expect("write");
        "done".to_string()
    })
}

fn worktree(fx: &Fixture) -> std::path::PathBuf {
    std::path::PathBuf::from(fx.job().worktree.expect("worktree"))
}

/// A fixture scoped to the `web` project in `apps/web`, which exists on the base branch.
fn scoped_fixture() -> Fixture {
    let mut fx = committing_fixture();
    commit_file(&fx.main, "apps/web/index.txt", "web\n");
    fx.repo = factory_git::Repo::open(&fx.main)
        .expect("open")
        .with_scope("web", std::path::Path::new("apps/web"));
    fx
}

#[test]
fn builder_starts_in_the_project_folder() {
    let fx = scoped_fixture();
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let wt = worktree(&fx);
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.cwd, wt.join("apps/web"));
    assert!(spec.agents_file.is_some());
    assert!(!wt.join(".claude").exists());
    assert!(!wt.join("apps/web/.claude").exists());
}

#[test]
fn top_folder_project_cwd_is_unchanged() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.cwd, worktree(&fx));
}

#[test]
fn a_later_build_step_rewrites_a_removed_agents_file() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("first");
    std::fs::remove_file(fx.job_dir.join("agents.json")).expect("remove");
    e.build_stage().expect("second");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(
        agents_prompt(&spec, "factory-builder").as_deref(),
        Some("BUILDER")
    );
}

#[test]
fn a_file_edited_by_the_builder_does_not_change_the_reviewers_agents() {
    let fx = committing_fixture();
    let tamper: Effect = Box::new(|spec| {
        let dir = spec.cwd.join(".claude/agents");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("factory-reviewer.md"), "WEAKENED").expect("tamper");
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    });
    let runner = scripted_auto(vec![tamper]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(files())
        .checks_stage()
        .expect("checks");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    let reviewer = scripted_auto(vec![Box::new(move |spec| {
        seen2
            .lock()
            .expect("seen")
            .push(agents_prompt(spec, "factory-reviewer"));
        "ok\nVERDICT: APPROVE".to_string()
    })]);
    fx.engine(&reviewer)
        .with_agent_files(files())
        .review_stage()
        .expect("review");
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("REVIEWER".to_string())]
    );
}

#[test]
fn a_resumed_step_gets_the_agents_file_missed_by_the_first_step() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let second: Effect = {
        let seen = seen.clone();
        Box::new(move |spec| {
            seen.lock()
                .expect("seen")
                .push(agents_prompt(spec, "factory-builder"));
            std::fs::write(spec.cwd.join("b.txt"), "b\n").expect("write");
            "done".to_string()
        })
    };
    let runner = scripted_auto(vec![write_effect("a.txt"), second]);
    // The first step runs without files, as after a crash that lost them.
    fx.engine(&runner).build_stage().expect("first");
    assert!(runner
        .last_spec_for("builder")
        .expect("builder")
        .agents_file
        .is_none());
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("second");
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        &[Some("BUILDER".to_string())]
    );
}

#[test]
fn names_outside_the_allow_list_are_refused() {
    for bad in [
        "../evil.md",
        "builder.md",
        "factory-x.txt",
        "factory-A.md",
        "factory-a/b.md",
        "factory-.md",
    ] {
        let fx: Fixture = committing_fixture();
        let runner = scripted_auto(vec![]);
        let list = vec![AgentFile {
            name: bad.into(),
            content: "x".into(),
        }];
        let err = fx
            .engine(&runner)
            .with_agent_files(list)
            .build_stage()
            .expect_err(bad);
        assert!(
            matches!(&err, EngineError::Msg(m) if m.contains(bad)),
            "{bad}: {err}"
        );
        assert!(fx.job().worktree.is_none(), "no worktree for {bad}");
    }
}

#[test]
fn a_resumed_step_refuses_bad_names_and_writes_nothing() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let runner = scripted_auto(vec![write_effect("a.txt"), write_effect("b.txt")]);
    fx.engine(&runner).build_stage().expect("first");
    let wt = worktree(&fx);
    for bad in ["../evil.md", "Factory-X.md", "factory-x.md.sh"] {
        let list = vec![
            AgentFile {
                name: "factory-good.md".into(),
                content: "G".into(),
            },
            AgentFile {
                name: bad.into(),
                content: "E".into(),
            },
        ];
        let err = fx
            .engine(&runner)
            .with_agent_files(list)
            .build_stage()
            .expect_err(bad);
        assert!(
            matches!(&err, EngineError::Msg(m) if m.contains(bad)),
            "{bad}: {err}"
        );
        assert!(!wt.join(".claude").exists(), "nothing created for {bad}");
        assert!(!wt.join("evil.md").exists() && !wt.join(".claude/evil.md").exists());
        assert!(!wt.join("../evil.md").exists());
    }
}

/// The agent file list of a full factory: six agents with marker-less text.
fn six_files() -> Vec<AgentFile> {
    [
        "builder",
        "code-reviewer",
        "committer",
        "kb-refresher",
        "security-reviewer",
        "spec-reviewer",
    ]
    .iter()
    .map(|n| AgentFile {
        name: format!("factory-{n}.md"),
        content: format!("---\nname: factory-{n}\ndescription: d {n}\n---\nRESOLVED {n}\n"),
    })
    .collect()
}

/// The parsed `--agents` file a recorded spec points at.
fn agents_of(spec: &factory_agent::AgentSpec) -> serde_json::Value {
    let path = spec.agents_file.as_ref().expect("agents_file");
    let text = std::fs::read_to_string(path).expect("read agents.json");
    serde_json::from_str(&text).expect("agents.json is JSON")
}

fn prompt_of(json: &serde_json::Value, name: &str) -> String {
    json[name]["prompt"]
        .as_str()
        .expect("prompt is a string")
        .to_string()
}

fn delete_builder_file() -> Effect {
    Box::new(|spec| {
        std::fs::remove_file(spec.cwd.join(BUILDER)).expect("delete");
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    })
}

#[test]
fn a_tracked_agent_text_wins_in_the_agents_file() {
    let fx = committing_fixture();
    commit_file(&fx.main, BUILDER, "TRACKED-TEXT");
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect("build");
    let json = agents_of(&runner.last_spec_for("builder").expect("builder"));
    assert!(prompt_of(&json, "factory-builder").contains("TRACKED-TEXT"));
    assert!(prompt_of(&json, "factory-committer").contains("RESOLVED committer"));
    assert_eq!(json.as_object().expect("object").len(), 6);
}

#[test]
fn a_job_that_deletes_a_tracked_agent_file_still_gets_its_agents() {
    let fx = committing_fixture();
    commit_file(&fx.main, BUILDER, "TRACKED-TEXT");
    let runner = scripted_auto(vec![delete_builder_file()]);
    fx.engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(six_files())
        .checks_stage()
        .expect("checks");
    let reviewer = scripted(vec![Box::new(|_| "ok\nVERDICT: APPROVE".to_string())]);
    fx.engine(&reviewer)
        .with_agent_files(six_files())
        .review_stage()
        .expect("review");
    let specs = [
        runner.last_spec_for("committer").expect("committer"),
        reviewer.last_spec_for("reviewer").expect("reviewer"),
    ];
    for spec in specs {
        let json = agents_of(&spec);
        assert!(
            prompt_of(&json, "factory-builder").contains("TRACKED-TEXT"),
            "{json}"
        );
        for name in [
            "code-reviewer",
            "committer",
            "kb-refresher",
            "spec-reviewer",
        ] {
            let key = format!("factory-{name}");
            assert_eq!(prompt_of(&json, &key), format!("RESOLVED {name}\n"));
        }
    }
}

#[test]
fn the_rules_marker_is_read_from_the_effective_text() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    commit_file(&fx.main, BUILDER, "<!-- factory-rules: 1 -->\nTRACKED\n");
    let runner = scripted_auto(vec![delete_builder_file(), write_effect("b.txt")]);
    let e = fx.engine(&runner).with_agent_files(six_files());
    e.build_stage().expect("first");
    e.build_stage().expect("second");
    assert!(!worktree(&fx).join(BUILDER).exists());
    let prompt = runner.last_spec_for("builder").expect("builder").prompt;
    assert!(prompt.contains("Do only Task 2"), "{prompt}");
    assert!(
        !prompt.contains("Status: DONE"),
        "short form expected: {prompt}"
    );
}

#[test]
fn a_codex_run_sends_no_agents_file() {
    let mut fx = committing_fixture();
    fx.config.agent_cli = Some("codex".to_string());
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.agents_file, None);
    assert!(!fx.job_dir.join("agents.json").exists());
}

#[test]
fn an_unreadable_tracked_agent_file_stops_the_run() {
    let fx = committing_fixture();
    std::fs::create_dir_all(fx.main.join(".claude/agents")).expect("mkdir");
    std::os::unix::fs::symlink("/etc/hostname", fx.main.join(BUILDER)).expect("symlink");
    let repo = Repository::open(&fx.main).expect("open");
    let mut index = repo.index().expect("index");
    index.add_path(std::path::Path::new(BUILDER)).expect("add");
    index.write().expect("write index");
    common::commit_index(&fx.main, "track a symlink");
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    let err = fx
        .engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect_err("unreadable file");
    assert!(err.to_string().contains("factory-builder.md"), "{err}");
    assert!(runner.last_spec_for("builder").is_none());
}

#[test]
fn no_agent_file_is_written_into_the_worktree() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(six_files())
        .checks_stage()
        .expect("checks");
    let reviewer = scripted(vec![Box::new(|_| "ok\nVERDICT: APPROVE".to_string())]);
    fx.engine(&reviewer)
        .with_agent_files(six_files())
        .review_stage()
        .expect("review");
    assert!(!worktree(&fx).join(".claude/agents").exists());
    assert!(!fx.main.join(".claude/agents").exists());
    let repo = Repository::open(&fx.main).expect("open");
    let exclude = std::fs::read_to_string(repo.path().join("info/exclude")).unwrap_or_default();
    assert!(!exclude.contains("factory-"), "{exclude}");
}

#[test]
fn no_shadow_ruling_is_written() {
    let fx = committing_fixture();
    commit_file(&fx.main, BUILDER, "TRACKED-TEXT");
    let runner = scripted_auto(vec![write_effect("a.txt")]);
    fx.engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect("build");
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger");
    assert!(!ledger.contains("shadow"), "{ledger}");
}

#[test]
fn a_job_that_deletes_the_tracked_agent_files_reaches_review() {
    let fx = committing_fixture();
    for file in six_files() {
        commit_file(
            &fx.main,
            &format!(".claude/agents/{}", file.name),
            &format!("TRACKED {}", file.name),
        );
    }
    let delete_all: Effect = Box::new(|spec| {
        std::fs::remove_dir_all(spec.cwd.join(".claude/agents")).expect("delete");
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    });
    let runner = scripted_auto(vec![delete_all]);
    fx.engine(&runner)
        .with_agent_files(six_files())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(six_files())
        .checks_stage()
        .expect("checks");
    let reviewer = scripted(vec![Box::new(|_| "ok\nVERDICT: APPROVE".to_string())]);
    fx.engine(&reviewer)
        .with_agent_files(six_files())
        .review_stage()
        .expect("review");
    let specs = [
        runner.last_spec_for("committer").expect("committer"),
        reviewer.last_spec_for("reviewer").expect("reviewer"),
    ];
    for spec in specs {
        assert_eq!(
            agents_prompt(&spec, "factory-committer").as_deref(),
            Some("TRACKED factory-committer.md")
        );
    }
}
