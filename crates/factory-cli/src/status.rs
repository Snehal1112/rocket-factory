use factory_core::{Board, Config, Job, ReviewState, Stage};

/// Short token counts: `850`, `12.3K`, `1.2M`.
fn human(n: u64) -> String {
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
    let pairs: Vec<String> = job.reviews.iter().map(|(k, v)| format!("{k}:{}", review_word(*v))).collect();
    if pairs.is_empty() {
        "-".to_string()
    } else {
        pairs.join(" ")
    }
}

fn stage_name(stage: Stage) -> &'static str {
    stage.as_str()
}

pub fn render_status(board: &Board, cfg: &Config) -> String {
    if board.jobs.is_empty() {
        return "no jobs\n".to_string();
    }
    let mut out = String::from("ID   STAGE        ROUND  REVIEWS         TOKENS    SERIES  SLUG\n");
    for job in &board.jobs {
        let series = job.series.as_ref().map_or("-".to_string(), |s| format!("{}/{}", s.index, s.total));
        let weighted = job.tokens.weighted();
        let pct = if cfg.budget_weighted > 0 { weighted * 100 / cfg.budget_weighted } else { 0 };
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

    #[test]
    fn status_shows_stage_round_reviews_tokens_and_series_position() {
        let mut board = factory_core::Board::default();
        let mut job = factory_core::Job::new("002", "d-002-x", "pagination", "2026-10-13", "f", 0.0);
        job.set_stage(factory_core::Stage::Review, None, false, 1.0).expect("stage");
        job.round = 1;
        job.series = Some(factory_core::SeriesPos { plan_file: "p".into(), index: 2, total: 5, id: None });
        board.jobs.push(job);
        let out = render_status(&board, &factory_core::Config::default());
        let line = out.lines().nth(1).expect("row");
        for needle in ["002", "review", "1", "2/5", "pagination"] {
            assert!(line.contains(needle), "missing {needle} in {line}");
        }
        assert_eq!(render_status(&factory_core::Board::default(), &factory_core::Config::default()), "no jobs\n");
    }

    #[test]
    fn token_counts_are_short() {
        assert_eq!(human(850), "850");
        assert_eq!(human(12_300), "12.3K");
        assert_eq!(human(1_200_000), "1.2M");
    }
}
