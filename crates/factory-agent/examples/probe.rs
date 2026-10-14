//! Probes which `claude` headless flags exist and how they behave.
//! Run: cargo run -j4 --manifest-path tools/factory/Cargo.toml -p factory-agent --example probe

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn run(args: &[&str], stdin: Option<&str>, cwd: &Path) -> Result<Out, Box<dyn std::error::Error>> {
    let mut child = Command::new("claude")
        .args(args)
        .current_dir(cwd)
        // Never bill an API account from a probe run.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut s) = child.stdin.take() {
        if let Some(text) = stdin {
            let _ = s.write_all(text.as_bytes());
        }
    }
    let mut so = child.stdout.take().ok_or("no stdout")?;
    let mut se = child.stderr.take().ok_or("no stderr")?;
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let mut timed_out = false;
    let code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code();
        }
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = t_out.join().map_err(|_| "stdout thread panicked")?;
    let stderr = t_err.join().map_err(|_| "stderr thread panicked")?;
    Ok(Out { code, stdout, stderr, timed_out })
}

/// Summarises a stream-json stdout: line types, session ids, message ids and usage places.
fn summarize(stdout: &str) -> String {
    let mut types = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    let (mut assistant_lines, mut ids) = (0u32, BTreeSet::new());
    let (mut result_usage, mut result_line) = (false, String::new());
    let mut bad = 0u32;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            bad += 1;
            continue;
        };
        let ty = v.get("type").and_then(|t| t.as_str()).map_or("?", |t| t).to_string();
        if let Some(s) = v.get("session_id").and_then(|s| s.as_str()) {
            sessions.insert(s.to_string());
        }
        if ty == "assistant" {
            assistant_lines += 1;
            if let Some(id) = v.pointer("/message/id").and_then(|i| i.as_str()) {
                ids.insert(id.to_string());
            }
        }
        if ty == "result" {
            result_usage = v.get("usage").is_some();
            result_line = line.chars().take(400).collect();
        }
        types.insert(ty);
    }
    format!(
        "types={types:?} sessions={sessions:?} assistant_lines={assistant_lines} unique_message_ids={} result_has_usage={result_usage} unparsable_lines={bad}\n    result_line={result_line}",
        ids.len()
    )
}

fn report(title: &str, out: &Out) {
    let err: String = out.stderr.chars().take(300).collect();
    println!("### {title}\n- exit={:?} timed_out={}\n- stderr={err:?}\n- {}\n", out.code, out.timed_out, summarize(&out.stdout));
}

fn first_session(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        v.get("session_id")?.as_str().map(str::to_string)
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let cwd = dir.path();
    std::fs::create_dir_all(cwd.join(".claude/agents"))?;
    std::fs::write(
        cwd.join(".claude/agents/probe.md"),
        "---\nname: probe\ndescription: probe agent\n---\nYou end every reply with the word PROBED.\n",
    )?;

    println!("## Probe output\n");
    let help = run(&["--help"], None, cwd)?;
    println!("### claude --help flag presence");
    for flag in [
        "--print", "--output-format", "--verbose", "--agent", "--resume", "--permission-mode", "--allowedTools",
        "--model", "--max-turns", "--append-system-prompt", "--session-id", "--max-budget-usd", "--include-partial-messages",
    ] {
        println!("- {flag}: {}", if help.stdout.contains(flag) { "listed" } else { "NOT listed" });
    }
    println!();

    let base = ["-p", "--model", "haiku", "--max-budget-usd", "0.05"];
    let join = |extra: &[&str]| -> Vec<String> { base.iter().chain(extra.iter()).map(|s| s.to_string()).collect() };
    let call = |extra: &[&str], stdin: &str| -> Result<Out, Box<dyn std::error::Error>> {
        let args = join(extra);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run(&refs, Some(stdin), cwd)
    };

    report("stream-json without --verbose", &call(&["--output-format", "stream-json"], "Reply with exactly: PONG")?);
    let verbose = call(&["--output-format", "stream-json", "--verbose"], "Reply with exactly: PONG")?;
    report("stream-json with --verbose (prompt on stdin)", &verbose);
    report("max-turns 1", &call(&["--output-format", "stream-json", "--verbose", "--max-turns", "1"], "Reply with exactly: PONG")?);
    report("append-system-prompt", &call(&["--output-format", "stream-json", "--verbose", "--append-system-prompt", "End every reply with ZEBRA."], "Say hi.")?);
    report("permission-mode acceptEdits and allowedTools", &call(&["--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--allowedTools", "Read,Bash"], "Run `echo hi` with the Bash tool, then reply DONE.")?);
    report("agent probe", &call(&["--output-format", "stream-json", "--verbose", "--agent", "probe"], "Say hi.")?);
    if let Some(session) = first_session(&verbose.stdout) {
        let resumed = call(&["--output-format", "stream-json", "--verbose", "--resume", &session], "What did I ask you to reply with? One word.")?;
        println!("original session id: {session}");
        report("resume (compare session ids above with the original)", &resumed);
    } else {
        println!("### resume\n- skipped: no session id in the verbose stream\n");
    }
    Ok(())
}
