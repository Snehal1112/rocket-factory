//! Draft cards: features that are still being specified and have no job yet.

use factory_core::{slugify, Draft, DRAFT_STAGES};

use crate::{Driver, EngineError, StoreError};

fn spec_path(driver: &Driver, date: &str, slug: &str) -> String {
    let layout = driver.layout();
    let full = layout.draft_spec_file(date, slug);
    full.strip_prefix(&layout.repo)
        .unwrap_or(&full)
        .to_string_lossy()
        .into_owned()
}

/// Adds a draft at stage `interviewing`. The slug is slugified and must not be taken by another draft.
pub fn start_draft(
    driver: &Driver,
    slug: &str,
    feature: &str,
    date: &str,
    now: f64,
) -> Result<Draft, EngineError> {
    // `slugify` falls back to `job`, so test the raw input first.
    if !slug.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err(EngineError::Msg(format!(
            "the draft slug {slug:?} has no letters or digits"
        )));
    }
    let slug = slugify(slug);
    // `factory cancel 12` would mean job 012, so a digits-only slug could not be cancelled.
    if slug.chars().all(|c| c.is_ascii_digit()) {
        return Err(EngineError::Msg(
            "a draft slug cannot be only digits".into(),
        ));
    }
    let spec_file = spec_path(driver, date, &slug);
    let draft = driver.store().update(|board| {
        if board.drafts.iter().any(|d| d.slug == slug) {
            return Err(StoreError::Other(format!(
                "a draft named {slug} already exists"
            )));
        }
        let draft = Draft {
            slug: slug.clone(),
            feature: feature.to_string(),
            stage: DRAFT_STAGES[0].to_string(),
            spec_file: Some(spec_file),
            created: date.to_string(),
            updated_at: now,
        };
        board.drafts.push(draft.clone());
        Ok(draft)
    })?;
    Ok(draft)
}

/// Moves a draft to another stage and bumps `updated_at`.
pub fn set_draft_stage(
    driver: &Driver,
    slug: &str,
    stage: &str,
    now: f64,
) -> Result<Draft, EngineError> {
    if !DRAFT_STAGES.contains(&stage) {
        return Err(EngineError::Msg(format!(
            "unknown draft stage {stage}, expected one of {}",
            DRAFT_STAGES.join(", ")
        )));
    }
    let slug = slugify(slug);
    let draft = driver.store().update(|board| {
        let draft = board
            .drafts
            .iter_mut()
            .find(|d| d.slug == slug)
            .ok_or_else(|| StoreError::Other(format!("no draft named {slug}")))?;
        draft.stage = stage.to_string();
        draft.updated_at = now;
        Ok(draft.clone())
    })?;
    Ok(draft)
}

/// Removes a draft card and returns it. The spec file is never touched.
pub fn remove_draft(driver: &Driver, slug: &str) -> Result<Draft, EngineError> {
    let slug = slugify(slug);
    let draft = driver.store().update(|board| {
        let at = board
            .drafts
            .iter()
            .position(|d| d.slug == slug)
            .ok_or_else(|| StoreError::Other(format!("no draft named {slug}")))?;
        Ok(board.drafts.remove(at))
    })?;
    Ok(draft)
}

/// The draft with this slug, if any. The slug is compared after `slugify`.
pub fn find_draft(driver: &Driver, slug: &str) -> Result<Option<Draft>, EngineError> {
    let slug = slugify(slug);
    Ok(driver
        .store()
        .load()?
        .drafts
        .into_iter()
        .find(|d| d.slug == slug))
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_agent::{AgentRunner, FakeRunner};
    use factory_core::{Config, Layout};
    use std::sync::Arc;

    fn driver(tmp: &tempfile::TempDir) -> Driver {
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
            Config::default(),
            Box::new(move |_| runner.clone()),
        )
        .expect("driver")
    }

    #[test]
    fn start_creates_an_interviewing_draft() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let draft = start_draft(&d, "demo", "a feature", "2026-10-18", 5.0).expect("start");
        assert_eq!(draft.stage, "interviewing");
        assert_eq!(
            draft.spec_file.as_deref(),
            Some("docs/superpowers/specs/2026-10-18-demo-design.md")
        );
        assert_eq!(find_draft(&d, "demo").expect("find"), Some(draft));
    }

    #[test]
    fn start_refuses_a_duplicate_slug_after_slugify() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        start_draft(&d, "Demo Card", "f", "2026-10-18", 1.0).expect("start");
        let err = start_draft(&d, "demo-card", "g", "2026-10-18", 2.0).expect_err("duplicate");
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(d.store().load().expect("load").drafts.len(), 1);
    }

    #[test]
    fn start_refuses_digit_only_and_empty_slugs() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        let err = start_draft(&d, "12", "f", "2026-10-18", 1.0).expect_err("digits");
        assert!(
            err.to_string()
                .contains("a draft slug cannot be only digits"),
            "{err}"
        );
        let err = start_draft(&d, "!!!", "f", "2026-10-18", 1.0).expect_err("empty");
        assert!(err.to_string().contains("no letters or digits"), "{err}");
        assert!(d.store().load().expect("load").drafts.is_empty());
        start_draft(&d, "2048-x", "f", "2026-10-18", 1.0).expect("mixed slug");
        start_draft(&d, "normal", "f", "2026-10-18", 1.0).expect("normal slug");
    }

    #[test]
    fn stage_rejects_an_unknown_stage_and_bumps_updated_at() {
        let tmp = tempfile::tempdir().expect("tmp");
        let d = driver(&tmp);
        start_draft(&d, "demo", "f", "2026-10-18", 1.0).expect("start");
        let err = set_draft_stage(&d, "demo", "dreaming", 2.0).expect_err("unknown");
        assert!(err.to_string().contains("unknown draft stage"), "{err}");
        let draft = set_draft_stage(&d, "demo", "writing-spec", 9.0).expect("stage");
        assert_eq!(
            (draft.stage.as_str(), draft.updated_at),
            ("writing-spec", 9.0)
        );
        assert_eq!(remove_draft(&d, "demo").expect("remove").slug, "demo");
        assert!(find_draft(&d, "demo").expect("find").is_none());
    }
}
