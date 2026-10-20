use factory_core::{Board, ReviewState, Stage};
use std::path::Path;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/board.json");

#[test]
fn real_board_round_trips_without_losing_fields() {
    let text = std::fs::read_to_string(FIXTURE).expect("fixture readable");
    let original: serde_json::Value = serde_json::from_str(&text).expect("fixture is json");
    let board = Board::load(Path::new(FIXTURE)).expect("board loads");
    assert_eq!(board.jobs.len(), 2);
    assert_eq!(board.jobs[0].stage, Stage::Merged);
    assert_eq!(board.jobs[0].reviews["ux"], ReviewState::Skipped);
    let again = serde_json::to_value(&board).expect("board serializes");
    assert_eq!(again, original);
}

#[test]
fn zero_task_index_is_not_serialised() {
    let mut board = Board::load(Path::new(FIXTURE)).expect("board loads");
    assert_eq!(board.jobs[0].task_index, 0);
    let text = serde_json::to_string(&board.jobs[0]).expect("json");
    assert!(!text.contains("task_index"));
    board.jobs[0].task_index = 2;
    let text = serde_json::to_string(&board.jobs[0]).expect("json");
    assert!(text.contains("\"task_index\":2"));
}

#[test]
fn save_then_load_is_identical() {
    let board = Board::load(Path::new(FIXTURE)).expect("board loads");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sub").join("board.json");
    board.save(&path).expect("save");
    let back = Board::load(&path).expect("reload");
    assert_eq!(back, board);
    assert!(std::fs::read_to_string(&path)
        .expect("read")
        .ends_with('\n'));
}

#[test]
fn missing_file_is_an_empty_board() {
    let dir = tempfile::tempdir().expect("tempdir");
    let board = Board::load(&dir.path().join("none.json")).expect("missing is ok");
    assert!(board.jobs.is_empty());
}

#[test]
fn corrupt_file_is_an_error_not_an_empty_board() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("board.json");
    std::fs::write(&path, "{\"jobs\": [{\"id\": ").expect("write");
    assert!(Board::load(&path).is_err());
    std::fs::write(&path, r#"{"jobs":[{"id":"001","stage":"flying"}]}"#).expect("write");
    assert!(Board::load(&path).is_err());
}

#[test]
fn an_old_board_without_drafts_loads() {
    let board: Board = serde_json::from_str(r#"{"jobs":[]}"#).expect("parse");
    assert!(board.drafts.is_empty());
}

#[test]
fn a_board_with_no_drafts_serializes_without_the_key() {
    let text = serde_json::to_string(&Board::default()).expect("json");
    assert!(!text.contains("drafts"));
}

#[test]
fn drafts_round_trip_with_every_field() {
    let board = Board {
        drafts: vec![factory_core::Draft {
            slug: "cards".to_string(),
            feature: "Draft cards".to_string(),
            stage: "writing-spec".to_string(),
            spec_file: Some("docs/superpowers/specs/2026-10-18-cards-design.md".to_string()),
            created: "2026-10-18".to_string(),
            updated_at: 12.5,
        }],
        ..Default::default()
    };
    let back: Board =
        serde_json::from_str(&serde_json::to_string(&board).expect("json")).expect("parse");
    assert_eq!(back, board);
    let mut bare = board.clone();
    bare.drafts[0].spec_file = None;
    assert!(!serde_json::to_string(&bare)
        .expect("json")
        .contains("spec_file"));
    assert_eq!(
        factory_core::DRAFT_STAGES,
        ["interviewing", "writing-spec", "writing-plans"]
    );
}

#[test]
fn a_board_without_task_session_loads() {
    let board = Board::load(Path::new(FIXTURE)).expect("board loads");
    assert!(board.jobs.iter().all(|j| j.task_session.is_none()));
    assert!(!serde_json::to_string(&board)
        .expect("json")
        .contains("task_session"));
    let text = serde_json::to_string(&factory_core::Job::new(
        "001",
        "d",
        "s",
        "2026-10-20",
        "f",
        1.0,
    ))
    .expect("json");
    let job = serde_json::from_str::<factory_core::Job>(&text).expect("parse");
    assert_eq!(job.task_session, None);
}

#[test]
fn spec_warning_is_optional_on_a_job() {
    let mut j = factory_core::Job::new("001", "d", "s", "2026-10-18", "f", 1.0);
    assert!(!serde_json::to_string(&j)
        .expect("json")
        .contains("spec_warning"));
    j.spec_warning = Some("no spec".to_string());
    let back: factory_core::Job =
        serde_json::from_str(&serde_json::to_string(&j).expect("json")).expect("parse");
    assert_eq!(back.spec_warning.as_deref(), Some("no spec"));
}
