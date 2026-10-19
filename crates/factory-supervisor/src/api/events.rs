//! The event stream: `GET /events` follows `events.jsonl` of one or all projects as server-sent events.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use factory_core::project;
use factory_core::Home;
use serde_json::Value;

use crate::api::http::{error_response, ApiResponse};

/// Lines replayed when a stream opens.
pub const REPLAY: usize = 50;
/// How much of the file end is read to find the replay lines.
const TAIL_BYTES: u64 = 1024 * 1024;
/// The most one poll reads.
const CHUNK: u64 = 1024 * 1024;
/// A line longer than this is dropped, so a file without newlines cannot grow memory.
const MAX_LINE: usize = 256 * 1024;

/// Follows one `events.jsonl`. It returns complete lines only, tagged with the project.
#[derive(Debug)]
pub struct EventFollower {
    path: PathBuf,
    project: String,
    pos: u64,
    ident: Option<u64>,
    partial: Vec<u8>,
}

#[cfg(unix)]
fn ident_of(meta: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ino())
}

#[cfg(not(unix))]
fn ident_of(_meta: &std::fs::Metadata) -> Option<u64> {
    None
}

/// Adds `"project"` to a JSON object line. A line that is not a JSON object gives nothing.
fn tag(project: &str, line: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(line).ok()?.trim();
    match serde_json::from_str::<Value>(text).ok()? {
        Value::Object(mut map) => {
            map.insert("project".to_string(), Value::String(project.to_string()));
            Some(Value::Object(map).to_string())
        }
        _ => None,
    }
}

impl EventFollower {
    /// Opens the file and returns the follower and the last `replay` lines.
    pub fn open(path: PathBuf, project: &str, replay: usize) -> (EventFollower, Vec<String>) {
        let mut f = EventFollower {
            path,
            project: project.to_string(),
            pos: 0,
            ident: None,
            partial: Vec::new(),
        };
        let lines = f.replay(replay);
        (f, lines)
    }

    fn replay(&mut self, count: usize) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        self.ident = ident_of(&meta);
        let len = meta.len();
        let start = len.saturating_sub(TAIL_BYTES);
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(start)).is_err()
            || (&mut file)
                .take(len - start)
                .read_to_end(&mut bytes)
                .is_err()
        {
            return Vec::new();
        }
        // Everything up to the last newline is complete. The rest waits for its newline.
        let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else {
            self.pos = start;
            return Vec::new();
        };
        self.pos = start + end as u64 + 1;
        let mut body = &bytes[..end];
        // A tail that starts in the middle of a line loses that first line.
        if start > 0 {
            body = match body.iter().position(|b| *b == b'\n') {
                Some(i) => &body[i + 1..],
                None => &[],
            };
        }
        let all: Vec<String> = body
            .split(|b| *b == b'\n')
            .filter_map(|l| tag(&self.project, l))
            .collect();
        all[all.len().saturating_sub(count)..].to_vec()
    }

    /// The new complete lines since the last call. A shorter or replaced file is read again from the start.
    pub fn poll(&mut self) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        let ident = ident_of(&meta);
        if meta.len() < self.pos || (self.ident.is_some() && ident != self.ident) {
            self.pos = 0;
            self.partial.clear();
        }
        self.ident = ident;
        if meta.len() == self.pos {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(self.pos)).is_err()
            || (&mut file).take(CHUNK).read_to_end(&mut bytes).is_err()
        {
            return Vec::new();
        }
        self.pos += bytes.len() as u64;
        self.partial.extend_from_slice(&bytes);
        let mut out = Vec::new();
        while let Some(i) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=i).collect();
            if let Some(tagged) = tag(&self.project, &line[..i]) {
                out.push(tagged);
            }
        }
        if self.partial.len() > MAX_LINE {
            self.partial.clear();
        }
        out
    }
}

/// How often the stream looks at the files, sends a keepalive and looks for new projects.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    pub keepalive: Duration,
    pub refresh: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            poll: Duration::from_millis(500),
            keepalive: Duration::from_secs(15),
            refresh: Duration::from_secs(5),
        }
    }
}

/// The SSE bytes of a stream. `read` blocks until it has something to give, and returns 0 once stopped.
pub struct EventStream {
    followers: Vec<EventFollower>,
    /// With a home, projects that appear later are added.
    discover: Option<Home>,
    stop: Arc<AtomicBool>,
    timing: Timing,
    pending: Vec<u8>,
    last_output: Instant,
    last_poll: Option<Instant>,
    last_refresh: Instant,
}

fn frame(line: &str) -> Vec<u8> {
    format!("data: {line}\n\n").into_bytes()
}

impl EventStream {
    pub fn new(
        followers: Vec<EventFollower>,
        replay: Vec<String>,
        discover: Option<Home>,
        stop: Arc<AtomicBool>,
        timing: Timing,
    ) -> EventStream {
        let mut pending = Vec::new();
        for l in &replay {
            pending.extend(frame(l));
        }
        let now = Instant::now();
        EventStream {
            followers,
            discover,
            stop,
            timing,
            pending,
            last_output: now,
            last_poll: None,
            last_refresh: now,
        }
    }

