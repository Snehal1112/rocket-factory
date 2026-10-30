mod common;

use common::{committing_fixture, reply, scripted, scripted_auto, Effect, Fixture};
use factory_agent::AgentSpec;
use factory_core::AgentFile;
use factory_engine::KbOutcome;

fn files() -> Vec<AgentFile> {
    ["builder", "committer", "reviewer", "kb-refresher"]
        .iter()
        .map(|n| AgentFile {
            name: format!("factory-{n}.md"),
            content: format!("---\nname: factory-{n}\n---\nBODY {n}\n"),
        })
        .collect()
}

fn write_effect() -> Effect {
    Box::new(|spec: &AgentSpec| {
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    })
}

fn codex(mut fx: Fixture) -> Fixture {
    fx.config.agent_cli = Some("codex".to_string());
    fx
}

fn with_kb(mut fx: Fixture) -> Fixture {
    let kb = fx.tmp.path().join("kb").join("rocketapi");
    std::fs::create_dir_all(&kb).expect("mkdir kb");
    fx.config.kb_path = Some(kb.to_string_lossy().into_owned());
    fx
}

#[test]
fn run_agent_prepends_the_agent_file_and_the_codex_preface() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.agent, None);
    let text = spec.agent_prompt.expect("agent text");
    assert!(text.starts_with("You run under Codex."), "{text}");
    assert!(text.contains("BODY builder"), "{text}");
    assert!(
        !text.contains("name: factory-builder"),
        "frontmatter must be stripped"
    );
    assert_eq!(spec.max_turns, None);
    assert_eq!(spec.sandbox.as_deref(), Some("workspace-write"));
}

#[test]
fn committer_message_and_kb_get_the_skill_sentence() {
    let fx = with_kb(codex(committing_fixture()));
    let runner = scripted_auto(vec![write_effect(), reply("No relevant changes")]);
    let e = fx.engine(&runner).with_agent_files(files());
    e.build_stage().expect("build");
    e.kb_refresh_step(None).expect("kb");
    for role in ["committer", "kb"] {
        let spec = runner.last_spec_for(role).expect(role);
        let text = spec.agent_prompt.expect("agent text");
        assert!(
            text.contains("The skill your instructions name is not available"),
            "{role}: {text}"
        );
    }
}

const BAN: &str = "Do not load or invoke skills";
const SENTENCE: &str = "The skill your instructions name is not available";

fn skill_root(fx: &Fixture, rels: &[&str]) -> std::path::PathBuf {
    let root = fx.tmp.path().join("skills");
    for rel in rels {
        let dir = root.join(rel);
        std::fs::create_dir_all(&dir).expect("mkdir skill");
        std::fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n").expect("write skill");
    }
    root
}

fn agent_text(runner: &common::ScriptedRunner, role: &str) -> String {
    runner
        .last_spec_for(role)
        .expect(role)
        .agent_prompt
        .expect("agent text")
}

#[test]
fn builder_with_superpowers_installed_gets_skills_and_the_allowlist() {
    let fx = codex(committing_fixture());
    let root = skill_root(&fx, &["superpowers/tdd", "other"]);
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .with_codex_skill_roots(vec![root.clone()])
        .build_stage()
        .expect("build");
    let other = std::fs::canonicalize(root.join("other/SKILL.md")).expect("canonical");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(
        spec.codex_config,
        [
            "skills.bundled.enabled=false".to_string(),
            format!(
                "skills.config=[{{path=\"{}\",enabled=false}}]",
                other.display()
            )
        ]
    );
    let text = spec.agent_prompt.expect("agent text");
    assert!(!text.contains(BAN), "{text}");
    assert!(text.contains("skill catalog"), "{text}");
}

