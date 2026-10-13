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
    assert!(std::fs::read_to_string(&path).expect("read").ends_with('\n'));
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
