//! Scripted stand-in for `codex exec --json`. The first line of stdin selects the behavior.

use std::io::{Read, Write};
use std::time::Duration;

const SESSION: &str = include_str!("../../tests/fixtures/codex-session.jsonl");
const FAILED: &str = include_str!("../../tests/fixtures/codex-failed.jsonl");
const THREAD: &str = r#"{"type":"thread.started","thread_id":"01a15eac-9d1e-73f0-98dd-ce2929f333e6"}"#;

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
        std::process::exit(0);
    }
}

fn message_and_turn(text: &str) {
    let text = text.replace('\\', "\\\\").replace('"', "\\\"");
    emit(&format!(
        r#"{{"type":"item.completed","item":{{"id":"item_1","type":"agent_message","text":"{text}"}}}}"#
    ));
    emit(
        r#"{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":1}}"#,
    );
}

fn main() {
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let mode = prompt.lines().next().map_or("", str::trim).to_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match mode.as_str() {
        "ok" => SESSION.lines().for_each(emit),
        "failed" => {
            FAILED.lines().for_each(emit);
            std::process::exit(1);
        }
        "noresult" => emit(THREAD),
        "silent" => {
            emit(THREAD);
            std::thread::sleep(Duration::from_secs(30));
        }
        "env" => {
            // The second prompt line names the variable to report.
            let name = prompt.lines().nth(1).map_or("", str::trim).to_string();
            emit(THREAD);
            message_and_turn(&std::env::var(&name).unwrap_or_else(|_| "<unset>".to_string()));
        }
        "args" => {
            emit(THREAD);
            message_and_turn(&args.join(" "));
        }
        _ => {
            eprintln!("unknown mode {mode}");
            std::process::exit(2);
        }
    }
}
