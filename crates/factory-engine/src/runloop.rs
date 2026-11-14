use std::path::Path;

use factory_core::{Job, Ledger, ParkKind, ParkPart, SeriesPos, Stage};

use crate::{
    merge_stage, now_secs, park_with_parts, parse_series, today, Driver, EngineError, MergeResult,
    StageOutcome,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Merged,
    NeedsHuman(String),
}

/// Adds one job to the board and makes its folder, ledger and plan copy. A job that runs now starts at
/// stage build. A plan that waits for its turn starts at stage spec and moves to build when it runs.
pub(crate) fn add_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    issues: &[u32],
    series: Option<SeriesPos>,
    stage: Stage,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    add_job_marked(driver, slug, feature, issues, series, stage, model, None)
}

/// Like `add_job`, and sets the intake marker in the same board write that adds the job.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_job_marked(
    driver: &Driver,
    slug: &str,
    feature: &str,
    issues: &[u32],
    series: Option<SeriesPos>,
    stage: Stage,
    model: Option<&str>,
    intake_issue: Option<u32>,
) -> Result<Job, EngineError> {
    // Folder and branch names keep a new id from colliding with a leftover of an older job.
    let dirs: Vec<String> = std::fs::read_dir(&driver.layout().jobs_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    // Only this project's own branches count, and `next_job_id` reads the `factory/` ones.
    let branches = driver.repo().own_branch_names()?;
    let job = driver.store().update(|board| {
        let ids: Vec<String> = board.jobs.iter().map(|j| j.id.clone()).collect();
        let id = factory_core::next_job_id(&ids, &dirs, &branches);
        let slug = factory_core::slugify(slug);
        let date = today();
        let dir = factory_core::job_dir_name(&date, &id, &slug);
        let now = now_secs();
        let mut job = Job::new(&id, &dir, &slug, &date, feature, now);
        if stage != Stage::Spec {
            crate::ports::set_stage_logged(&mut job, stage, None, false, now)?;
        }
        job.issues = issues.to_vec();
        job.series = series;
        job.model = model.map(str::to_string);
        job.intake_issue = intake_issue;
        job.budget_weighted = Some(driver.config().budget_weighted);
        job.set_reviewers(&driver.config().reviewers);
        board.jobs.push(job.clone());
        Ok(job)
    })?;
    let mut job = job;
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir)?;
    Ledger::new(dir.join("progress.md")).create(&format!(
        "# SDD ledger, plan: {}",
        dir.join("plan.md").display()
    ))?;
    if let Some(pos) = &job.series {
        std::fs::copy(&pos.plan_file, dir.join("plan.md"))?;
        // The estimate is best effort: no history or a failed read gives no estimate, never an error.
        let bytes = std::fs::metadata(dir.join("plan.md"))
            .map(|m| m.len())
            .unwrap_or(0);
        if let Some(estimate) = crate::costs::estimate_for_plan(driver, bytes) {
            let stored = estimate.clone();
            // A failed write is ignored, so the job simply has no estimate.
            let written = driver.store().update_job(&job.id, |j| {
                j.estimate = Some(stored);
                Ok(())
            });
            if written.is_ok() {
                job.estimate = Some(estimate);
            }
        }
    }
    let series_label = job
        .series
        .as_ref()
        .and_then(|s| s.id.clone())
        .unwrap_or_default();
    tracing::info!(
        event = "job.created",
        job_id = job.id.as_str(),
        slug = job.slug.as_str(),
        series = series_label.as_str(),
        msg = format!("job {} created", job.id).as_str()
    );
    Ok(job)
}

