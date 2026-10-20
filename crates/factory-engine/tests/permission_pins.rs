mod common;

use std::path::Path;

use common::*;
use factory_agent::{AgentSpec, Caps};
use factory_core::Config;
use factory_engine::ReviewerExtra;

const GOLDEN: &str = "reviewer: -p --output-format stream-json --verbose --strict-mcp-config --model sonnet --allowedTools Read,Grep,Glob --tools Read,Grep,Glob --agent factory-code-reviewer --max-turns 40\ncommitter: -p --output-format stream-json --verbose --strict-mcp-config --model sonnet --tools Bash,Read,Skill --agent factory-committer --max-turns 40\nmessage: -p --output-format stream-json --verbose --strict-mcp-config --model sonnet --allowedTools Read,Skill --tools Read,Skill --agent factory-committer --max-turns 40\nkb: -p --output-format stream-json --verbose --strict-mcp-config --model sonnet --permission-mode dontAsk --allowedTools Read,Grep,Glob,Skill,Agent,Edit(//kb/**),Write(//kb/**),Bash(git log:*),Bash(git diff:*),Bash(git show:*),Bash(git rev-parse:*),Bash(git merge-base:*),Bash(git status:*),Bash(git -C /repo log:*),Bash(git -C /repo diff:*),Bash(git -C /repo show:*),Bash(git -C /repo rev-parse:*),Bash(git -C /repo merge-base:*),Bash(git -C /repo status:*),Bash(date:*),Bash(ls:*) --tools Read,Grep,Glob,Edit,Write,Skill,Agent,Bash --add-dir /kb --agent factory-kb-refresher --max-turns 80";

fn change_effect(file: &'static str, lines: usize) -> Effect {
    Box::new(move |spec| {
        let body: String = (0..lines).map(|n| format!("line {n}\n")).collect();
        std::fs::write(spec.cwd.join(file), body).expect("write change");
        "done".to_string()
    })
}

fn line(role: &str, spec: &AgentSpec) -> String {
    format!("{role}: {}", spec.args(&Caps::default()).join(" "))
}

#[test]
fn reviewer_committer_message_and_kb_arguments_are_unchanged() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let committer = runner.last_spec_for("committer").expect("committer spec");
    let reviewer = fx.engine(&runner).reviewer_spec(
        &fx.main,
        &fx.job_dir.join("d.diff"),
        &fx.job_dir.join("n.txt"),
        1,
        &[],
        &ReviewerExtra::default(),
    );
    let message = factory_engine::message_spec("001", &Config::default(), Path::new("/w"), "p");
    let kb = factory_engine::kb_spec(
        "001",
        &Config::default(),
        Path::new("/repo"),
        Path::new("/kb"),
        "p",
    );
    let actual = [
        line("reviewer", &reviewer),
        line("committer", &committer),
        line("message", &message),
        line("kb", &kb),
    ]
    .join("\n");
    assert_eq!(actual, GOLDEN);
}
