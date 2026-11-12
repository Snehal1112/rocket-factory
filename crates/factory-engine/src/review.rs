use std::path::PathBuf;

use factory_checks::ChecksReport;
use factory_core::{
    budget_state, classify_paths, combine_verdicts, open_blockers, parse_verdict, DiffClass,
    LastReview, ParkKind, ReviewState, Stage, Verdict, REVIEW_ROLES,
};

use crate::decide::{decide, risk_summary, DecideInput, Decision};
use crate::ports::{git_err, io_err};
use crate::specs::{ReviewerExtra, SECURITY_AGENT};
use crate::tasks::{plan_outline, review_focus};
use crate::{Engine, EngineError, StageOutcome};

/// Which run a review belongs to. A sync review keeps its own file next to the round review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRun {
    Round,
    Sync,
}

pub struct ReviewOutcome {
    pub verdict: Verdict,
    pub review_path: PathBuf,
    pub decision: Decision,
}

impl Engine<'_> {
    /// The ledger lines the builder path wrote, `Ruling: builder task N...`, without the `Ruling: ` prefix.
    pub(crate) fn builder_notes(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.job_dir.join("progress.md")).unwrap_or_default();
        text.lines()
            .filter(|l| l.starts_with("Ruling: builder task "))
            .filter_map(|l| l.strip_prefix("Ruling: "))
            .map(str::to_string)
            .collect()
    }

    /// Writes the diff files, runs the reviewer once, saves its reply and decides.
    pub fn review_stage(&self) -> Result<ReviewOutcome, EngineError> {
        self.review_stage_as(ReviewRun::Round)
    }

    /// Like `review_stage`, and a sync run saves `review-code-r{round}-sync{n}.md` instead.
    pub fn review_stage_as(&self, run_kind: ReviewRun) -> Result<ReviewOutcome, EngineError> {
        let job = self.job()?;
        let base = job
            .base
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        let head = job
            .head
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no head".into()))?;
        // The reviewer sees only the new work of this round. Risk and size judge the whole job diff.
        let round_base = job.prev_head.clone().unwrap_or_else(|| base.clone());
        // The file names carry both short shas, so the round diff and the whole diff never overwrite each other.
        let round_diff = self
            .repo
            .diff_to_files(&round_base, &head, &self.job_dir)
            .map_err(git_err)?;
        let whole_base = self.range_start(&base, &head)?;
        let whole = self
            .repo
            .diff_to_files(&whole_base, &head, &self.job_dir)
            .map_err(git_err)?;
        let paths = self.job_changed_paths()?;

        let cwd = self.worktree()?;
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let whole_text = std::fs::read_to_string(&whole.diff).ok();
        let no_tests =
            crate::checks_stage::no_tests_added(&paths, whole_text.as_deref(), &self.config);
        let extra = ReviewerExtra {
            focus: review_focus(&plan),
            notes: self.builder_notes(),
            plan_outline: plan_outline(&plan),
            no_tests,
        };
        let class = classify_paths(&paths, &self.config);
        let risky = class == DiffClass::Risky;
        // The first review of a risky job records its risky paths, so a park can name those that fix rounds added.
        let first_risky = self.job_dir.join(crate::park::RISKY_PATHS_FILE);
        if risky && !first_risky.exists() {
            let list: Vec<String> = paths
                .iter()
                .filter(|p| {
                    classify_paths(std::slice::from_ref(*p), &self.config) == DiffClass::Risky
                })
                .cloned()
                .collect();
            // Like the other evidence files, this one is best effort and never stops the review.
            let _ = std::fs::write(&first_risky, list.join("\n") + "\n");
        }
        let round_lines = round_diff.changed_lines();
        let job_model = self.job_model();
        let model = job_model
            .clone()
            .unwrap_or_else(|| self.reviewer_model_for(round_lines, risky).to_string());
        if job_model.is_some() {
            self.ruling(&format!("review model: {model} (job model)"))?;
        } else if model != self.config.reviewer_model {
            let reason = if risky {
                "risky diff".to_string()
            } else {
                format!(
                    "{round_lines} changed lines, over {}",
                    self.config.reviewer_large_over_lines
                )
            };
            self.ruling(&format!("review model: {model} ({reason})"))?;
        }
        let review_path = match run_kind {
            ReviewRun::Round => self.job_dir.join(format!("review-code-r{}.md", job.round)),
            ReviewRun::Sync => {
                let prefix = format!("review-code-r{}-sync", job.round);
                let taken = std::fs::read_dir(&self.job_dir)
                    .map_err(io_err)?
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                    .count();
                self.job_dir.join(format!("{prefix}{}.md", taken + 1))
            }
        };
        let file = review_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // A sync review runs the code reviewer only. A round runs every role the job holds.
        let mut to_run: Vec<&'static str> = Vec::new();
        for role in REVIEW_ROLES {
            let held =
                role == "code" || (run_kind == ReviewRun::Round && job.reviews.contains_key(role));
            if !held {
                continue;
            }
            if role == "security" && security_skippable(&paths, class) {
                self.update(|j| {
                    j.set_review(role, ReviewState::Skipped);
                    Ok(())
                })?;
                self.ruling("security review skipped, docs-only round")?;
            } else {
                to_run.push(role);
            }
        }
        if to_run.contains(&"security") && !self.has_security_agent()? {
            let reason = format!("{SECURITY_AGENT}.md not found");
            self.ruling(&format!("parked, {reason}"))?;
            return Ok(ReviewOutcome {
                verdict: Verdict::Missing,
                review_path,
                decision: Decision::NeedsHuman {
                    kind: ParkKind::Other,
                    reason,
                },
            });
        }

        let multi = to_run.len() > 1;
        let mut parts: Vec<RolePart> = Vec::new();
        let mut verdicts: Vec<Verdict> = Vec::new();
        for role in to_run {
            let spec = if role == "security" {
                self.security_spec_with_model(
                    &cwd,
                    &round_diff.diff,
                    &round_diff.names,
                    job.round,
                    &extra,
                    &model,
                )
            } else {
                self.reviewer_spec_with_model(
                    &cwd,
                    &round_diff.diff,
                    &round_diff.names,
                    job.round,
                    &self.config.reviewer_rules,
                    &extra,
                    &model,
                )
            };
            self.update(|j| {
                j.set_review(role, ReviewState::Running);
                Ok(())
            })?;
            let run = self.run_agent(spec)?;
            if multi {
                let raw = self
                    .job_dir
                    .join(format!("review-{role}-r{}.raw.md", job.round));
                std::fs::write(raw, &run.result_text).map_err(io_err)?;
            }
            let role_verdict = if run.stop_reason == factory_agent::StopReason::Done {
                parse_verdict(&run.result_text)
            } else {
                Verdict::Missing
            };
            let state = if role_verdict == Verdict::Approve {
                ReviewState::Pass
            } else {
                ReviewState::Changes
            };
            self.update(|j| {
                j.set_review(role, state);
                Ok(())
            })?;
            verdicts.push(role_verdict);
            parts.push(RolePart {
                role,
                text: run.result_text,
                verdict: role_verdict,
            });
        }
        let verdict = combine_verdicts(&verdicts);
        let text = match parts.as_slice() {
            [only] => only.text.clone(),
            _ => merge_replies(&parts, verdict),
        };
        std::fs::write(&review_path, &text).map_err(io_err)?;
        let blockers = open_blockers(&text).len() as u32;
        let source = match run_kind {
            ReviewRun::Round => job.fix_source.clone().unwrap_or_else(|| "build".into()),
            ReviewRun::Sync => "sync".into(),
        };
        let label = match run_kind {
            ReviewRun::Round => "review",
            ReviewRun::Sync => "sync review",
        };
        let last = LastReview {
            round: job.round,
            verdict: verdict.as_word().into(),
            blockers,
            file,
            source: source.clone(),
        };
        self.update(|j| {
            j.last_review = Some(last.clone());
            Ok(())
        })?;
        self.ruling(&format!(
            "{label} round {} ({source}) verdict {verdict:?}, {blockers} blockers",
            job.round
        ))?;

        let checks_ok = self.latest_checks_ok()?;
        let input = DecideInput {
            checks_ok,
            verdict: &verdict,
            class,
            risky_summary: risk_summary(&paths, &self.config),
            diff_lines: whole.changed_lines(),
            diff_cap: self.config.diff_cap_lines as usize,
            budget: budget_state(self.job_weighted_tokens(), self.config.budget_weighted),
            round: job.round,
            fix_rounds: self.config.fix_rounds_for(class == DiffClass::Risky),
            rework_base: job.rework_base,
        };
        let decision = decide(&input);
        let choice = match &decision {
            Decision::AutoMerge => "merge",
            Decision::FixRound => "fix",
            Decision::NeedsHuman { .. } => "needs-human",
        };
        tracing::info!(
            event = "decide",
            choice = choice,
            round = job.round,
            verdict = verdict.as_word(),
            risky = risky
        );
        Ok(ReviewOutcome {
            verdict,
            review_path,
            decision,
        })
    }

    /// True when the effective agent files hold the security reviewer. A file tracked only on the
    /// base branch does not count, because it never reaches `agents.json`.
    fn has_security_agent(&self) -> Result<bool, EngineError> {
        let name = format!("{SECURITY_AGENT}.md");
        Ok(self.effective_agent_files()?.iter().any(|f| f.name == name))
    }

    /// True when `checks.md` exists and has no FAIL line.
    fn latest_checks_ok(&self) -> Result<bool, EngineError> {
        let text = std::fs::read_to_string(self.job_dir.join("checks.md")).map_err(io_err)?;
        Ok(text.lines().any(|l| l.starts_with("PASS "))
            && !text.lines().any(|l| l.starts_with("FAIL ")))
    }

    /// Re-runs the mapped checks over the whole job diff on the final head.
    pub fn final_checks(&self) -> Result<ChecksReport, EngineError> {
        self.run_checks_now()
    }

    /// Applies a decision to the board.
    pub fn apply_decision(&self, outcome: &ReviewOutcome) -> Result<StageOutcome, EngineError> {
        match &outcome.decision {
            Decision::NeedsHuman { kind, reason } => {
                // A risky approve still needs the final checks to pass on the exact head.
                if *kind == ParkKind::RiskyDiff {
                    if let Some(outcome) = self.final_gate()? {
                        return Ok(outcome);
                    }
                }
                let parts = self.decision_parts(*kind, reason);
                self.park_with(*kind, reason.clone(), parts)
            }
            Decision::FixRound => {
                let text = std::fs::read_to_string(&outcome.review_path).map_err(io_err)?;
                let findings =
                    format!("The reviewer asked for changes. Fix these findings only.\n\n{text}");
                self.fix_or_park(
                    &findings,
                    "review asked for changes",
                    ParkKind::ChangesRemain,
                    "review reply",
                )
            }
            Decision::AutoMerge => {
                if let Some(outcome) = self.final_gate()? {
                    return Ok(outcome);
                }
                self.set_stage(Stage::Approve, None, false)?;
                self.ruling("auto-merge approved, checks green, reviewer approved")?;
                Ok(StageOutcome::Advanced(Stage::Approve))
            }
        }
    }

    /// Re-runs the checks on the final head and applies the gates.
    /// Returns `Some` when it started a fix round or parked, and `None` when everything passed.
    fn final_gate(&self) -> Result<Option<StageOutcome>, EngineError> {
        let report = self.final_checks()?;
        if !report.all_passed() {
            let findings = format!(
                "The final checks failed. Fix these failures only.\n\n{}",
                report.failure_text()
            );
            return self
                .fix_or_park(
                    &findings,
                    "final checks failed",
                    ParkKind::ChecksFailed,
                    "checks",
                )
                .map(Some);
        }
        let paths = self.job_changed_paths()?;
        if let Some((kind, reason)) = self.gate_checks(&paths)? {
            return self.park_gate(kind, reason, &paths).map(Some);
        }
        let commands: Vec<&str> = report.results.iter().map(|r| r.command.as_str()).collect();
        let list = if commands.is_empty() {
            "none".to_string()
        } else {
            commands.join(", ")
        };
        self.ruling(&format!("checks ran: {list}"))?;
        Ok(None)
    }
}

