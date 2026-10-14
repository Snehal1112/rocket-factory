mod support;
use support::{build, review, Scenario, Step, TestResult, APPROVE, CHANGES};

#[test]
fn two_plans_go_from_new_to_merged_with_a_failed_check_resumed_once() -> TestResult {
    let sc = Scenario::new(
        "",
        vec![
            // Plan A, build: bad.flag exists, so the check fails.
            build(vec![("a.txt", "a\n"), ("bad.flag", "x\n")], 1_000),
            // The resume after the failed check deletes the flag.
            build(vec![("bad.flag", "")], 500),
            review(APPROVE),
            // Plan B: clean build and approve.
            build(vec![("b.txt", "b\n")], 1_000),
            review(APPROVE),
        ],
    )?;
    let end = sc.start(&sc.plan_a, true)?;
    assert_eq!(end, "merged", "{end}");
    // Spec decision 4: a red check resumes the builder, the committer commits the fix, and no reviewer run is spent.
    assert_eq!(sc.runner.roles(), ["builder", "committer", "builder", "committer", "reviewer", "builder", "committer", "reviewer"]);
    // Spec decision 11: plan B starts by itself after plan A merges.
    assert_eq!(sc.merges_on_main()?, 2);
    assert_eq!(sc.stage("001")?, "merged");
    assert_eq!(sc.stage("002")?, "merged");
    Ok(())
}

#[test]
fn a_changes_verdict_runs_one_fix_round_then_merges() -> TestResult {
    let sc = Scenario::new(
        "",
        vec![
            build(vec![("a.txt", "a\n")], 1_000),
            review(CHANGES),
            build(vec![("a.txt", "fixed\n")], 400),
            review(APPROVE),
        ],
    )?;
    assert_eq!(sc.start(&sc.plan_b, false)?, "merged");
    assert_eq!(sc.runner.roles(), ["builder", "committer", "reviewer", "builder", "committer", "reviewer"]);
    // Spec decision 10: the merge commit has the fixed message.
    assert!(sc.last_merge_subject()?.starts_with("chore(factory): merge job 001"), "{}", sc.last_merge_subject()?);
    Ok(())
}

#[test]
fn a_risky_diff_waits_for_approve_and_then_merges_the_reviewed_sha() -> TestResult {
    let sc = Scenario::new("", vec![build(vec![("secrets/token.txt", "x\n")], 1_000), review(APPROVE)])?;
    let before = sc.main_tip()?;
    // Spec decision 6: risky paths park the job, main is untouched.
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human") && end.contains("risky"), "{end}");
    assert_eq!(sc.runner.roles(), ["builder", "committer", "reviewer"]);
    assert_eq!(sc.stage("001")?, "needs-human");
    assert_eq!(sc.main_tip()?, before);
    let reviewed = sc.job("001")?.head.ok_or("no reviewed head")?;
    let approved = sc.run(&["approve", "1"])?;
    assert!(approved.starts_with("merged"), "{approved}");
    assert_eq!(sc.stage("001")?, "merged");
    assert_eq!(sc.second_parent_of_last_merge()?, reviewed);
    Ok(())
}

#[test]
fn a_moved_main_is_synced_into_the_branch_and_still_merges() -> TestResult {
    let hook: support::Hook = Box::new(|main| {
        // Spec decision 10: move main between the build and the merge.
        let _ = support::commit_files(main, &[("other.txt", "o\n")], "chore: move main");
    });
    let sc = Scenario::new(
        "",
        vec![
            build(vec![("b.txt", "b\n")], 1_000),
            Step::Review { text: APPROVE, before: Some(hook) },
            // The review of the sync diff.
            review(APPROVE),
        ],
    )?;
    assert_eq!(sc.start(&sc.plan_b, false)?, "merged");
    assert_eq!(sc.job("001")?.syncs, 1);
    assert!(sc.root.join("other.txt").exists() && sc.root.join("b.txt").exists());
    Ok(())
}

#[test]
fn a_budget_stop_parks_the_job_and_leaves_main_alone() -> TestResult {
    // The builder reports more weighted tokens than the budget allows.
    let sc = Scenario::new(
        "budget_weighted = 1000\n",
        vec![build(vec![("a.txt", "a\n")], 5_000), review(APPROVE)],
    )?;
    let before = sc.main_tip()?;
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human") && end.contains("budget"), "{end}");
    assert_eq!(sc.stage("001")?, "needs-human");
    assert_eq!(sc.main_tip()?, before);
    Ok(())
}

#[test]
fn tokens_report_shows_roles_and_the_baseline() -> TestResult {
    let sc = Scenario::new("", vec![build(vec![("a.txt", "a\n")], 1_000), review(APPROVE)])?;
    assert_eq!(sc.start(&sc.plan_b, false)?, "merged");
    let text = sc.run(&["tokens", "1"])?;
    assert!(text.contains("builder"), "{text}");
    assert!(text.contains("reviewer"), "{text}");
    assert!(text.contains("% of job 001 baseline weighted"), "{text}");
    Ok(())
}

#[test]
fn tokens_of_a_job_with_no_runs_prints_zeros() -> TestResult {
    let sc = Scenario::new("", vec![])?;
    let plan = sc.plan_b.to_string_lossy().to_string();
    sc.run(&["new", "hello", "x", "--plans", &plan])?;
    let text = sc.run(&["tokens", "1"])?;
    assert!(text.contains("raw 0"), "{text}");
    Ok(())
}