    fn add_new_projects(&mut self) {
        let Some(home) = &self.discover else { return };
        let Ok(entries) = project::list(home) else {
            return;
        };
        for e in entries {
            if self.followers.iter().any(|f| f.project == e.name) {
                continue;
            }
            let Ok(file) = e.file else { continue };
            let Ok(layout) = project::layout(home, &e.name, &file) else {
                continue;
            };
            let (f, lines) = EventFollower::open(layout.events_file(), &e.name, REPLAY);
            for l in &lines {
                self.pending.extend(frame(l));
            }
            self.followers.push(f);
        }
    }

    fn step(&mut self) {
        let now = Instant::now();
        if self
            .last_poll
            .is_some_and(|t| now.duration_since(t) < self.timing.poll)
        {
            std::thread::sleep(Duration::from_millis(20).min(self.timing.poll));
            return;
        }
        self.last_poll = Some(now);
        if self.discover.is_some() && now.duration_since(self.last_refresh) >= self.timing.refresh {
            self.last_refresh = now;
            self.add_new_projects();
        }
        for f in &mut self.followers {
            for l in f.poll() {
                self.pending.extend(frame(&l));
            }
        }
        if self.pending.is_empty() && now.duration_since(self.last_output) >= self.timing.keepalive
        {
            self.pending.extend_from_slice(b": keepalive\n\n");
        }
    }
}

impl Read for EventStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.pending.is_empty() {
                let n = buf.len().min(self.pending.len());
                buf[..n].copy_from_slice(&self.pending[..n]);
                self.pending.drain(..n);
                self.last_output = Instant::now();
                return Ok(n);
            }
            if self.stop.load(Ordering::SeqCst) {
                return Ok(0);
            }
            self.step();
        }
    }
}

/// Copies the stream to the client and flushes each piece. It ends when the stream ends or a write fails.
pub fn pump<R: Read, W: Write>(mut stream: R, out: &mut W) {
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                if out.write_all(&buf[..n]).is_err() || out.flush().is_err() {
                    return;
                }
            }
        }
    }
}