#[test]
fn builder_sees_the_worktree_skills_folder() {
    let fx = codex(committing_fixture());
    let wt = fx
        .repo
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "demo".into(),
            project: None,
        })
        .expect("worktree");
    let tip = fx.repo.tip(&wt.branch).expect("tip");
    set_job(&fx, |j| {
        j.worktree = Some(wt.path.to_string_lossy().into_owned());
        j.branch = wt.branch.clone();
        j.base = Some(wt.base.clone());
        j.head = Some(tip);
    });
    let skills = wt.path.join(".agents/skills");
    for rel in ["superpowers/x", "other"] {
        std::fs::create_dir_all(skills.join(rel)).expect("mkdir skill");
        std::fs::write(skills.join(rel).join("SKILL.md"), "x\n").expect("write skill");
    }
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let other = std::fs::canonicalize(skills.join("other/SKILL.md")).expect("canonical");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(
        spec.codex_config,
        [
            "skills.bundled.enabled=false".to_string(),
            format!(
                "skills.config=[{{path=\"{}\",enabled=false}}]",
                other.display()
            )
        ]
    );
}

#[test]
fn unmatched_allowlist_has_no_skills_preface() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.codex_config, ["skills.include_instructions=false"]);
    assert!(spec.agent_prompt.expect("agent text").contains(BAN));
}

#[test]
fn reviewer_gets_no_skills() {
    let fx = codex(committing_fixture());
    let root = skill_root(&fx, &["superpowers/tdd"]);
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    fx.engine(&runner)
        .with_agent_files(files())
        .checks_stage()
        .expect("checks");
    let reviewer = scripted_auto(vec![reply("ok\nVERDICT: APPROVE")]);
    let mut list = files();
    list.push(AgentFile {
        name: "factory-code-reviewer.md".to_string(),
        content: "BODY reviewer\n".to_string(),
    });
    fx.engine(&reviewer)
        .with_agent_files(list)
        .with_codex_skill_roots(vec![root])
        .review_stage()
        .expect("review");
    let spec = reviewer.last_spec_for("reviewer").expect("reviewer");
    assert_eq!(spec.codex_config, ["skills.include_instructions=false"]);
}

#[test]
fn committer_keeps_the_skill_sentence_only_when_the_commit_skill_is_missing() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert!(agent_text(&runner, "committer").contains(SENTENCE));

    let fx = codex(committing_fixture());
    let root = skill_root(&fx, &["dev-workflow-skills/1-git-commit"]);
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .with_codex_skill_roots(vec![root])
        .build_stage()
        .expect("build");
    let text = agent_text(&runner, "committer");
    assert!(!text.contains(SENTENCE), "{text}");
    assert!(text.contains("skill catalog"), "{text}");
}

#[test]
fn kb_with_kb_refresh_installed_has_no_skill_sentence() {
    let fx = with_kb(codex(Fixture::new()));
    let root = skill_root(&fx, &["kb-refresh"]);
    let runner = scripted(vec![reply("No relevant changes")]);
    fx.engine(&runner)
        .with_agent_files(files())
        .with_codex_skill_roots(vec![root])
        .kb_refresh_step(None)
        .expect("kb");
    let text = agent_text(&runner, "kb");
    assert!(!text.contains(SENTENCE), "{text}");
}

#[test]
fn network_is_on_only_for_builder_roles() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert!(runner.last_spec_for("builder").expect("builder").network);
    assert!(
        !runner
            .last_spec_for("committer")
            .expect("committer")
            .network
    );
}

#[test]
fn committer_gets_the_git_dir_as_a_writable_root() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("committer").expect("committer");
    assert_eq!(spec.writable_roots, vec![fx.repo.common_git_dir()]);
    assert_eq!(spec.sandbox.as_deref(), Some("workspace-write"));
}

#[test]
fn codex_builder_runs_in_the_project_folder() {
    let mut fx = codex(committing_fixture());
    common::commit_file(&fx.main, "apps/web/index.txt", "web\n");
    fx.repo = factory_git::Repo::open(&fx.main)
        .expect("open")
        .with_scope("web", std::path::Path::new("apps/web"));
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let wt = std::path::PathBuf::from(fx.job().worktree.expect("worktree"));
    let builder = runner.last_spec_for("builder").expect("builder");
    assert_eq!(builder.cwd, wt.join("apps/web"));
    assert_eq!(builder.sandbox.as_deref(), Some("workspace-write"));
    let committer = runner.last_spec_for("committer").expect("committer");
    assert_eq!(committer.cwd, wt);
    assert_eq!(committer.writable_roots, vec![fx.repo.common_git_dir()]);
}

