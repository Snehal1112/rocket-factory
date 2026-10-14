use std::path::Path;

use factory_core::{Job, Ledger, SeriesPos, Stage};

use crate::{merge_stage, now_secs, park, parse_series, today, Driver, EngineError, MergeResult, StageOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Merged,
    NeedsHuman(String),
}

/// Adds one job to the board and makes its folder, ledger and plan copy. A job that runs now starts at
/// stage build. A plan that waits for its turn starts at stage spec and moves to build when it runs.
fn add_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    issues: &[u32],
    series: Option<SeriesPos>,
    stage: Stage,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    // Folder and branch names keep a new id from colliding with a leftover of an older job.
    let dirs: Vec<String> = std::fs::read_dir(driver.root().join(".superpowers/sdd"))
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let branches = driver.repo().local_branch_names()?;
    let job = driver.store().update(|board| {
        let ids: Vec<String> = board.jobs.iter().map(|j| j.id.clone()).collect();
        let id = factory_core::next_job_id(&ids, &dirs, &branches);
        let slug = factory_core::slugify(slug);
        let date = today();
        let dir = factory_core::job_dir_name(&date, &id, &slug);
        let now = now_secs();
        let mut job = Job::new(&id, &dir, &slug, &date, feature, now);
        if stage != Stage::Spec {
            job.set_stage(stage, None, false, now)?;
        }
        job.issues = issues.to_vec();
        job.series = series;
        job.model = model.map(str::to_string);
        job.budget_weighted = Some(driver.config().budget_weighted);
        board.jobs.push(job.clone());
        Ok(job)
    })?;
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir)?;
    Ledger::new(dir.join("progress.md"))
        .create(&format!("# SDD ledger, plan: .superpowers/sdd/{}/plan.md", job.dir))?;
    if let Some(pos) = &job.series {
        std::fs::copy(&pos.plan_file, dir.join("plan.md"))?;
    }
    Ok(job)
}

/// The number of leading `#` of a heading line (`## Goal` is 2), or 0 for any other line.
fn heading_level(line: &str) -> usize {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes > 0 && line[hashes..].starts_with(' ') {
        hashes
    } else {
        0
    }
}

/// The text of a section: the lines under the `## <title>` line up to the next heading of level 1 or 2.
/// Deeper headings and lines inside fenced code belong to the section. None when there is no such heading.
fn section_body(spec: &str, title: &str) -> Option<String> {
    let mut in_fence = false;
    let mut in_section = false;
    let mut found = false;
    let mut body: Vec<&str> = Vec::new();
    for line in spec.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            if in_section {
                body.push(line);
            }
            continue;
        }
        let level = if in_fence { 0 } else { heading_level(line) };
        if in_section {
            if level == 1 || level == 2 {
                break;
            }
            body.push(line);
        } else if level == 2 && line[2..].trim().eq_ignore_ascii_case(title) {
            in_section = true;
            found = true;
        }
    }
    found.then(|| body.join("\n"))
}

/// The least words a Goal needs. A one-line stub is not a goal.
const MIN_GOAL_WORDS: usize = 8;

/// Words that mark unfinished text. They are matched as whole upper-case words.
const PLACEHOLDERS: [&str; 3] = ["TBD", "TODO", "FIXME"];

/// The first line (1-based) and word of unfinished text outside fenced code and inline code spans.
fn find_placeholder(spec: &str) -> Option<(usize, &'static str)> {
    let mut in_fence = false;
    for (index, line) in spec.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Text between backticks is quoted code, so only the parts outside count.
        let outside: Vec<&str> = line.split('`').step_by(2).collect();
        for part in outside {
            for word in part.split(|c: char| !c.is_ascii_alphanumeric()) {
                if let Some(found) = PLACEHOLDERS.iter().find(|p| **p == word) {
                    return Some((index + 1, found));
                }
            }
            if part.contains("???") {
                return Some((index + 1, "???"));
            }
        }
    }
    None
}

/// True when a line starts a list item: `- `, `* ` or `1. `.
fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") {
        return true;
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && t[digits..].starts_with(". ")
}