/// Builds the stream for `GET /events?project=<p>`. No `project` means every project.
pub fn open(
    home: &Home,
    query: &str,
    stop: Arc<AtomicBool>,
    timing: Timing,
) -> Result<EventStream, ApiResponse> {
    let mut only: Option<String> = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        match pair.split_once('=') {
            Some(("project", v)) if only.is_none() => only = Some(v.to_string()),
            _ => {
                return Err(error_response(
                    400,
                    "bad_request",
                    "the only query parameter is project=<name>",
                ))
            }
        }
    }
    let mut followers = Vec::new();
    let mut replay = Vec::new();
    if let Some(name) = &only {
        project::validate_name(name)
            .map_err(|e| error_response(400, "bad_request", &e.to_string()))?;
        if !home.project_dir(name).is_dir() {
            return Err(error_response(
                404,
                "not_found",
                &format!("no project named {name}"),
            ));
        }
        let disabled = |why: String| error_response(409, "project_disabled", &why);
        let file = project::get(home, name).map_err(|e| disabled(e.to_string()))?;
        let layout = project::layout(home, name, &file).map_err(|e| disabled(e.to_string()))?;
        let (f, lines) = EventFollower::open(layout.events_file(), name, REPLAY);
        followers.push(f);
        replay = lines;
    }
    let mut stream = EventStream::new(followers, replay, None, stop, timing);
    if only.is_none() {
        stream.discover = Some(home.clone());
        stream.add_new_projects();
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::testutil::*;

    fn append(path: &std::path::Path, text: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open");
        f.write_all(text.as_bytes()).expect("append");
    }

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("{{\"n\":{i}}}\n")).collect()
    }

    #[test]
    fn open_replays_the_last_lines_tagged() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, &lines(5));
        let (_f, replay) = EventFollower::open(path, "p", 3);
        assert_eq!(
            replay,
            vec![
                r#"{"n":3,"project":"p"}"#,
                r#"{"n":4,"project":"p"}"#,
                r#"{"n":5,"project":"p"}"#
            ]
        );
    }

    #[test]
    fn a_missing_file_is_empty_until_it_appears() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        let (mut f, replay) = EventFollower::open(path.clone(), "p", 10);
        assert!(replay.is_empty() && f.poll().is_empty());
        append(&path, "{\"a\":1}\n");
        assert_eq!(f.poll(), vec![r#"{"a":1,"project":"p"}"#]);
    }

    #[test]
    fn only_new_complete_lines_follow_and_a_partial_line_waits() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, &lines(2));
        let (mut f, _) = EventFollower::open(path.clone(), "p", 10);
        assert!(f.poll().is_empty());
        append(&path, "{\"n\":3}\n{\"n\":");
        assert_eq!(f.poll(), vec![r#"{"n":3,"project":"p"}"#]);
        assert!(f.poll().is_empty());
        append(&path, "4}\n");
        assert_eq!(f.poll(), vec![r#"{"n":4,"project":"p"}"#]);
    }

    #[test]
    fn a_partial_last_line_is_not_replayed() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, "{\"n\":1}\n{\"n\":2");
        let (mut f, replay) = EventFollower::open(path.clone(), "p", 10);
        assert_eq!(replay, vec![r#"{"n":1,"project":"p"}"#]);
        append(&path, "}\n");
        assert_eq!(f.poll(), vec![r#"{"n":2,"project":"p"}"#]);
    }

    #[test]
    fn truncation_and_replacement_restart_from_the_start() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, &lines(3));
        let (mut f, _) = EventFollower::open(path.clone(), "p", 0);
        std::fs::write(&path, "{\"n\":9}\n").expect("truncate");
        assert_eq!(f.poll(), vec![r#"{"n":9,"project":"p"}"#]);
        // A replacement that is as long as the old file is told by its identity.
        let other = t.path().join("other");
        std::fs::write(&other, "{\"n\":7}\n").expect("other");
        std::fs::rename(&other, &path).expect("replace");
        #[cfg(unix)]
        assert_eq!(f.poll(), vec![r#"{"n":7,"project":"p"}"#]);
    }

    #[test]
    fn lines_that_are_not_json_objects_are_skipped() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, "garbage\n[1]\n{\"ok\":true}\n");
        let (_f, replay) = EventFollower::open(path, "p", 10);
        assert_eq!(replay, vec![r#"{"ok":true,"project":"p"}"#]);
    }

    fn fast() -> Timing {
        Timing {
            poll: Duration::from_millis(10),
            keepalive: Duration::from_millis(80),
            refresh: Duration::from_millis(30),
        }
    }

    fn read_some(s: &mut EventStream) -> String {
        let mut buf = [0u8; 4096];
        let n = s.read(&mut buf).expect("read");
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn the_stream_gives_sse_frames_keepalives_and_ends_on_stop() {
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, "{\"n\":1}\n");
        let (f, replay) = EventFollower::open(path.clone(), "p", 10);
        let stop = Arc::new(AtomicBool::new(false));
        let mut s = EventStream::new(vec![f], replay, None, stop.clone(), fast());
        assert_eq!(read_some(&mut s), "data: {\"n\":1,\"project\":\"p\"}\n\n");
        assert_eq!(read_some(&mut s), ": keepalive\n\n");
        append(&path, "{\"n\":2}\n");
        assert_eq!(read_some(&mut s), "data: {\"n\":2,\"project\":\"p\"}\n\n");
        stop.store(true, Ordering::SeqCst);
        assert_eq!(read_some(&mut s), "");
    }

    #[test]
    fn open_checks_the_project_and_the_query() {
        let (_t, home) = fixture();
        let stop = Arc::new(AtomicBool::new(false));
        let code = |q: &str| {
            open(&home, q, stop.clone(), fast()).err().map(|r| {
                (
                    r.status,
                    r.body["error"]["code"].as_str().map(str::to_string),
                )
            })
        };
        assert_eq!(
            code("project=Bad..Name"),
            Some((400, Some("bad_request".to_string())))
        );
        assert_eq!(
            code("other=1"),
            Some((400, Some("bad_request".to_string())))
        );
        assert_eq!(
            code("project=ghost"),
            Some((404, Some("not_found".to_string())))
        );
        assert_eq!(code("project=good"), None);
        assert_eq!(code(""), None);
    }

    #[test]
    fn all_projects_stream_every_project_with_its_tag() {
        let (_t, home) = fixture();
        let state = home.project_dir("good").join("state");
        std::fs::create_dir_all(&state).expect("state");
        append(&state.join("events.jsonl"), "{\"k\":1}\n");
        let stop = Arc::new(AtomicBool::new(false));
        let mut s = open(&home, "", stop, fast()).expect("open");
        assert_eq!(
            read_some(&mut s),
            "data: {\"k\":1,\"project\":\"good\"}\n\n"
        );
    }

    #[test]
    fn a_dropped_client_ends_the_pump() {
        use std::net::{TcpListener, TcpStream};
        let t = tempfile::tempdir().expect("tempdir");
        let path = t.path().join("events.jsonl");
        append(&path, "{\"n\":1}\n");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = TcpStream::connect(addr).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");
        let (f, replay) = EventFollower::open(path, "p", 10);
        let stop = Arc::new(AtomicBool::new(false));
        let stream = EventStream::new(vec![f], replay, None, stop, fast());
        let handle = std::thread::spawn(move || pump(stream, &mut server_side));
        let mut first = [0u8; 64];
        let mut client = client;
        let n = client.read(&mut first).expect("first frame");
        assert!(String::from_utf8_lossy(&first[..n]).starts_with("data: "));
        drop(client);
        let began = Instant::now();
        while !handle.is_finished() {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "the pump did not end"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        handle.join().expect("join");
    }
}