#[test]
fn kb_runs_in_the_kb_folder_under_codex() {
    let fx = with_kb(codex(Fixture::new()));
    let runner = scripted(vec![reply("No relevant changes")]);
    fx.engine(&runner)
        .with_agent_files(files())
        .kb_refresh_step(None)
        .expect("kb");
    let spec = runner.last_spec_for("kb").expect("kb spec");
    assert_eq!(spec.cwd, fx.config.kb_dir().expect("kb dir"));
    assert!(spec
        .agent_prompt
        .expect("agent text")
        .contains("BODY kb-refresher"));
    assert!(spec.writable_roots.is_empty());
}

#[test]
fn codex_runs_skip_the_cache_ttl_variables() {
    let mut fx = codex(committing_fixture());
    fx.config.cache_ttl = Some("5m".to_string());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert!(
        spec.env.iter().all(|(k, _)| k != "FORCE_PROMPT_CACHING_5M"),
        "{:?}",
        spec.env
    );
}

#[test]
fn agent_file_name_missing_from_the_list_fails_before_spawn() {
    let fx = with_kb(codex(Fixture::new()));
    let runner = scripted(vec![reply("No relevant changes")]);
    let list = vec![AgentFile {
        name: "factory-nope.md".into(),
        content: "x".into(),
    }];
    let out = fx
        .engine(&runner)
        .with_agent_files(list)
        .kb_refresh_step(None)
        .expect("kb");
    match out {
        KbOutcome::Failed(m) => assert!(
            m.contains("agent file factory-kb-refresher not found"),
            "{m}"
        ),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(
        runner.last_spec().is_none(),
        "the runner must not be called"
    );
}

#[test]
fn claude_projects_are_untouched() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.agent.as_deref(), Some("factory-builder"));
    assert_eq!(spec.agent_prompt, None);
    assert_eq!(spec.sandbox, None);
    assert!(!spec.network);
    assert!(spec.writable_roots.is_empty());
    assert!(spec.max_turns.is_some());
}

fn set_job(fx: &Fixture, f: impl FnOnce(&mut factory_core::Job)) {
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    f(board
        .jobs
        .iter_mut()
        .find(|j| j.id == "001")
        .expect("job 001"));
    board.save(&fx.board_path).expect("save");
}

fn saved_claude_state(j: &mut factory_core::Job) {
    j.provider = Some("claude".to_string());
    j.builder_id = Some("claude-sess".to_string());
    j.builder_last = Some(factory_core::BuilderLast {
        task: 1,
        id: "claude-sess".to_string(),
        ended_at: 1,
        end_ctx: 10,
    });
}

#[test]
fn a_provider_switch_clears_saved_session_ids_once() {
    let fx = codex(committing_fixture());
    set_job(&fx, saved_claude_state);
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let job = fx.job();
    assert_eq!(job.provider.as_deref(), Some("codex"));
    assert_eq!(job.task_session, None);
    // The fresh builder may save its own new id, never the Claude one.
    assert_ne!(job.builder_id.as_deref(), Some("claude-sess"));
    assert!(job.builder_last.is_none_or(|l| l.id != "claude-sess"));
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.resume, None, "nothing goes to codex exec resume");
}

#[test]
fn provider_switch_clears_task_resume_before_build_task_selects_a_session() {
    let fx = codex(committing_fixture());
    let wt = fx
        .repo
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "demo".into(),
            project: None,
        })
        .expect("worktree");
    let tip = fx.repo.tip(&wt.branch).expect("tip");
    set_job(&fx, |j| {
        j.provider = Some("claude".into());
        j.worktree = Some(wt.path.to_string_lossy().into_owned());
        j.branch = wt.branch.clone();
        j.base = Some(wt.base.clone());
        j.head = Some(tip.clone());
        j.task_session = Some(factory_core::TaskSession {
            task: 1,
            id: "claude-task-session".into(),
            tip,
        });
    });
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(spec.resume, None);
    assert_eq!(fx.job().task_session, None);
    assert_eq!(fx.job().provider.as_deref(), Some("codex"));
}

