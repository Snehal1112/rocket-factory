use factory_core::{Board, Config, Job, ParkRecord, ReviewState, Stage};
use factory_engine::ProjectLeaseStatus;

/// How many lines of the first evidence part `factory status` prints.
pub const PARK_EXCERPT_LINES: usize = 12;

/// The block under the table for one parked job: reason, excerpt and next commands.
pub fn render_park_details(job: &Job, record: Option<&ParkRecord>) -> String {
    let reason = job
        .reason
        .clone()
        .unwrap_or_else(|| "needs a human".to_string());
    let mut out = format!("{} needs a human: {}\n", job.id, reason);
    let Some(rec) = record else {
        return out;
    };
    if let Some(part) = rec.parts.first() {
        out.push_str(&format!("  {} ({})\n", part.title, part.source));
        for line in part.text.lines().take(PARK_EXCERPT_LINES) {
            out.push_str(&format!("    {line}\n"));
        }
        let total = part.text.lines().count();
        if total > PARK_EXCERPT_LINES {
            out.push_str(&format!(
                "    ... {} more lines in park.json\n",
                total - PARK_EXCERPT_LINES
            ));
        }
    }
    for command in &rec.next {
        out.push_str(&format!("  next: {command}\n"));
    }
    out
}

/// Park blocks for every parked job that was not cancelled, or an empty string when there are none.
pub fn render_parked(board: &Board, read: impl Fn(&Job) -> Option<ParkRecord>) -> String {
    let mut out = String::new();
    for job in board
        .jobs
        .iter()
        .filter(|j| j.stage == Stage::NeedsHuman && !j.cancelled)
    {
        if out.is_empty() {
            out.push('\n');
        }
        out.push_str(&render_park_details(job, read(job).as_ref()));
    }
    out
}

/// Short token counts: `850`, `12.3K`, `1.2M`.
pub(crate) fn human(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn review_word(state: ReviewState) -> &'static str {
    match state {
        ReviewState::Pending => "pending",
        ReviewState::Running => "running",
        ReviewState::Pass => "pass",
        ReviewState::Changes => "changes",
        ReviewState::Skipped => "skipped",
    }
}

/// The reviewer states as `code:pass` pairs, joined by spaces.
fn reviews_text(job: &Job) -> String {
    let pairs: Vec<String> = job
        .reviews
        .iter()
        .map(|(k, v)| format!("{k}:{}", review_word(*v)))
        .collect();
    if pairs.is_empty() {
        "-".to_string()
    } else {
        pairs.join(" ")
    }
}

fn stage_name(stage: Stage) -> &'static str {
    stage.as_str()
}

/// The first line of `factory status`: who holds the project.
pub fn render_project_line(status: &ProjectLeaseStatus) -> String {
    match status {
        ProjectLeaseStatus::Free => "project: free\n".to_string(),
        ProjectLeaseStatus::Held {
            owner,
            job,
            secs_left,
        } => {
            format!("project: busy, job {job} (owner {owner}, {secs_left} s left)\n")
        }
        ProjectLeaseStatus::Stale { owner, job } => {
            format!("project: stale lease from {owner} (job {job}), parked\n")
        }
        ProjectLeaseStatus::Unreadable(reason) => {
            format!("project: lease file unreadable: {reason}\n")
        }
    }
}

pub fn render_status(board: &Board, cfg: &Config) -> String {
    let mut out = if board.jobs.is_empty() {
        "no jobs\n".to_string()
    } else {
        render_jobs(board, cfg)
    };
    if !board.drafts.is_empty() {
        out.push_str("drafts:\n");
        for draft in &board.drafts {
            out.push_str(&format!(
                "  {:<28} {:<14} {}\n",
                draft.slug, draft.stage, draft.feature
            ));
        }
    }
    out
}

