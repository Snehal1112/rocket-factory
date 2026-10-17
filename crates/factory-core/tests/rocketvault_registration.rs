//! The RocketVault registration must resolve cleanly, with its own risk paths, checks and rules.

use factory_core::{preset, resolve, BaseTipFiles, CheckRule, Home, Resolved};
use tempfile::TempDir;

const REGISTRATION: &str = include_str!("fixtures/rocketvault-project.toml");
const NAME: &str = "rocketvault";

/// Globs that must stay risky: the preset's own list plus RocketVault's sensitive areas.
const RISK: &[&str] = &[
    "go.mod",
    "go.sum",
    "**/*auth*",
    "**/*secret*",
    "**/*credential*",
    "**/Dockerfile",
    ".github/**",
    "internal/db/migrations/**",
    "internal/crypto/**",
    "internal/signing/**",
    "internal/services/auth/**",
    "internal/services/authorization/**",
    "internal/services/oauth2/**",
    "internal/middleware/**",
    "internal/keycache/**",
    "model/scope.go",
    "cmd/vaultcli/**",
    "Caddyfile",
    "Caddyfile.local",
    "docker-compose.yml",
    "docker-entrypoint.sh",
    "fly.toml",
    "railway.json",
    "release.sh",
    "install.sh",
    "build.sh",
    ".golangci.yml",
    "scripts/hooks/**",
];

/// A base branch tip with no committed files.
struct EmptyTip;

impl BaseTipFiles for EmptyTip {
    fn read_file(&self, _rel: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn list_dir(&self, _rel: &str) -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }
}

fn resolved() -> (TempDir, Resolved) {
    let t = TempDir::new().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    preset::seed(&home.presets_dir()).expect("seed presets");
    let dir = home.project_dir(NAME);
    std::fs::create_dir_all(&dir).expect("mkdir project");
    std::fs::write(dir.join("project.toml"), REGISTRATION).expect("write project.toml");
    let r = resolve(&home, NAME, &EmptyTip).expect("resolve");
    (t, r)
}

fn rule<'a>(checks: &'a [CheckRule], paths: &[&str]) -> &'a CheckRule {
    checks
        .iter()
        .find(|c| c.paths == paths)
        .unwrap_or_else(|| panic!("no check rule for {paths:?}"))
}

#[test]
fn registration_does_not_fail_closed() {
    let (_t, r) = resolved();
    assert!(r.problems.is_empty(), "problems: {:?}", r.problems);
    assert!(!r.config.degraded);
    assert_ne!(r.config.risk_paths, vec!["**".to_string()]);
}

#[test]
fn base_branch_and_tickets_are_set() {
    let (_t, r) = resolved();
    assert_eq!(r.config.base_branch, "v-4.0.0");
    assert!(r.config.tickets_on_merge);
}

#[test]
fn every_risk_path_is_listed() {
    let (_t, r) = resolved();
    for glob in RISK {
        assert!(r.config.risk_paths.iter().any(|p| p == glob), "missing risk path {glob}");
    }
}

#[test]
fn checks_have_exactly_the_planned_commands() {
    let (_t, r) = resolved();
    let checks = &r.config.checks;
    assert_eq!(checks.len(), 3, "{checks:?}");
    let go = rule(checks, &["**/*.go", "go.mod", "go.sum"]);
    assert_eq!(go.commands, ["go build ./...", "go vet ./...", "go test ./..."]);
    for dir in ["web", "journeybook"] {
        let glob = format!("{dir}/**");
        let c = rule(checks, &[glob.as_str()]);
        let want = [
            format!("bun install --cwd {dir} --frozen-lockfile"),
            format!("bun run --cwd {dir} typecheck"),
            format!("bun run --cwd {dir} lint"),
        ];
        assert_eq!(c.commands, want);
    }
    for c in checks {
        for cmd in &c.commands {
            for bad in ["--workspace", "-race", "format"] {
                assert!(!cmd.contains(bad), "{bad} in {cmd}");
            }
            // With the flag before `run`, bun prints its usage and exits 0, so the check would always pass.
            assert!(!cmd.starts_with("bun --cwd"), "bun needs `run --cwd <dir>`: {cmd}");
        }
    }
}

#[test]
fn read_first_and_rules_name_the_vault_conventions() {
    let (_t, r) = resolved();
    let c = r.config;
    assert_eq!(c.read_first, ["CLAUDE.md", ".claude/known-bugs.md", ".claude/multi-vault.md"]);
    let builder = c.builder_rules.join("\n");
    for needle in ["RequireDataAction", "NewVaultScope", "bun"] {
        assert!(builder.contains(needle), "builder rules miss {needle}");
    }
    let reviewer = c.reviewer_rules.join("\n");
    for needle in ["RequireDataAction", "NewVaultScope"] {
        assert!(reviewer.contains(needle), "reviewer rules miss {needle}");
    }
    assert!(!c.check_hints.is_empty());
}
