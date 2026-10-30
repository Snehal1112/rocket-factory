//! Live marker probe: types, the prompt, the answer parser and the expected channels.
use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

/// Skills the harness itself ships and a probe never counts as extra. Filled after the first live run.
pub const KNOWN_BUILTIN_SKILLS: &[&str] = &[];

/// The agent CLI a probe runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Claude,
    Codex,
}

impl Harness {
    pub fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        }
    }

    /// The harness the config selects.
    pub fn from_config(config: &Config) -> Harness {
        if config.agent_cli() == "codex" {
            Harness::Codex
        } else {
            Harness::Claude
        }
    }
}

impl FromStr for Harness {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "claude" => Ok(Harness::Claude),
            "codex" => Ok(Harness::Codex),
            other => Err(format!(
                "unknown harness `{other}`, expected claude or codex"
            )),
        }
    }
}

/// A factory agent role a probe covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeRole {
    Builder,
    Reviewer,
    SpecReviewer,
    Committer,
    Kb,
}

impl ProbeRole {
    pub const ALL: [ProbeRole; 5] = [
        ProbeRole::Builder,
        ProbeRole::Reviewer,
        ProbeRole::SpecReviewer,
        ProbeRole::Committer,
        ProbeRole::Kb,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ProbeRole::Builder => "builder",
            ProbeRole::Reviewer => "reviewer",
            ProbeRole::SpecReviewer => "spec-reviewer",
            ProbeRole::Committer => "committer",
            ProbeRole::Kb => "kb",
        }
    }

    /// The role string `Config::plugins_for` takes.
    pub fn config_role(self) -> &'static str {
        match self {
            ProbeRole::SpecReviewer => "spec-reviewer",
            other => other.as_str(),
        }
    }
}

impl FromStr for ProbeRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        ProbeRole::ALL
            .into_iter()
            .find(|r| r.as_str() == s)
            .ok_or_else(|| {
                let names: Vec<&str> = ProbeRole::ALL.iter().map(|r| r.as_str()).collect();
                format!("unknown role `{s}`, expected one of {}", names.join(", "))
            })
    }
}

/// One thing that may reach an agent's context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    pub expected: bool,
    pub present: bool,
    pub detail: String,
}

/// The result for one role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleProbe {
    pub role: ProbeRole,
    pub channels: Vec<Channel>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
}

impl RoleProbe {
    /// True when every expected channel is present.
    pub fn ok(&self) -> bool {
        self.channels.iter().all(|c| !c.expected || c.present)
    }
}

/// The result for a whole probe run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeReport {
    pub harness: Harness,
    pub cli_version: String,
    pub roles: Vec<RoleProbe>,
    pub transcript_dir: PathBuf,
}

impl ProbeReport {
    /// True when every role is ok.
    pub fn ok(&self) -> bool {
        self.roles.iter().all(RoleProbe::ok)
    }
}

/// What the agent said about its own context.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProbeAnswer {
    pub marker_seen: bool,
    pub bootstrap: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
}

/// The one-turn prompt. It names only the marker prefix, never the full token.
pub fn probe_prompt(marker: &str) -> String {
    let prefix = marker.split('-').next().unwrap_or(marker);
    format!(
        "PROBE\n\
         Do not use any tool. Answer from the context you were given, in exactly four lines.\n\
         MARKER: the full token that starts with `{prefix}-` in your instructions, or NONE.\n\
         BOOTSTRAP: a short quote of any text that starts with `You have superpowers`, or NONE.\n\
         TOOLS: the names of the tools you can call, comma separated, or NONE.\n\
         SKILLS: the names of the skills you can see, comma separated, or NONE.\n"
    )
}

fn list_value(value: &str) -> Vec<String> {
    if value.eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Reads the four answer lines. `marker_seen` needs the exact marker.
pub fn parse_probe_answer(text: &str, marker: &str) -> ProbeAnswer {
    let mut answer = ProbeAnswer::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("MARKER:") {
            answer.marker_seen = v.trim().trim_matches('`') == marker;
        } else if let Some(v) = line.strip_prefix("BOOTSTRAP:") {
            let v = v.trim();
            answer.bootstrap =
                (!v.is_empty() && !v.eq_ignore_ascii_case("none")).then(|| v.to_string());
        } else if let Some(v) = line.strip_prefix("TOOLS:") {
            answer.tools = list_value(v.trim());
        } else if let Some(v) = line.strip_prefix("SKILLS:") {
            answer.skills = list_value(v.trim());
        }
    }
    answer
}