/// One role's reply with the verdict the driver took from it.
struct RolePart {
    role: &'static str,
    text: String,
    verdict: Verdict,
}

/// True when security review may be skipped: the round changed something, the diff is not risky and
/// every changed path is under `docs/`. Markdown elsewhere can be an agent instruction, so it is never skippable.
fn security_skippable(paths: &[String], class: DiffClass) -> bool {
    class != DiffClass::Risky && !paths.is_empty() && paths.iter().all(|p| p.starts_with("docs/"))
}

/// The review file of several roles: a section per role without its own `VERDICT:` line, then the combined line.
/// The combined line is left out when the verdict is `Missing`.
fn merge_replies(parts: &[RolePart], combined: Verdict) -> String {
    let mut out = String::new();
    for part in parts {
        let mut lines: Vec<&str> = part.text.lines().collect();
        if parse_verdict(&part.text) != Verdict::Missing {
            if let Some(last) = lines.iter().rposition(|l| !l.trim().is_empty()) {
                lines.truncate(last);
            }
        }
        let word = match part.verdict {
            Verdict::Approve => "APPROVE",
            Verdict::Changes => "CHANGES",
            Verdict::Missing => "MISSING",
        };
        out.push_str(&format!("## {} review\n", part.role));
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&format!("Role verdict: {word}\n"));
    }
    match combined {
        Verdict::Approve => out.push_str("\nVERDICT: APPROVE\n"),
        Verdict::Changes => out.push_str("\nVERDICT: CHANGES\n"),
        Verdict::Missing => {}
    }
    out
}