/// The span that carries the job id on every log line of a run.
pub fn job_span(job_id: &str) -> tracing::Span {
    tracing::info_span!("job", job_id)
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
pub(crate) fn section_body(spec: &str, title: &str) -> Option<String> {
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
pub(crate) fn find_placeholder(spec: &str) -> Option<(usize, &'static str)> {
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
pub(crate) fn is_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") {
        return true;
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && t[digits..].starts_with(". ")
}

/// The warning for a spec in which the user answered no decision.
pub(crate) const NO_USER_DECISION: &str = "no decision in this spec was answered by the user";

/// How many list items in the `## Decisions` section say `Decided by: user`. No section gives 0.
pub(crate) fn user_decisions(spec: &str) -> usize {
    section_body(spec, "Decisions")
        .map(|body| {
            body.lines()
                .filter(|l| is_list_item(l) && l.contains("Decided by: user"))
                .count()
        })
        .unwrap_or(0)
}

/// Notes a failed side step of `new`. The jobs exist already, so a failure must not fail the call.
/// It goes to stderr and, when the ledger takes it, to a ruling line of the job.
fn note_failure(driver: &Driver, job: &Job, what: &str, slug: &str, err: &EngineError) {
    tracing::error!(event = "step.error", step = "note_failure", msg = %format!("{what} error for {slug}: {err}"));
    let line = format!("{what} error: {err}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if let Err(ledger_err) = Ledger::new(driver.job_dir(job).join("progress.md")).ruling(&line) {
        tracing::error!(event = "step.error", step = "note_failure_ledger", msg = %format!("{what} ledger note error for {slug}: {ledger_err}"));
    }
}

/// Removes the draft card of this slug, if there is one. A failure, such as a draft that a concurrent
/// cancel removed first, is reported and never changes the result.
pub(crate) fn adopt_draft(driver: &Driver, slug: &str, job: &Job) {
    adopt_draft_with(driver, slug, job, |d, s| {
        crate::remove_draft(d, s).map(|_| ())
    });
}

fn adopt_draft_with(
    driver: &Driver,
    slug: &str,
    job: &Job,
    remove: impl FnOnce(&Driver, &str) -> Result<(), EngineError>,
) {
    let result = match crate::find_draft(driver, slug) {
        Ok(Some(_)) => remove(driver, slug),
        Ok(None) => Ok(()),
        Err(err) => Err(err),
    };
    if let Err(err) = result {
        note_failure(driver, job, "draft adoption", slug, &err);
    }
}

/// Adds a warning to the job. Warnings are joined with `; `, so a later check appends with this
/// function instead of assigning. A failed write is reported and the returned job still has the warning.
pub(crate) fn add_warning(driver: &Driver, slug: &str, job: Job, text: &str) -> Job {
    let join = |job: &mut Job| {
        job.spec_warning = Some(match job.spec_warning.take() {
            Some(old) => format!("{old}; {text}"),
            None => text.to_string(),
        });
    };
    let stored = driver.store().update_job(&job.id, |j| {
        join(j);
        Ok(j.clone())
    });
    match stored {
        Ok(updated) => updated,
        Err(err) => {
            note_failure(driver, &job, "spec warning", slug, &err.into());
            let mut job = job;
            join(&mut job);
            job
        }
    }
}

/// A spec must name its goal and its acceptance criteria. The headings are the contract with `/factory`.
/// The goal needs a few words, the criteria need at least one list item, and no placeholder may remain.
fn check_spec(spec: &str, name: &str) -> Result<(), EngineError> {
    for title in ["Goal", "Acceptance criteria"] {
        let body = match section_body(spec, title) {
            Some(body) if !body.trim().is_empty() => body,
            Some(_) => {
                return Err(EngineError::Msg(format!(
                    "{name} section `## {title}` is empty"
                )))
            }
            None => {
                return Err(EngineError::Msg(format!(
                    "{name} has no `## {title}` section"
                )))
            }
        };
        if title == "Goal" && body.split_whitespace().count() < MIN_GOAL_WORDS {
            return Err(EngineError::Msg(format!(
                "{name} section `## Goal` has fewer than {MIN_GOAL_WORDS} words; say what is built and why"
            )));
        }
        if title == "Acceptance criteria" && !body.lines().any(is_list_item) {
            return Err(EngineError::Msg(format!(
                "{name} section `## Acceptance criteria` has no list item; list each check as a bullet"
            )));
        }
    }
    if let Some((line, word)) = find_placeholder(spec) {
        return Err(EngineError::Msg(format!(
            "{name} line {line} still has `{word}`; finish it or remove it"
        )));
    }
    Ok(())
}

/// A series that passed every check that must hold before a job exists.
pub(crate) struct Validated {
    pub series: crate::Series,
    /// Plan lint findings that do not refuse the call.
    pub plan_warnings: Vec<String>,
    /// The checked spec text, when there is a spec.
    pub spec_text: Option<String>,
    /// The user's reason, when `--accept-spec` overrode a refusal.
    pub accepted: Option<String>,
}

/// Records that the user accepted a refused spec, in the job's ledger and its warning.
pub(crate) fn record_accepted(driver: &Driver, slug: &str, job: Job, reason: &str) -> Job {
    let text = format!("spec accepted by the user: {reason}");
    if let Err(err) = driver.ledger(&job).ruling(&text) {
        note_failure(driver, &job, "spec acceptance", slug, &err.into());
    }
    add_warning(driver, slug, job, &text)
}

/// Parses the chain, lints every plan and checks the spec, all before any job exists.
/// `parse` reads the chain, so the caller picks the cap on tasks per plan and maps its errors.
pub(crate) fn validate_series(
    driver: &Driver,
    start: &Path,
    spec_path: Option<&Path>,
    accept_spec: Option<&str>,
    intake_tree: Option<&Path>,
    parse: impl Fn(&Path, &Path) -> Result<crate::Series, EngineError>,
) -> Result<Validated, EngineError> {
    // Every failure here comes from the drafted files, so it is a validation error.
    validate_series_inner(driver, start, spec_path, accept_spec, intake_tree, parse).map_err(|e| {
        match e {
            EngineError::SpecRefused(r) => EngineError::SpecRefused(r),
            other => EngineError::Invalid(other.to_string()),
        }
    })
}

fn validate_series_inner(
    driver: &Driver,
    start: &Path,
    spec_path: Option<&Path>,
    accept_spec: Option<&str>,
    intake_tree: Option<&Path>,
    parse: impl Fn(&Path, &Path) -> Result<crate::Series, EngineError>,
) -> Result<Validated, EngineError> {
    // Nobody can answer a refusal of an unattended draft, so a reason to accept has no effect there.
    let accept_spec = accept_spec.filter(|_| intake_tree.is_none());
    // Absolute paths keep `Next plan:` lookups and the stored plan files right from any directory.
    let absolute = |path: &Path| {
        std::fs::canonicalize(path)
            .map_err(|e| EngineError::Msg(format!("cannot read {}: {e}", path.display())))
    };
    let start_buf = absolute(start)?;
    let start = start_buf.as_path();
    let spec_buf = spec_path.map(absolute).transpose()?;
    let spec_path = spec_buf.as_deref();
    // A broken chain fails here, before any job exists.
    let series = parse(driver.root(), start)?;
    // A placeholder in any plan refuses the call before a job exists. Other findings are warnings.
    let mut plan_warnings: Vec<String> = Vec::new();
    for plan in &series.plans {
        let name = plan
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("plan")
            .to_string();
        let text = std::fs::read_to_string(&plan.path)
            .map_err(|e| EngineError::Msg(format!("cannot read {}: {e}", plan.path.display())))?;
        let lint = crate::planlint::lint_plan(&text, &name);
        if let Some(reason) = lint.refuse {
            return Err(EngineError::Msg(reason));
        }
        plan_warnings.extend(lint.warnings);
    }
    // An explicit spec path wins. Otherwise a folder holds the spec beside the plans, and a single
    // plan file has no spec to check.
    let spec = spec_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| start.join("spec.md"));
    let has_spec = spec_path.is_some() || start.is_dir();
    // The text that passes the check is the text every job gets, so the file is read once.
    let mut spec_text: Option<String> = None;
    let mut accepted: Option<String> = None;
    if has_spec {
        let text = std::fs::read_to_string(&spec).map_err(|_| {
            if spec_path.is_some() {
                EngineError::Msg(format!("the spec file {} cannot be read", spec.display()))
            } else {
                EngineError::Msg("the plans folder has no spec.md".to_string())
            }
        })?;
        check_spec(&text, &spec.display().to_string())?;
        if let Some(tree) = intake_tree {
            crate::spec_check::check_intake_decisions(&text, tree).map_err(|message| {
                EngineError::SpecRefused(crate::spec_check::SpecRefusal::UnsourcedDecision(message))
            })?;
        }
        // A spec that touches risky paths needs a decision the user answered.
        let mut plan_texts: Vec<String> = Vec::new();
        for plan in &series.plans {
            plan_texts.push(std::fs::read_to_string(&plan.path).map_err(|e| {
                EngineError::Msg(format!("cannot read {}: {e}", plan.path.display()))
            })?);
        }
        // A refusal stops here, unless the user gave a reason to go on. A risky spec the user did not
        // answer is refused before the agent runs, so an accepted one never pays for a review.
        let plan_paths: Vec<std::path::PathBuf> =
            series.plans.iter().map(|p| p.path.clone()).collect();
        // An intake spec has sourced decisions instead, which are stricter than the risky-path rule.
        let risky = match intake_tree {
            Some(_) => None,
            None => {
                crate::spec_check::risky_without_user_decision(&text, &plan_texts, driver.config())
            }
        };
        let refusal = match risky {
            Some(reason) => Some(crate::spec_check::SpecRefusal::NoUserDecision(reason)),
            None => crate::spec_check::review_spec_with(
                driver,
                &spec,
                &plan_paths,
                accept_spec.is_some(),
                intake_tree.is_some(),
            )
            .err(),
        };
        if let Some(refusal) = refusal {
            match accept_spec {
                Some(reason)
                    if matches!(refusal, crate::spec_check::SpecRefusal::NoUserDecision(_)) =>
                {
                    accepted = Some(format!("{reason} (no agent review ran)"));
                }
                Some(reason) => accepted = Some(reason.to_string()),
                None => return Err(EngineError::SpecRefused(refusal)),
            }
        }
        spec_text = Some(text);
    }
    Ok(Validated {
        series,
        plan_warnings,
        spec_text,
        accepted,
    })
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
    create_job_with_spec_accepting(driver, slug, feature, plan, spec_path, model, None, None)
}

/// Like `create_job_with_spec`. A reason in `accept_spec` turns a spec refusal into a pass and is recorded.
/// With `intake_tree`, the spec was drafted unattended: its decisions must cite files of that tree, and
/// `accept_spec` has no effect.
#[allow(clippy::too_many_arguments)]
pub fn create_job_with_spec_accepting(
    driver: &Driver,
    slug: &str,
    feature: &str,
    plan: Option<&Path>,
    spec_path: Option<&Path>,
    model: Option<&str>,
    accept_spec: Option<&str>,
    intake_tree: Option<&Path>,
) -> Result<Job, EngineError> {
    // A spec with no plans would be skipped without a word, so refuse it.
    if spec_path.is_some() && plan.is_none() {
        return Err(EngineError::Msg("--spec needs --plans".to_string()));
    }
    let Some(start) = plan else {
        let job = add_job(
            driver,
            slug,
            feature,
            &factory_core::parse_issues(feature),
            None,
            Stage::Build,
            model,
        )?;
        adopt_draft(driver, slug, &job);
        return Ok(job);
    };
    let Validated {
        series,
        plan_warnings,
        spec_text,
        accepted,
    } = validate_series(
        driver,
        start,
        spec_path,
        accept_spec,
        intake_tree,
        |root, start| {
            crate::parse_series_with(root, start, Some(crate::series::MAX_TASKS_PER_PLAN))
                .map_err(EngineError::from)
        },
    )?;
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
    let first = add_job(
        driver,
        slug,
        feature,
        &issues,
        Some(pos(0)),
        Stage::Build,
        model,
    )?;
    copy_spec(&first)?;
    for (index, later) in series.plans.iter().enumerate().skip(1) {
        let stem = later
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("plan")
            .to_string();
        let later_job = add_job(
            driver,
            &stem,
            feature,
            &issues,
            Some(pos(index)),
            Stage::Spec,
            model,
        )?;
        copy_spec(&later_job)?;
    }
    adopt_draft(driver, slug, &first);
    // The job is made either way. The warning only tells the reader how little the user decided.
    let mut first = first;
    if intake_tree.is_none()
        && spec_text
            .as_deref()
            .is_some_and(|text| user_decisions(text) == 0)
    {
        first = add_warning(driver, slug, first, NO_USER_DECISION);
    }
    if let Some(reason) = &accepted {
        first = record_accepted(driver, slug, first, reason);
    }
    for warning in &plan_warnings {
        first = add_warning(driver, slug, first, warning);
    }
    Ok(first)
}

/// Parks a job that has no plan, naming the file that was expected.
fn park_missing_plan(driver: &Driver, job: &Job) -> Result<(), EngineError> {
    let text = format!("expected {}", driver.job_dir(job).join("plan.md").display());
    let parts = vec![ParkPart::new("Missing plan", "driver", &text)];
    park_with_parts(
        driver,
        &job.id,
        "no plan.md in the job folder",
        ParkKind::Other,
        parts,
    )
}

/// Parks a job whose run loop used up its steps, with the end of the ledger as evidence.
fn park_unsettled(driver: &Driver, id: &str) -> Result<(), EngineError> {
    let ledger = driver
        .job(id)
        .ok()
        .and_then(|job| std::fs::read_to_string(driver.job_dir(&job).join("progress.md")).ok())
        .unwrap_or_default();
    let lines: Vec<&str> = ledger.lines().collect();
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    // A blank tail adds no part, so the card shows why the run stopped instead.
    let parts = if tail.trim().is_empty() {
        Vec::new()
    } else {
        vec![ParkPart::new("Ledger", "progress.md", &tail)]
    };
    park_with_parts(
        driver,
        id,
        "run loop did not settle, see the ledger",
        ParkKind::Other,
        parts,
    )
}

/// Frees the job lease when `run_job` ends, also while a panic unwinds.
struct LeaseGuard<'a> {
    driver: &'a Driver,
    id: &'a str,
}

