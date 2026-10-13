use factory_core::{Job, JobTokens};

#[test]
fn a_fresh_job_serializes_without_the_new_fields_and_loads_back() {
    let job = Job::new("001", "2026-10-13-001-demo", "demo", "2026-10-13", "demo feature", 1.0);
    let text = serde_json::to_string(&job).expect("json");
    assert!(!text.contains("prev_head"), "{text}");
    assert!(!text.contains("tokens"), "{text}");
    let back: Job = serde_json::from_str(&text).expect("parse");
    assert_eq!(back, job);
}

#[test]
fn prev_head_and_tokens_round_trip() {
    let mut job = Job::new("001", "2026-10-13-001-demo", "demo", "2026-10-13", "demo feature", 1.0);
    job.prev_head = Some("abc".to_string());
    job.tokens.add(100, 50, 25, 1000);
    let back: Job = serde_json::from_str(&serde_json::to_string(&job).expect("json")).expect("parse");
    assert_eq!(back, job);
}

#[test]
fn tokens_add_and_weigh() {
    let mut t = JobTokens::default();
    t.add(100, 50, 25, 1000);
    t.add(1, 1, 1, 10);
    // 101 input, 51 cache creation, 26 output and 1010 / 10 cache read.
    assert_eq!(t.weighted(), 101 + 51 + 26 + 101);
}