/// The allowlist entries whose skills a role may see on this harness.
pub fn allowed_skill_entries(role: ProbeRole, harness: Harness, config: &Config) -> Vec<String> {
    match (role, harness) {
        (ProbeRole::Reviewer | ProbeRole::SpecReviewer, Harness::Claude)
        | (ProbeRole::SpecReviewer, Harness::Codex) => Vec::new(),
        _ => config.skill_allow_for(role.config_role()),
    }
}

fn entry_name(entry: &str) -> &str {
    entry.split('@').next().unwrap_or(entry)
}

/// A skill matches an entry by its group (before `:`) or its whole name.
fn skill_matches(skill: &str, entry: &str) -> bool {
    let name = entry_name(entry);
    skill.split(':').next() == Some(name) || skill == name
}

/// The channels a role must show on a harness.
pub fn expected_channels(role: ProbeRole, harness: Harness, config: &Config) -> Vec<&'static str> {
    let entries = allowed_skill_entries(role, harness, config);
    let mut out = vec!["agent_text"];
    if !entries.is_empty() {
        out.push("skills");
    }
    if harness == Harness::Claude && entries.iter().any(|e| entry_name(e) == "superpowers") {
        out.push("bootstrap");
    }
    let strict = match harness {
        Harness::Codex => true,
        Harness::Claude => matches!(role, ProbeRole::Reviewer | ProbeRole::SpecReviewer),
    };
    if strict {
        out.push("no_extra_skills");
    }
    out
}