#[test]
fn provider_switch_clears_legacy_resume_before_findings_are_required() {
    let fx = codex(committing_fixture());
    let wt = fx
        .repo
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "demo".into(),
            project: None,
        })
        .expect("worktree");
    let tip = fx.repo.tip(&wt.branch).expect("tip");
    set_job(&fx, |j| {
        j.provider = Some("claude".into());
        j.builder_id = Some("claude-legacy-session".into());
        j.worktree = Some(wt.path.to_string_lossy().into_owned());
        j.branch = wt.branch.clone();
        j.base = Some(wt.base.clone());
        j.head = Some(tip);
    });
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build after switch");
    assert_eq!(
        runner.last_spec_for("builder").expect("builder").resume,
        None
    );
    assert_eq!(fx.job().provider.as_deref(), Some("codex"));
}

#[test]
fn an_old_job_counts_as_claude() {
    let fx = committing_fixture();
    set_job(&fx, |j| {
        j.provider = None;
        j.builder_id = Some("claude-sess".to_string());
    });
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    assert_eq!(fx.job().provider.as_deref(), Some("claude"));
    let usage = std::fs::read_to_string(fx.job_dir.join("usage.jsonl")).expect("usage");
    assert!(usage.contains("\"provider\":\"claude\""), "{usage}");
}

#[test]
fn fallback_commit_runs_when_a_codex_committer_fails() {
    let fx = codex(committing_fixture());
    let wt = fx
        .repo
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "demo".into(),
            project: None,
        })
        .expect("worktree");
    std::fs::write(wt.path.join("a.txt"), "content\n").expect("write change");
    let nothing = || -> Effect { Box::new(|_: &AgentSpec| "did nothing".to_string()) };
    let runner = scripted(vec![nothing(), nothing()]);
    let request = factory_engine::CommitRequest {
        kind: factory_engine::CommitKind::Task,
        plan_title: "demo plan",
        task_title: "Add the thing.",
        goal: "add the thing",
        floor: &wt.base,
        issues: &[],
        snapshots: &[],
    };
    let out = fx
        .engine(&runner)
        .with_agent_files(files())
        .commit_step(&wt, &request)
        .expect("step");
    assert!(
        matches!(
            out,
            factory_engine::CommitOutcome::Committed { fallback: true, .. }
        ),
        "{out:?}"
    );
}

#[test]
fn provider_sync_runs_once_per_engine() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![]);
    let engine = fx.engine(&runner);
    set_job(&fx, |j| j.provider = None);
    engine.sync_provider().expect("first sync");
    assert_eq!(fx.job().provider.as_deref(), Some("codex"));
    set_job(&fx, |j| {
        j.provider = None;
        j.task_session = Some(factory_core::TaskSession {
            task: 1,
            id: "keep".to_string(),
            tip: "t".to_string(),
        });
    });
    engine.sync_provider().expect("second sync");
    assert!(fx.job().task_session.is_some());
    let fresh = fx.engine(&runner);
    fresh.sync_provider().expect("fresh sync");
    assert_eq!(fx.job().task_session, None);
}

#[test]
fn codex_builders_get_no_simplify_line() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder spec");
    assert!(!spec.prompt.contains("simplify"), "{}", spec.prompt);
}

#[test]
fn a_claude_run_pins_helper_agents_to_its_own_model() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.last_spec_for("builder").expect("builder spec");
    let model = spec.model.clone().expect("model");
    assert!(
        spec.env
            .iter()
            .any(|(k, v)| k == "CLAUDE_CODE_SUBAGENT_MODEL" && *v == model),
        "{:?}",
        spec.env
    );
}

#[test]
fn a_codex_run_sets_no_subagent_model() {
    let fx = codex(committing_fixture());
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner)
        .with_agent_files(files())
        .build_stage()
        .expect("build");
    let spec = runner.last_spec_for("builder").expect("builder spec");
    assert!(
        spec.env
            .iter()
            .all(|(k, _)| k != "CLAUDE_CODE_SUBAGENT_MODEL"),
        "{:?}",
        spec.env
    );
}