/// A spec must name its goal and its acceptance criteria. The headings are the contract with `/factory`.
/// The goal needs a few words, the criteria need at least one list item, and no placeholder may remain.
fn check_spec(spec: &str) -> Result<(), EngineError> {
    for title in ["Goal", "Acceptance criteria"] {
        let body = match section_body(spec, title) {
            Some(body) if !body.trim().is_empty() => body,
            Some(_) => return Err(EngineError::Msg(format!("spec.md section `## {title}` is empty"))),
            None => return Err(EngineError::Msg(format!("spec.md has no `## {title}` section"))),
        };
        if title == "Goal" && body.split_whitespace().count() < MIN_GOAL_WORDS {
            return Err(EngineError::Msg(format!(
                "spec.md section `## Goal` has fewer than {MIN_GOAL_WORDS} words; say what is built and why"
            )));
        }
        if title == "Acceptance criteria" && !body.lines().any(is_list_item) {
            return Err(EngineError::Msg(
                "spec.md section `## Acceptance criteria` has no list item; list each check as a bullet".to_string(),
            ));
        }
    }
    if let Some((line, word)) = find_placeholder(spec) {
        return Err(EngineError::Msg(format!(
            "spec.md line {line} still has `{word}`; finish it or remove it"
        )));
    }
    Ok(())
}

/// Creates the jobs of a series and returns plan 1's job, which starts at stage build. Every later plan
/// gets its job now too, parked at stage spec, so the board shows the whole series from the start.
pub fn create_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    plan: Option<&Path>,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    create_job_with_spec(driver, slug, feature, plan, None, model)
}

/// Like `create_job`, with the spec at an explicit path (for example a committed file under
/// `docs/superpowers/specs/`). Without `spec_path` a plans folder holds `spec.md` beside the plans.
pub fn create_job_with_spec(
    driver: &Driver,
    slug: &str,
    feature: &str,
    plan: Option<&Path>,
    spec_path: Option<&Path>,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    // A spec with no plans would be skipped without a word, so refuse it.
    if spec_path.is_some() && plan.is_none() {
        return Err(EngineError::Msg("--spec needs --plans".to_string()));
    }
    let Some(start) = plan else {
        return add_job(driver, slug, feature, &factory_core::parse_issues(feature), None, Stage::Build, model);
    };
    // A broken chain fails here, before any job exists.
    let series = parse_series(driver.root(), start)?;
    // An explicit spec path wins. Otherwise a folder holds the spec beside the plans, and a single
    // plan file has no spec to check.
    let spec = spec_path.map(Path::to_path_buf).unwrap_or_else(|| start.join("spec.md"));
    let has_spec = spec_path.is_some() || start.is_dir();
    // The text that passes the check is the text every job gets, so the file is read once.
    let mut spec_text: Option<String> = None;
    if has_spec {
        let text = std::fs::read_to_string(&spec).map_err(|_| {
            if spec_path.is_some() {
                EngineError::Msg(format!("the spec file {} cannot be read", spec.display()))
            } else {
                EngineError::Msg("the plans folder has no spec.md".to_string())
            }
        })?;
        check_spec(&text)?;
        spec_text = Some(text);
    }
    let total = series.plans.len() as u32;
    // A single plan needs no series id. A longer chain shares one id between all its jobs.
    let series_id = if total > 1 {
        let used: Vec<String> = driver
            .store()
            .load()?
            .jobs
            .iter()
            .filter_map(|j| j.series.as_ref().and_then(|s| s.id.clone()))
            .collect();
        Some(factory_core::next_series_id(&used))
    } else {
        None
    };
    let pos = |index: usize| SeriesPos {
        plan_file: series.plans[index].path.to_string_lossy().into_owned(),
        index: index as u32 + 1,
        total,
        id: series_id.clone(),
    };
    // Every job gets a copy of the spec, so each builder and reviewer can read it.
    // Issue numbers come from the feature text only, so a spec that cites an issue never closes it.
    let issues = factory_core::parse_issues(feature);
    let copy_spec = |job: &Job| -> Result<(), EngineError> {
        if let Some(text) = &spec_text {
            std::fs::write(driver.job_dir(job).join("spec.md"), text)?;
        }
        Ok(())
    };
    let first = add_job(driver, slug, feature, &issues, Some(pos(0)), Stage::Build, model)?;
    copy_spec(&first)?;
    for (index, later) in series.plans.iter().enumerate().skip(1) {
        let stem = later.path.file_stem().and_then(|s| s.to_str()).unwrap_or("plan").to_string();
        let later_job = add_job(driver, &stem, feature, &issues, Some(pos(index)), Stage::Spec, model)?;
        copy_spec(&later_job)?;
    }
    Ok(first)
}

