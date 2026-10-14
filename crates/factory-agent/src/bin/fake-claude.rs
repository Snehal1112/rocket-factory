//! Scripted stand-in for `claude -p`. The first line of stdin selects the behavior.

use std::io::{Read, Write};
use std::time::Duration;

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
        std::process::exit(0);
    }
}

fn init(session: &str) {
    emit(&format!(r#"{{"type":"system","subtype":"init","session_id":"{session}"}}"#));
}

fn assistant(id: &str, output: u64) {
    emit(&format!(
        r#"{{"type":"assistant","message":{{"id":"{id}","content":[],"usage":{{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":{output}}}}},"session_id":"fake-1"}}"#
    ));
}

fn result(text: &str, is_error: bool, subtype: &str) {
    let text = text.replace('\\', "\\\\").replace('"', "\\\"");
    emit(&format!(
        r#"{{"type":"result","subtype":"{subtype}","is_error":{is_error},"result":"{text}","session_id":"fake-1"}}"#
    ));
}

fn main() {
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let mode = prompt.lines().next().map_or("", str::trim).to_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match mode.as_str() {
        "ok" => {
            init("fake-1");
            assistant("m1", 5);
            assistant("m1", 50);
            emit(r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"echo hi"}}],"usage":{"output_tokens":10}}}"#);
            result("all good", false, "success");
        }
        "args" => {
            init("fake-1");
            result(&args.join(" "), false, "success");
        }
        "env" => {
            // The second prompt line names the variable to report.
            let name = prompt.lines().nth(1).map_or("", str::trim).to_string();
            init("fake-1");
            result(&std::env::var(&name).unwrap_or_else(|_| "<unset>".to_string()), false, "success");
        }
        "flood" => {
            init("fake-1");
            let junk = "x".repeat(200);
            for i in 0..20000 {
                emit(&format!("garbage {i} {junk}"));
                if i % 100 == 0 {
                    eprintln!("stderr noise {i} {junk}");
                }
            }
            // A non-UTF-8 line must not stop the reader.
            let _ = std::io::stdout().write_all(&[0xff, 0xfe, b'\n']);
            result("flooded", false, "success");
        }
        "silent" => {
            init("fake-1");
            std::thread::sleep(Duration::from_secs(30));
        }
        "budget" => {
            init("fake-1");
            for i in 0..1000 {
                assistant(&format!("b{i}"), 1000);
                std::thread::sleep(Duration::from_millis(20));
            }
            result("never reached", false, "success");
        }
        "turns" => {
            init("fake-1");
            for i in 0..50 {
                assistant(&format!("t{i}"), 1);
                std::thread::sleep(Duration::from_millis(20));
            }
            result("never reached", false, "success");
        }
        "error" => {
            init("fake-1");
            result("it broke", true, "error_during_execution");
        }
        "noresult" => {
            init("fake-1");
            eprintln!("fake claude crashed: boom");
            std::process::exit(3);
        }
        _ => {
            eprintln!("unknown mode {mode}");
            std::process::exit(2);
        }
    }
}