fn render_jobs(board: &Board, cfg: &Config) -> String {
    let mut out = String::from("ID   STAGE        ROUND  REVIEWS         TOKENS    SERIES  SLUG\n");
    for job in &board.jobs {
        let series = job
            .series
            .as_ref()
            .map_or("-".to_string(), |s| format!("{}/{}", s.index, s.total));
        let weighted = job.tokens.weighted();
        let pct = (weighted * 100)
            .checked_div(cfg.budget_weighted)
            .unwrap_or(0);
        out.push_str(&format!(
            "{:<4} {:<12} {:<6} {:<15} {:<9} {:<7} {}\n",
            job.id,
            stage_name(job.stage),
            job.round,
            reviews_text(job),
            format!("{} ({pct}%)", human(weighted)),
            series,
            job.slug,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::{ParkKind, ParkPart};

    #[test]
    fn status_shows_stage_round_reviews_tokens_and_series_position() {
        let mut board = factory_core::Board::default();
        let mut job =
            factory_core::Job::new("002", "d-002-x", "pagination", "2026-10-13", "f", 0.0);
        job.set_stage(factory_core::Stage::Review, None, false, 1.0)
            .expect("stage");
        job.round = 1;
        job.series = Some(factory_core::SeriesPos {
            plan_file: "p".into(),
            index: 2,
            total: 5,
            id: None,
        });
        board.jobs.push(job);
        let out = render_status(&board, &factory_core::Config::default());
        let line = out.lines().nth(1).expect("row");
        for needle in ["002", "review", "1", "2/5", "pagination"] {
            assert!(line.contains(needle), "missing {needle} in {line}");
        }
        assert_eq!(
            render_status(
                &factory_core::Board::default(),
                &factory_core::Config::default()
            ),
            "no jobs\n"
        );
    }

    #[test]
    fn token_counts_are_short() {
        assert_eq!(human(850), "850");
        assert_eq!(human(12_300), "12.3K");
        assert_eq!(human(1_200_000), "1.2M");
    }

    fn parked_job(id: &str, reason: &str) -> factory_core::Job {
        let mut job = factory_core::Job::new(id, "d", "slug", "2026-10-16", "feature", 1.0);
        job.set_stage(Stage::Build, None, false, 2.0)
            .expect("build");
        job.set_stage(Stage::NeedsHuman, Some(reason), false, 3.0)
            .expect("park");
        job
    }

    fn record(id: &str, lines: usize) -> ParkRecord {
        let text = (1..=lines)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        ParkRecord::new(
            ParkKind::ChecksFailed,
            "checks failed after 2 fix rounds",
            id,
            3,
            10,
            vec![ParkPart::new("Failing checks", "checks.md", &text)],
        )
    }

    #[test]
    fn a_parked_job_shows_reason_excerpt_and_next_commands() {
        let job = parked_job("012", "checks failed after 2 fix rounds");
        let text = render_park_details(&job, Some(&record("012", 3)));
        assert!(
            text.contains("012 needs a human: checks failed after 2 fix rounds"),
            "{text}"
        );
        assert!(text.contains("Failing checks (checks.md)"), "{text}");
        assert!(
            text.contains("    line 1") && text.contains("    line 3"),
            "{text}"
        );
        assert!(text.contains("  next: factory rework 012 <note>"), "{text}");
        assert!(!text.contains("more lines"), "{text}");
    }

    #[test]
    fn a_long_excerpt_is_cut_and_says_how_much_is_left() {
        let job = parked_job("012", "why");
        let text = render_park_details(&job, Some(&record("012", 20)));
        assert!(text.contains("    line 12"), "{text}");
        assert!(!text.contains("line 13"), "{text}");
        assert!(text.contains("8 more lines in park.json"), "{text}");
    }

    #[test]
    fn a_parked_job_without_a_record_still_shows_its_reason() {
        let job = parked_job("012", "cancelled: not needed");
        let text = render_park_details(&job, None);
        assert_eq!(text, "012 needs a human: cancelled: not needed\n");
    }

    #[test]
    fn render_parked_lists_only_parked_jobs_that_were_not_cancelled() {
        let mut board = factory_core::Board::default();
        board.jobs.push(parked_job("001", "first"));
        let mut cancelled = parked_job("002", "cancelled: x");
        cancelled.cancelled = true;
        board.jobs.push(cancelled);
        board.jobs.push(factory_core::Job::new(
            "003",
            "d",
            "s",
            "2026-10-16",
            "f",
            1.0,
        ));
        let text = render_parked(&board, |j| Some(record(&j.id, 1)));
        assert!(text.contains("001 needs a human: first"), "{text}");
        assert!(!text.contains("002") && !text.contains("003"), "{text}");
    }

    #[test]
    fn render_parked_of_a_board_with_nothing_parked_is_empty() {
        assert_eq!(render_parked(&factory_core::Board::default(), |_| None), "");
    }
}