pub fn review_stage(e: &Engine) -> Result<ReviewOutcome, EngineError> {
    e.review_stage()
}

pub fn apply_decision(e: &Engine, outcome: &ReviewOutcome) -> Result<StageOutcome, EngineError> {
    e.apply_decision(outcome)
}

pub fn final_checks(e: &Engine) -> Result<ChecksReport, EngineError> {
    e.final_checks()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skippable(paths: &[&str]) -> bool {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        security_skippable(&paths, DiffClass::Normal)
    }

    #[test]
    fn security_skippable_is_default_deny() {
        for paths in [
            &["docs/a.md"][..],
            &["docs/x/y.txt"],
            &["docs/a.md", "docs/b/c.md"],
        ] {
            assert!(skippable(paths), "{paths:?}");
        }
        for paths in [
            &["a.md"][..],
            &["README.md", "docs/a.md"],
            &["crates/factory-core/assets/global-command.md"],
            &["crates/factory-core/presets/legacy/rust-factory-builder.md"],
            &["crates/factory-core/presets/rust/commit-context.md"],
            &["Dockerfile"],
            &["docs/a.md", "Cargo.toml"],
            &[".claude/agents/x.md"],
            &["presets/rust/agents/x.md"],
            &["CLAUDE.md"],
            &["sub/AGENTS.md"],
            &[],
        ] {
            assert!(!skippable(paths), "{paths:?}");
        }
    }

    #[test]
    fn a_risky_diff_is_never_skippable() {
        assert!(!security_skippable(
            &["docs/a.md".to_string()],
            DiffClass::Risky
        ));
    }

    #[test]
    fn merge_replies_drops_the_role_verdict_lines_and_leaves_out_a_missing_verdict() {
        let parts = [
            RolePart {
                role: "code",
                text: "a\nVERDICT: APPROVE\n".to_string(),
                verdict: Verdict::Approve,
            },
            RolePart {
                role: "security",
                text: "no verdict here".to_string(),
                verdict: Verdict::Missing,
            },
        ];
        let text = merge_replies(&parts, Verdict::Missing);
        assert_eq!(
            text,
            "## code review\na\nRole verdict: APPROVE\n## security review\nno verdict here\nRole verdict: MISSING\n"
        );
        assert_eq!(parse_verdict(&text), Verdict::Missing);
    }
}