/// Drives one job to merged or needs-human. The board stage is the only state, so a crash resumes here.
pub fn run_job(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    let result = run_job_inner(driver, id);
    driver.release_lease(id);
    result
}

fn run_job_inner(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    // Each plan task costs one build call, so the allowance grows with the task count.
    let task_calls = {
        let job = driver.job(id)?;
        let plan = std::fs::read_to_string(driver.job_dir(&job).join("plan.md")).unwrap_or_default();
        crate::tasks::parse_tasks(&plan).len()
    };
    let limit = driver.config().fix_rounds as usize * 4 + 12 + task_calls;
    for _ in 0..limit {
        // A live lease of another driver stops this one here, before any stage work.
        driver.renew_lease(id)?;
        let job = driver.job(id)?;
        match job.stage {
            Stage::Merged => return Ok(RunEnd::Merged),
            Stage::NeedsHuman => return Ok(RunEnd::NeedsHuman(job.reason.unwrap_or_default())),
            Stage::Spec => {
                if !driver.job_dir(&job).join("plan.md").exists() {
                    park(driver, id, "no plan.md in the job folder")?;
                } else {
                    let now = now_secs();
                    driver.store().update_job(id, |j| j.set_stage(Stage::Build, None, false, now))?;
                }
            }
            Stage::Build => {
                driver.build(id)?;
            }
            Stage::Review => {
                // The checks stage starts the fix round itself and parks the job when the rounds are
                // used up, so anything but a pass loops back to the board.
                if driver.checks(id)? != StageOutcome::Advanced(Stage::Review) {
                    continue;
                }
                let outcome = driver.review(id)?;
                // A decision approves after the final checks, starts a fix round, or parks.
                driver.apply(id, &outcome)?;
            }
            Stage::Approve => {
                if let MergeResult::Parked(why) = merge_stage(driver, id)? {
                    return Ok(RunEnd::NeedsHuman(why));
                }
            }
        }
    }
    park(driver, id, "run loop did not settle, see the ledger")?;
    Ok(RunEnd::NeedsHuman("run loop did not settle".into()))
}

/// Runs plan k, then creates and runs plan k+1, one at a time. Stops on needs-human.
pub fn run_series(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    let mut current = id.to_string();
    loop {
        let end = run_job(driver, &current)?;
        if end != RunEnd::Merged {
            return Ok(end);
        }
        let job = driver.job(&current)?;
        let Some(pos) = job.series.clone() else { return Ok(RunEnd::Merged) };
        let series = parse_series(driver.root(), Path::new(&pos.plan_file))?;
        let Some(next) = series.plans[0].next.clone() else { return Ok(RunEnd::Merged) };
        // Resume: the next job may already exist from an earlier run.
        let board = driver.store().load()?;
        let next_name = next.to_string_lossy().into_owned();
        let existing = board
            .jobs
            .iter()
            .find(|j| j.series.as_ref().is_some_and(|s| s.id == pos.id && s.plan_file == next_name && s.index == pos.index + 1));
        current = match existing {
            Some(j) => j.id.clone(),
            None => {
                // A series made before plans got their jobs up front: create this plan's job now.
                let stem = next.file_stem().and_then(|s| s.to_str()).unwrap_or("plan").to_string();
                let next_pos =
                    SeriesPos { plan_file: next_name, index: pos.index + 1, total: pos.total, id: pos.id.clone() };
                add_job(driver, &stem, &job.feature, &job.issues, Some(next_pos), Stage::Build, job.model.as_deref())?
                    .id
            }
        };
    }
}