/// Builds the four channels of a role from the agent's answer.
pub fn evaluate(
    role: ProbeRole,
    harness: Harness,
    config: &Config,
    answer: &ProbeAnswer,
) -> RoleProbe {
    let expected = expected_channels(role, harness, config);
    let entries = allowed_skill_entries(role, harness, config);
    let missing: Vec<&String> = entries
        .iter()
        .filter(|e| !answer.skills.iter().any(|s| skill_matches(s, e)))
        .collect();
    let extra: Vec<&String> = answer
        .skills
        .iter()
        .filter(|s| !KNOWN_BUILTIN_SKILLS.contains(&s.as_str()))
        .filter(|s| !entries.iter().any(|e| skill_matches(s, e)))
        .collect();
    let join = |v: &[&String]| v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
    let bootstrap_seen = answer
        .bootstrap
        .as_deref()
        .is_some_and(|b| b.contains("You have superpowers"));
    let channel = |name: &str, present: bool, detail: String| Channel {
        name: name.to_string(),
        expected: expected.contains(&name),
        present,
        detail,
    };
    let channels = vec![
        channel(
            "agent_text",
            answer.marker_seen,
            if answer.marker_seen {
                "marker echoed".into()
            } else {
                "marker not seen".into()
            },
        ),
        channel(
            "skills",
            missing.is_empty(),
            if missing.is_empty() {
                format!("{} skills listed", answer.skills.len())
            } else {
                format!("missing: {}", join(&missing))
            },
        ),
        channel(
            "bootstrap",
            bootstrap_seen,
            answer
                .bootstrap
                .clone()
                .unwrap_or_else(|| "no bootstrap quote".into()),
        ),
        channel(
            "no_extra_skills",
            extra.is_empty(),
            if extra.is_empty() {
                "no extra skills".into()
            } else {
                format!("extra: {}", join(&extra))
            },
        ),
    ];
    RoleProbe {
        role,
        channels,
        tools: answer.tools.clone(),
        skills: answer.skills.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn builder_on_claude_expects_bootstrap_and_skills() {
        assert_eq!(
            expected_channels(ProbeRole::Builder, Harness::Claude, &cfg()),
            ["agent_text", "skills", "bootstrap"]
        );
    }

    #[test]
    fn committer_on_claude_expects_skills_only() {
        assert_eq!(
            expected_channels(ProbeRole::Committer, Harness::Claude, &cfg()),
            ["agent_text", "skills"]
        );
    }

    #[test]
    fn kb_expects_kb_refresh() {
        assert_eq!(
            allowed_skill_entries(ProbeRole::Kb, Harness::Claude, &cfg()),
            ["kb-refresh"]
        );
        assert_eq!(
            expected_channels(ProbeRole::Kb, Harness::Claude, &cfg()),
            ["agent_text", "skills"]
        );
    }

    #[test]
    fn reviewer_expects_no_skills_on_claude() {
        let want = ["agent_text", "no_extra_skills"];
        for role in [ProbeRole::Reviewer, ProbeRole::SpecReviewer] {
            assert_eq!(expected_channels(role, Harness::Claude, &cfg()), want);
        }
        let set = Config::parse("reviewer_plugins = [\"superpowers\"]").expect("parse");
        assert_eq!(
            expected_channels(ProbeRole::Reviewer, Harness::Claude, &set),
            want
        );
        let answer = ProbeAnswer {
            marker_seen: true,
            skills: vec!["superpowers:brainstorming".into()],
            ..ProbeAnswer::default()
        };
        let probe = evaluate(ProbeRole::Reviewer, Harness::Claude, &set, &answer);
        assert!(!probe.ok());
    }

    #[test]
    fn codex_expects_no_extra_skills_for_every_role() {
        for role in ProbeRole::ALL {
            assert!(
                expected_channels(role, Harness::Codex, &cfg()).contains(&"no_extra_skills"),
                "{role:?}"
            );
        }
    }

    #[test]
    fn codex_never_expects_bootstrap() {
        for role in ProbeRole::ALL {
            assert!(!expected_channels(role, Harness::Codex, &cfg()).contains(&"bootstrap"));
        }
    }

    #[test]
    fn prompt_starts_with_probe_and_asks_four_lines() {
        let p = probe_prompt("FPROBE-abc123");
        assert_eq!(p.lines().next(), Some("PROBE"));
        for key in ["MARKER:", "BOOTSTRAP:", "TOOLS:", "SKILLS:", "NONE"] {
            assert!(p.contains(key), "{key}");
        }
    }

    #[test]
    fn prompt_does_not_leak_the_marker() {
        assert!(!probe_prompt("FPROBE-abc123").contains("abc123"));
    }

    #[test]
    fn parse_reads_four_lines() {
        let text = "Sure.\nMARKER: FPROBE-abc\nBOOTSTRAP: You have superpowers.\nTOOLS: Read, Bash\nSKILLS: superpowers:tdd, imagegen\n";
        let a = parse_probe_answer(text, "FPROBE-abc");
        assert!(a.marker_seen);
        assert_eq!(a.bootstrap.as_deref(), Some("You have superpowers."));
        assert_eq!(a.tools, ["Read", "Bash"]);
        assert_eq!(a.skills, ["superpowers:tdd", "imagegen"]);
    }

    #[test]
    fn none_values_parse_as_empty() {
        let a = parse_probe_answer(
            "MARKER: NONE\nBOOTSTRAP: NONE\nTOOLS: NONE\nSKILLS: none",
            "FPROBE-abc",
        );
        assert!(!a.marker_seen);
        assert_eq!(a.bootstrap, None);
        assert!(a.tools.is_empty());
        assert!(a.skills.is_empty());
    }

    #[test]
    fn wrong_marker_is_not_seen() {
        assert!(!parse_probe_answer("MARKER: FPROBE-old", "FPROBE-abc").marker_seen);
        assert!(!parse_probe_answer("MARKER: FPROBE-abcd", "FPROBE-abc").marker_seen);
    }

    #[test]
    fn skills_with_group_match_the_entry() {
        let answer = ProbeAnswer {
            marker_seen: true,
            bootstrap: Some("You have superpowers".into()),
            skills: vec!["superpowers:brainstorming".into(), "imagegen".into()],
            ..ProbeAnswer::default()
        };
        let probe = evaluate(ProbeRole::Builder, Harness::Claude, &cfg(), &answer);
        let by = |n: &str| {
            probe
                .channels
                .iter()
                .find(|c| c.name == n)
                .expect("channel")
        };
        assert!(by("skills").present);
        assert!(by("bootstrap").present);
        assert!(!by("no_extra_skills").present);
        assert!(by("no_extra_skills").detail.contains("imagegen"));
        assert!(
            probe.ok(),
            "extra skills are not expected to pass on a Claude builder"
        );
    }

    #[test]
    fn evaluate_marks_missing_expected_channels() {
        let probe = evaluate(
            ProbeRole::Builder,
            Harness::Claude,
            &cfg(),
            &ProbeAnswer::default(),
        );
        assert!(!probe.ok());
        let missing: Vec<&str> = probe
            .channels
            .iter()
            .filter(|c| c.expected && !c.present)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(missing, ["agent_text", "skills", "bootstrap"]);
    }

    #[test]
    fn report_round_trips_json() {
        let report = ProbeReport {
            harness: Harness::Codex,
            cli_version: "1.2.3".into(),
            roles: vec![evaluate(
                ProbeRole::SpecReviewer,
                Harness::Codex,
                &cfg(),
                &ProbeAnswer::default(),
            )],
            transcript_dir: PathBuf::from("/t"),
        };
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains("\"codex\""));
        assert!(json.contains("\"spec-reviewer\""));
        let back: ProbeReport = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, report);
        assert!(!back.ok());
    }

    #[test]
    fn role_parses_from_str_and_rejects_unknown() {
        for role in ProbeRole::ALL {
            assert_eq!(role.as_str().parse::<ProbeRole>(), Ok(role));
        }
        let err = "nobody".parse::<ProbeRole>().expect_err("unknown role");
        assert!(err.contains("nobody"), "{err}");
    }
}