impl Drop for LeaseGuard<'_> {
    fn drop(&mut self) {
        self.driver.release_lease(self.id);
    }
}

/// Drives one job to merged or needs-human. The board stage is the only state, so a crash resumes here.
pub fn run_job(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    // The project guard keeps the project lease renewed for the whole run and drops after the job lease guard.
    let guard = driver.hold_project(id)?;
    let _lease = LeaseGuard { driver, id };
    let span = job_span(id);
    span.in_scope(|| run_job_inner(driver, id, &guard))
}

fn run_job_inner(
    driver: &Driver,
    id: &str,
    guard: &crate::ProjectGuard<'_>,
) -> Result<RunEnd, EngineError> {
    // Each plan task costs one build call, so the allowance grows with the task count.
    let task_calls = {
        let job = driver.job(id)?;
        let plan =
            std::fs::read_to_string(driver.job_dir(&job).join("plan.md")).unwrap_or_default();
        crate::tasks::parse_tasks(&plan).len()
    };
    let cfg = driver.config();
    let rounds = cfg.fix_rounds.max(cfg.fix_rounds_for(true));
    let limit = rounds as usize * 4 + 12 + task_calls;
    for _ in 0..limit {
        if guard.lost() {
            return Err(EngineError::Msg("project is busy: another owner holds the project lease (the lease was taken over), this run stops".into()));
        }
        // A live lease of another driver stops this one here, before any stage work.
        driver.renew_lease(id)?;
        let job = driver.job(id)?;
        let _stage_span = tracing::info_span!("stage", stage = job.stage.as_str()).entered();
        match job.stage {
            Stage::Merged => return Ok(RunEnd::Merged),
            Stage::NeedsHuman => return Ok(RunEnd::NeedsHuman(job.reason.unwrap_or_default())),
            Stage::Spec => {
                if !driver.job_dir(&job).join("plan.md").exists() {
                    park_missing_plan(driver, &job)?;
                } else {
                    let now = now_secs();
                    driver.store().update_job(id, |j| {
                        crate::ports::set_stage_logged(j, Stage::Build, None, false, now)
                    })?;
                }
            }
            Stage::Build => {
                // A job made without plans has no plan.md until a human places it. Park before any agent call.
                if !driver.job_dir(&job).join("plan.md").exists() {
                    park_missing_plan(driver, &job)?;
                    continue;
                }
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
    park_unsettled(driver, id)?;
    Ok(RunEnd::NeedsHuman("run loop did not settle".into()))
}

/// Runs plan k, then creates and runs plan k+1, one at a time. Stops on needs-human.
pub fn run_series(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    // One guard for the whole series, so no other driver gets in between two plans.
    let _project = driver.hold_project(id)?;
    let mut current = id.to_string();
    loop {
        let end = run_job(driver, &current)?;
        if end != RunEnd::Merged {
            return Ok(end);
        }
        let job = driver.job(&current)?;
        let Some(pos) = job.series.clone() else {
            return Ok(RunEnd::Merged);
        };
        let series = parse_series(driver.root(), Path::new(&pos.plan_file))?;
        let Some(next) = series.plans[0].next.clone() else {
            return Ok(RunEnd::Merged);
        };
        // Resume: the next job may already exist from an earlier run.
        let board = driver.store().load()?;
        let next_name = next.to_string_lossy().into_owned();
        let existing = board.jobs.iter().find(|j| {
            j.series.as_ref().is_some_and(|s| {
                s.id == pos.id && s.plan_file == next_name && s.index == pos.index + 1
            })
        });
        current = match existing {
            Some(j) => j.id.clone(),
            None => {
                // A series made before plans got their jobs up front: create this plan's job now.
                let stem = next
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("plan")
                    .to_string();
                let next_pos = SeriesPos {
                    plan_file: next_name,
                    index: pos.index + 1,
                    total: pos.total,
                    id: pos.id.clone(),
                };
                // An intake job keeps its marker, so the tracker gate covers the late job too.
                let created = add_job_marked(
                    driver,
                    &stem,
                    &job.feature,
                    &job.issues,
                    Some(next_pos),
                    Stage::Build,
                    job.model.as_deref(),
                    job.intake_issue,
                )?;
                created.id
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec_check::SpecRefusal;
    use factory_agent::{AgentRunner, FakeRunner};
    use factory_core::{Config, Layout};
    use std::sync::Arc;

    fn driver(tmp: &tempfile::TempDir) -> Driver {
        driver_with(tmp, Config::default())
    }

    fn driver_with(tmp: &tempfile::TempDir, config: Config) -> Driver {
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).expect("mkdir");
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        let repo = git2::Repository::init_opts(&root, &opts).expect("init");
        let sig = git2::Signature::now("t", "t@example.com").expect("sig");
        let tree = repo
            .find_tree(repo.index().expect("index").write_tree().expect("tree"))
            .expect("find");
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("commit");
        let runner: Arc<dyn AgentRunner> = Arc::new(FakeRunner::new(Vec::new()));
        Driver::new(
            Layout::in_repo(&root),
            config,
            Box::new(move |_| runner.clone()),
        )
        .expect("driver")
    }

    const SOURCED_SPEC: &str = "# Spec\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n## Acceptance criteria\n\n- It works.\n\n## Decisions\n\n- Use the notes. Source: notes.md:1. Decided by: call\n";

    /// A plans folder with a risky plan and the given spec, and a tree that holds `notes.md`.
    fn risky_series(
        tmp: &tempfile::TempDir,
        spec: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let plans = tmp.path().join("plans");
        std::fs::create_dir_all(&plans).expect("plans");
        std::fs::write(plans.join("spec.md"), spec).expect("spec");
        std::fs::write(
            plans.join("plan-01-a.md"),
            "# Plan\n\n### Task 1: t\n\n**Files:**\n- Modify: `src-tauri/src/lib.rs`\n\nNext plan: none\n",
        )
        .expect("plan");
        let tree = tmp.path().join("tree");
        std::fs::create_dir_all(&tree).expect("tree");
        std::fs::write(tree.join("notes.md"), "note\n").expect("notes");
        (plans, tree)
    }

    fn risky_config() -> Config {
        Config {
            risk_paths: vec!["src-tauri/**".to_string()],
            spec_review: false,
            ..Config::default()
        }
    }

    fn validate(
        d: &Driver,
        plans: &Path,
        accept: Option<&str>,
        tree: Option<&Path>,
    ) -> Result<Validated, EngineError> {
        validate_series(d, plans, None, accept, tree, |root, start| {
            crate::parse_series_with(root, start, None).map_err(EngineError::from)
        })
    }

    #[test]
    fn validate_series_in_intake_mode_skips_the_risky_user_refusal() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver_with(&tmp, risky_config());
        let (plans, tree) = risky_series(&tmp, SOURCED_SPEC);
        validate(&d, &plans, None, Some(&tree)).expect("an intake spec passes");
        let err = validate(&d, &plans, None, None)
            .err()
            .expect("a normal risky spec without a user decision is refused");
        assert!(
            matches!(
                err,
                EngineError::SpecRefused(SpecRefusal::NoUserDecision(_))
            ),
            "{err}"
        );
    }

    #[test]
    fn validate_series_in_intake_mode_refuses_an_unsourced_spec() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver_with(&tmp, risky_config());
        let unsourced = SOURCED_SPEC.replace(" Source: notes.md:1.", "");
        let (plans, tree) = risky_series(&tmp, &unsourced);
        for accept in [None, Some("reason")] {
            let err = validate(&d, &plans, accept, Some(&tree))
                .err()
                .expect("an unsourced spec is refused");
            assert!(
                matches!(
                    err,
                    EngineError::SpecRefused(SpecRefusal::UnsourcedDecision(_))
                ),
                "{err}"
            );
        }
    }

    #[test]
    fn add_job_marked_writes_the_marker_with_the_job() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let marked =
            add_job_marked(&d, "demo", "f", &[], None, Stage::Build, None, Some(7)).expect("job");
        assert_eq!(marked.intake_issue, Some(7));
        assert_eq!(d.job(&marked.id).expect("reload").intake_issue, Some(7));
        let plain = add_job(&d, "plain", "f", &[], None, Stage::Build, None).expect("job");
        assert_eq!(d.job(&plain.id).expect("reload").intake_issue, None);
    }

    #[test]
    fn validate_series_reports_a_spec_without_a_goal_as_invalid() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let plans = tmp.path().join("plans");
        std::fs::create_dir_all(&plans).expect("plans");
        std::fs::write(
            plans.join("spec.md"),
            "# Spec\n\n## Acceptance criteria\n\n- It works.\n",
        )
        .expect("spec");
        std::fs::write(
            plans.join("plan-01-a.md"),
            "# Plan\n\n### Task 1: t\n\nNext plan: none\n",
        )
        .expect("plan");
        let err = validate_series(&d, &plans, None, None, None, |root, start| {
            crate::parse_series_with(root, start, None).map_err(EngineError::from)
        })
        .err()
        .expect("a spec without a goal is refused");
        assert!(
            matches!(&err, EngineError::Invalid(m) if m.contains("has no `## Goal` section")),
            "{err}"
        );
    }

    #[test]
    fn unsettled_park_keeps_the_last_twenty_ledger_lines() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let job = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
        let ledger: String = (1..=25).map(|n| format!("line {n}\n")).collect();
        std::fs::write(d.job_dir(&job).join("progress.md"), ledger).expect("ledger");
        park_unsettled(&d, &job.id).expect("park");
        let job = d.job(&job.id).expect("job");
        let rec = factory_core::ParkRecord::read(&d.job_dir(&job)).expect("park.json");
        assert_eq!(rec.reason, "run loop did not settle, see the ledger");
        assert_eq!(rec.parts.len(), 1);
        assert_eq!(rec.parts[0].title, "Ledger");
        assert_eq!(rec.parts[0].source, "progress.md");
        let lines: Vec<&str> = rec.parts[0].text.lines().collect();
        assert_eq!(lines.len(), 20);
        assert_eq!(lines[0], "line 6");
        assert_eq!(lines[19], "line 25");
    }

    #[test]
    fn a_blank_ledger_leaves_out_the_ledger_part() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let job = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
        std::fs::write(d.job_dir(&job).join("progress.md"), "\n  \n\n").expect("ledger");
        park_unsettled(&d, &job.id).expect("park");
        let job = d.job(&job.id).expect("job");
        let rec = factory_core::ParkRecord::read(&d.job_dir(&job)).expect("park.json");
        assert_eq!(rec.parts.len(), 1);
        assert_eq!(rec.parts[0].title, "Why it stopped");
        assert_eq!(rec.parts[0].text, "run loop did not settle, see the ledger");
    }

    #[test]
    fn an_unreadable_ledger_leaves_out_the_ledger_part() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let job = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
        let ledger = d.job_dir(&job).join("progress.md");
        std::fs::remove_file(&ledger).expect("remove ledger");
        std::fs::create_dir_all(&ledger).expect("ledger dir");
        // The ruling write fails on the directory, but the record is already written.
        let _ = park_unsettled(&d, &job.id);
        let job = d.job(&job.id).expect("job");
        let rec = factory_core::ParkRecord::read(&d.job_dir(&job)).expect("park.json");
        assert_eq!(rec.parts.len(), 1);
        assert_eq!(rec.parts[0].title, "Why it stopped");
        assert_eq!(rec.parts[0].text, "run loop did not settle, see the ledger");
    }

    #[test]
    fn note_failure_logs_step_error_with_the_slug() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        crate::start_draft(&d, "demo", "f", "2026-10-18", 1.0).expect("draft");
        let job = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
        let (_, lines) = factory_telemetry::capture("info", || {
            adopt_draft_with(&d, "demo", &job, |_, _| {
                Err(EngineError::Msg("no draft named demo".into()))
            });
        });
        let hit: Vec<_> = lines
            .iter()
            .filter(|l| l["level"] == "error" && l["step"] == "note_failure")
            .collect();
        assert_eq!(hit.len(), 1, "{lines:?}");
        assert!(
            hit[0]["msg"].as_str().expect("msg").contains("demo"),
            "{}",
            hit[0]
        );
    }

    #[test]
    fn a_failed_draft_removal_is_reported_and_keeps_the_one_job() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        crate::start_draft(&d, "demo", "f", "2026-10-18", 1.0).expect("draft");
        let job = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
        adopt_draft_with(&d, "demo", &job, |_, _| {
            Err(EngineError::Msg("no draft named demo".into()))
        });
        let board = d.store().load().expect("board");
        assert_eq!(board.jobs.len(), 1);
        assert_eq!(board.drafts.len(), 1, "the failed removal leaves the card");
        let ledger = std::fs::read_to_string(d.job_dir(&job).join("progress.md")).expect("ledger");
        assert!(
            ledger.contains("Ruling: draft adoption error: no draft named demo"),
            "{ledger}"
        );
    }

    #[test]
    fn a_failed_warning_write_still_returns_the_warning() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let mut ghost = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
        ghost.id = "999".to_string();
        let warned = add_warning(&d, "demo", ghost, "first");
        assert_eq!(warned.spec_warning.as_deref(), Some("first"));
        let real = d.store().load().expect("board").jobs.remove(0);
        let warned = add_warning(&d, "demo", add_warning(&d, "demo", real, "one"), "two");
        assert_eq!(warned.spec_warning.as_deref(), Some("one; two"));
        assert_eq!(
            d.job(&warned.id).expect("job").spec_warning.as_deref(),
            Some("one; two")
        );
    }

    #[test]
    fn user_decisions_counts_only_user_entries() {
        let spec = "# S\n\n## Decisions\n\n- Queue. Decided by: user\n- Retry. Decided by: call\n\
                    1. Naming. Decided by: user\n\n## Other\n\n- Late. Decided by: user\n";
        assert_eq!(user_decisions(spec), 2);
    }

    #[test]
    fn a_spec_without_a_decisions_section_counts_zero() {
        assert_eq!(user_decisions("# S\n\n## Goal\n\n- Decided by: user\n"), 0);
        assert_eq!(
            user_decisions("## Decisions\n\nDecided by: user, but not a list item.\n"),
            0
        );
    }
}
