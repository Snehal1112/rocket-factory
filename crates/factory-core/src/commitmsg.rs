//! Rules for commit messages and builder commit notes, checked by the driver.

/// Longest subject the driver accepts. The commit skill asks for 50, and the driver
/// allows slack so a 51 character subject does not fail a run.
pub const MAX_SUBJECT_CHARS: usize = 72;

const TYPES: &[&str] = &[
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// Checks a full commit message: a Conventional Commits subject of at most 72 characters
/// with no trailing period, and a blank line before any body. The error is a short reason.
pub fn validate_message(message: &str) -> Result<(), String> {
    let mut lines = message.lines();
    let subject = lines.next().unwrap_or("");
    if subject.trim().is_empty() {
        return Err("the subject is empty".to_string());
    }
    let chars = subject.chars().count();
    if chars > MAX_SUBJECT_CHARS {
        return Err(format!(
            "the subject is {chars} characters, the limit is {MAX_SUBJECT_CHARS}"
        ));
    }
    if subject.ends_with(char::is_whitespace) {
        return Err("the subject ends with whitespace".to_string());
    }
    if subject.ends_with('.') {
        return Err("the subject ends with a period".to_string());
    }
    let (head, description) = subject
        .split_once(": ")
        .ok_or_else(|| "the subject must look like `type(scope): description`".to_string())?;
    let head = head.strip_suffix('!').unwrap_or(head);
    let (kind, scope) = match head.split_once('(') {
        Some((kind, rest)) => {
            let scope = rest
                .strip_suffix(')')
                .ok_or_else(|| "the scope has no closing parenthesis".to_string())?;
            (kind, Some(scope))
        }
        None => (head, None),
    };
    if !TYPES.contains(&kind) {
        return Err(format!(
            "`{kind}` is not a lowercase Conventional Commits type"
        ));
    }
    if let Some(scope) = scope {
        if scope.is_empty() || scope.chars().any(char::is_whitespace) {
            return Err("the scope must be one word".to_string());
        }
    }
    if description.trim().is_empty() {
        return Err("the description is empty".to_string());
    }
    if let Some(second) = lines.next() {
        if !second.trim().is_empty() {
            return Err("a blank line must follow the subject".to_string());
        }
    }
    Ok(())
}

/// One issue reference found in text. `repo` is `owner/repo` when the text was a GitHub issue URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IssueRef {
    pub number: u32,
    pub repo: Option<String>,
}

const GITHUB_PREFIX: &str = "https://github.com/";

/// True for a GitHub owner or repository name: letters, digits, `-`, `_` and `.`.
pub fn is_repo_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_repo_char)
}

/// True for a character that may be part of a GitHub owner or repository name.
fn is_repo_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

/// The issue number after the `#` at `hash`. N must be at least 1, fit `u32` and be followed by the
/// end of the text or a character that is not alphanumeric or `_`.
fn number_after_hash(chars: &[char], hash: usize) -> Option<u32> {
    let start = hash + 1;
    let len = chars[start..]
        .iter()
        .take_while(|d| d.is_ascii_digit())
        .count();
    if chars
        .get(start + len)
        .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    {
        return None;
    }
    let digits: String = chars[start..start + len].iter().collect();
    digits.parse::<u32>().ok().filter(|n| *n >= 1)
}

/// The `#N` references. The `#` must start the text or follow a character that is not alphanumeric
/// or `_`, and N must pass `number_after_hash`.
fn hash_refs(text: &str) -> Vec<IssueRef> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '#' {
            continue;
        }
        if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            continue;
        }
        if let Some(number) = number_after_hash(&chars, i) {
            found.push(IssueRef { number, repo: None });
        }
    }
    found
}

/// The `owner/repo#N` references. The char before the owner must be absent or neither a repository
/// name char nor `/`.
fn shorthand_refs(text: &str) -> Vec<IssueRef> {
    let chars: Vec<char> = strip_code(text).chars().collect();
    let mut found = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '#' {
            continue;
        }
        let repo_start = chars[..i]
            .iter()
            .rposition(|c| !is_repo_char(*c))
            .map_or(0, |p| p + 1);
        if repo_start == 0 || chars[repo_start - 1] != '/' {
            continue;
        }
        let owner_end = repo_start - 1;
        let owner_start = chars[..owner_end]
            .iter()
            .rposition(|c| !is_repo_char(*c))
            .map_or(0, |p| p + 1);
        if owner_start > 0 && chars[owner_start - 1] == '/' {
            continue;
        }
        let owner: String = chars[owner_start..owner_end].iter().collect();
        let repo: String = chars[repo_start..i].iter().collect();
        if !is_repo_name(&owner) || !is_repo_name(&repo) {
            continue;
        }
        if let Some(number) = number_after_hash(&chars, i) {
            found.push(IssueRef {
                number,
                repo: Some(format!("{owner}/{repo}")),
            });
        }
    }
    found
}

/// The `https://github.com/<owner>/<repo>/issues/N` references.
fn url_refs(text: &str) -> Vec<IssueRef> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(GITHUB_PREFIX) {
        let tail = &rest[at + GITHUB_PREFIX.len()..];
        let mut parts = tail.splitn(4, '/');
        let (owner, repo, kind, after) = (parts.next(), parts.next(), parts.next(), parts.next());
        if let (Some(owner), Some(repo), Some("issues"), Some(after)) = (owner, repo, kind, after) {
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(number) = digits.parse::<u32>() {
                if number >= 1 && is_repo_name(owner) && is_repo_name(repo) {
                    found.push(IssueRef {
                        number,
                        repo: Some(format!("{owner}/{repo}")),
                    });
                }
            }
        }
        rest = tail;
    }
    found
}

/// Blanks fenced blocks and inline code spans, so refs inside code are not found.
fn strip_code(text: &str) -> String {
    let mut in_fence = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            lines.push(String::new());
        } else if in_fence {
            lines.push(String::new());
        } else {
            lines.push(strip_spans(line));
        }
    }
    lines.join("\n")
}

/// Replaces each backtick pair on one line, and what it holds, with a space. A lone backtick stays.
fn strip_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        match after.find('`') {
            Some(close) => {
                out.push_str(&rest[..open]);
                out.push(' ');
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// The `#N` and GitHub issue URL references outside code, sorted and de-duplicated.
fn plain_refs(text: &str) -> Vec<IssueRef> {
    let text = strip_code(text);
    let mut refs = hash_refs(&text);
    refs.extend(url_refs(&text));
    refs.sort();
    refs.dedup();
    refs
}

/// Every issue reference in the text outside code, `#N`, GitHub issue URLs and `owner/repo#N`,
/// sorted and de-duplicated. Refs in inline code or fenced blocks do not count.
pub fn parse_issue_refs(text: &str) -> Vec<IssueRef> {
    let mut refs = plain_refs(text);
    refs.extend(shorthand_refs(text));
    refs.sort();
    refs.dedup();
    refs
}

/// Every issue number in the text, sorted and de-duplicated. `#N` and GitHub issue URLs both count,
/// `owner/repo#N` does not. Refs in inline code or fenced blocks do not count.
pub fn parse_issues(text: &str) -> Vec<u32> {
    let mut numbers: Vec<u32> = plain_refs(text).into_iter().map(|r| r.number).collect();
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// The footer line `<key>: #20, #21`, or `None` when there are no issues.
pub fn issue_footer(key: &str, issues: &[u32]) -> Option<String> {
    if issues.is_empty() {
        return None;
    }
    let list: Vec<String> = issues.iter().map(|n| format!("#{n}")).collect();
    Some(format!("{key}: {}", list.join(", ")))
}

/// True when there are no issues, or a line of the message equals the footer line exactly.
pub fn has_footer(message: &str, key: &str, issues: &[u32]) -> bool {
    match issue_footer(key, issues) {
        None => true,
        Some(line) => message.lines().any(|l| l == line),
    }
}

/// True when the line looks like `Key: value` with a key of letters, digits and dashes.
fn is_footer_line(line: &str) -> bool {
    match line.split_once(": ") {
        Some((key, value)) => {
            !key.is_empty()
                && !value.trim().is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
                && !key.starts_with(' ')
                && !key.ends_with(' ')
        }
        None => false,
    }
}

/// Appends footer lines after the body. Lines the message already has are skipped. The lines join an
/// existing footer paragraph, or start a new one after a blank line.
pub fn append_footers(message: &str, lines: &[String]) -> String {
    let base = message.trim_end();
    let new: Vec<&String> = lines
        .iter()
        .filter(|l| !base.lines().any(|m| m == l.as_str()))
        .collect();
    if new.is_empty() {
        return base.to_string();
    }
    let mut out = base.to_string();
    let last_paragraph: Vec<&str> = base.rsplit("\n\n").next().unwrap_or("").lines().collect();
    let has_paragraph = base.contains("\n\n");
    let in_footer = has_paragraph
        && !last_paragraph.is_empty()
        && last_paragraph.iter().all(|l| is_footer_line(l));
    out.push_str(if in_footer { "\n" } else { "\n\n" });
    let joined: Vec<&str> = new.iter().map(|l| l.as_str()).collect();
    out.push_str(&joined.join("\n"));
    out
}

/// The reason after `BLOCKED:` when the first line of an agent reply starts with it.
pub fn blocked_reason(reply: &str) -> Option<String> {
    let first = reply.lines().next()?.trim();
    first
        .strip_prefix("BLOCKED:")
        .map(|reason| reason.trim().to_string())
}

/// What a builder reply says about how the task went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderStatus {
    Done,
    DoneWithConcerns(String),
    Blocked(String),
    NeedsContext(String),
}

/// The lines of the reply outside code fences.
fn unfenced_lines(reply: &str) -> impl Iterator<Item = &str> {
    let mut in_fence = false;
    reply.lines().filter(move |line| {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            return false;
        }
        !in_fence
    })
}

/// The value after a `Status:` key, or `None` when the line is not a status line.
/// Asterisks and backticks around the key and the value are ignored, so `**Status**: X`,
/// `**Status: X**` and ``Status: `X` `` all count. A leading `- ` is allowed.
fn status_value(line: &str) -> Option<&str> {
    const MARKS: [char; 2] = ['*', '`'];
    let t = line.trim();
    let t = t.strip_prefix("- ").unwrap_or(t).trim_start();
    let t = t.trim_start_matches(MARKS).strip_prefix("Status")?;
    let t = t.trim_start_matches(MARKS).strip_prefix(':')?;
    Some(t.trim_start_matches(|c: char| MARKS.contains(&c) || c.is_whitespace()))
}

/// The status the builder reported, or `None` when it reported none or an unknown one.
/// A first line `BLOCKED:` wins. Else the first `Status:` line outside code fences with a known
/// value counts (see `status_value` for the accepted forms); a line with an unknown value is skipped.
/// The text is the rest of that line and the following unfenced lines up to the first `Ruling:`
/// line or commit note marker.
pub fn parse_builder_status(reply: &str) -> Option<BuilderStatus> {
    if let Some(reason) = blocked_reason(reply) {
        return Some(BuilderStatus::Blocked(reason));
    }
    let lines: Vec<&str> = unfenced_lines(reply).collect();
    for (at, line) in lines.iter().enumerate() {
        let Some(value) = status_value(line) else {
            continue;
        };
        let word_end = value
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(value.len());
        let (word, tail) = value.split_at(word_end);
        let make: fn(String) -> BuilderStatus = match word.to_ascii_uppercase().as_str() {
            "DONE" => |_| BuilderStatus::Done,
            "DONE_WITH_CONCERNS" => BuilderStatus::DoneWithConcerns,
            "BLOCKED" => BuilderStatus::Blocked,
            "NEEDS_CONTEXT" => BuilderStatus::NeedsContext,
            _ => continue,
        };
        let mut text = tail
            .trim_start_matches(|c: char| {
                c == '*' || c == '`' || c == ':' || c == '-' || c.is_whitespace()
            })
            .to_string();
        for rest in lines[at + 1..]
            .iter()
            .take_while(|l| !l.trim().starts_with("Ruling:") && l.trim() != COMMIT_BEGIN)
        {
            text.push('\n');
            text.push_str(rest);
        }
        return Some(make(text.trim().to_string()));
    }
    None
}

/// Like `parse_builder_status`, with `Done` when the reply has no usable status line.
pub fn builder_status(reply: &str) -> BuilderStatus {
    parse_builder_status(reply).unwrap_or(BuilderStatus::Done)
}

/// The bodies of the `Ruling:` lines outside code fences, trimmed, empty bodies dropped.
pub fn builder_rulings(reply: &str) -> Vec<String> {
    unfenced_lines(reply)
        .filter_map(|line| line.trim().strip_prefix("Ruling:"))
        .map(|body| body.trim().to_string())
        .filter(|body| !body.is_empty())
        .collect()
}

pub const COMMIT_BEGIN: &str = "---BEGIN COMMIT---";
pub const COMMIT_END: &str = "---END COMMIT---";
/// The most characters a job commit subject may have.
pub const MAX_JOB_SUBJECT_CHARS: usize = 50;
const NOTE_WIDTH: usize = 72;
const FALLBACK_SUBJECT: &str = "Factory task";
const FALLBACK_MAX_FILES: usize = 20;

/// The builder's commit note: a subject and a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitNote {
    pub subject: String,
    pub body: String,
}

impl CommitNote {
    /// The note as a file: subject, a blank line, the body and a final newline.
    pub fn to_file_text(&self) -> String {
        format!("{}\n\n{}\n", self.subject, self.body)
    }

    /// Reads a note back from `to_file_text`. `None` when the subject is empty.
    pub fn from_file_text(text: &str) -> Option<CommitNote> {
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        let subject = first.trim();
        if subject.is_empty() {
            return None;
        }
        Some(CommitNote {
            subject: subject.to_string(),
            body: rest.trim().to_string(),
        })
    }
}

/// The last commit note in the reply, fenced or not. `None` without a marker pair or a subject.
pub fn parse_commit_note(reply: &str) -> Option<CommitNote> {
    for (at, _) in reply.rmatch_indices(COMMIT_BEGIN) {
        let rest = &reply[at + COMMIT_BEGIN.len()..];
        let Some(end) = rest.find(COMMIT_END) else {
            continue;
        };
        let mut lines = rest[..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("```"))
            .skip_while(|l| l.trim().is_empty());
        let subject = lines.next()?.trim();
        if subject.is_empty() {
            return None;
        }
        let body = lines.collect::<Vec<_>>().join("\n");
        return Some(CommitNote {
            subject: subject.to_string(),
            body: body.trim().to_string(),
        });
    }
    None
}

/// One part of a note body: a prose run or a bullet, both as single-line text.
enum NotePart {
    Prose(String),
    Bullet(String),
}

fn bullet_text(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
}

/// The paragraphs of a body, each as its prose run (if any) followed by its bullets.
fn note_parts(body: &str) -> Vec<Vec<NotePart>> {
    let mut paragraphs: Vec<Vec<NotePart>> = Vec::new();
    let mut current: Vec<NotePart> = Vec::new();
    let mut lines: Vec<&str> = body.lines().collect();
    lines.push("");
    for line in lines {
        if line.trim().is_empty() {
            // A bare `- ` line with no continuation is an empty bullet and is skipped.
            current.retain(|p| !matches!(p, NotePart::Bullet(s) if s.is_empty()));
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            continue;
        }
        let fold = |l: &str| l.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(t) = bullet_text(line) {
            current.push(NotePart::Bullet(fold(t)));
            continue;
        }
        match current.last_mut() {
            Some(NotePart::Prose(s) | NotePart::Bullet(s)) => {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&fold(line));
            }
            None => current.push(NotePart::Prose(fold(line))),
        }
    }
    paragraphs
}

/// The bullets of a note body as single-line strings without their marker. Prose is skipped.
pub fn note_bullets(body: &str) -> Vec<String> {
    note_parts(body)
        .into_iter()
        .flatten()
        .filter_map(|p| match p {
            NotePart::Bullet(s) => Some(s),
            NotePart::Prose(_) => None,
        })
        .collect()
}

/// Folds whitespace and wraps on word boundaries. A word longer than `width` keeps its own line.
pub fn wrap_paragraph(text: &str, width: usize) -> String {
    wrap_words(text, width, "", "")
}

/// Wraps one bullet. The result starts with `- ` and continuation lines with two spaces.
pub fn wrap_bullet(text: &str, width: usize) -> String {
    let t = text.trim_start();
    let t = t.strip_prefix("- ").unwrap_or(t);
    wrap_words(t, width, "- ", "  ")
}

fn wrap_words(text: &str, width: usize, first: &str, rest: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut line = first.to_string();
    let mut empty = true;
    for word in text.split_whitespace() {
        let fits = line.chars().count() + 1 + word.chars().count() <= width;
        if !empty && !fits {
            lines.push(std::mem::replace(&mut line, rest.to_string()));
            empty = true;
        }
        if !empty {
            line.push(' ');
        }
        line.push_str(word);
        empty = false;
    }
    lines.push(line);
    lines.join("\n")
}

/// Checks a note against the job message rules and rewraps its body at 72 characters.
pub fn check_note(note: &CommitNote) -> Result<CommitNote, String> {
    let subject = note.subject.trim();
    let n = subject.chars().count();
    if subject.is_empty() {
        return Err("the subject is empty".to_string());
    }
    if n > MAX_JOB_SUBJECT_CHARS {
        return Err(format!(
            "the subject is {n} characters, the limit is {MAX_JOB_SUBJECT_CHARS}"
        ));
    }
    if subject.ends_with('.') {
        return Err("the subject ends with a period".to_string());
    }
    if note.body.trim().is_empty() {
        return Err("the body is empty".to_string());
    }
    let paragraphs: Vec<String> = note_parts(&note.body)
        .into_iter()
        .map(|parts| {
            parts
                .iter()
                .map(|p| match p {
                    NotePart::Prose(s) => wrap_paragraph(s, NOTE_WIDTH),
                    NotePart::Bullet(s) => wrap_bullet(s, NOTE_WIDTH),
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    if paragraphs.is_empty() {
        return Err("the body is empty".to_string());
    }
    Ok(CommitNote {
        subject: subject.to_string(),
        body: paragraphs.join("\n\n"),
    })
}

/// Folds whitespace and cuts the text to at most `limit` characters at a word boundary, or at
/// the limit when one word is longer. Trailing whitespace and periods are dropped. An empty
/// result means the caller should use its default.
fn cut_text(text: &str, limit: usize) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = folded.chars().collect();
    let cut: String = if chars.len() <= limit {
        folded
    } else {
        let head: String = chars[..limit].iter().collect();
        if chars[limit] == ' ' {
            head
        } else {
            match head.rfind(' ') {
                Some(i) => head[..i].to_string(),
                None => head,
            }
        }
    };
    cut.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string()
}

/// A note built from the task title and the changed files, for when the builder wrote none.
pub fn fallback_note(task_title: &str, files: &[String]) -> CommitNote {
    let mut subject = cut_text(task_title, MAX_JOB_SUBJECT_CHARS);
    if subject.is_empty() {
        subject = FALLBACK_SUBJECT.to_string();
    }
    let mut lines: Vec<String> = files
        .iter()
        .take(FALLBACK_MAX_FILES)
        .map(|f| format!("- {f}"))
        .collect();
    if files.len() > FALLBACK_MAX_FILES {
        lines.push(format!(
            "- and {} more files",
            files.len() - FALLBACK_MAX_FILES
        ));
    }
    if lines.is_empty() {
        lines.push("- no files changed".to_string());
    }
    CommitNote {
        subject,
        body: lines.join("\n"),
    }
}

/// The most bullets a job message holds across its task sections and review fixes.
pub const MAX_JOB_BULLETS: usize = 60;

/// Checks a job subject: the shared message rules plus the 50 character job limit.
pub fn check_job_subject(subject: &str) -> Result<(), String> {
    validate_message(subject)?;
    let n = subject.chars().count();
    if n > MAX_JOB_SUBJECT_CHARS {
        return Err(format!(
            "the subject is {n} characters, the limit is {MAX_JOB_SUBJECT_CHARS}"
        ));
    }
    Ok(())
}

/// The job subject taken from a plan title: `feat: <title>` without the plan suffix, links and
/// issue refs, cut to fit 50 characters. An empty result becomes `feat: factory job`.
pub fn title_subject(title: &str) -> String {
    let mut words: Vec<&str> = title.split_whitespace().collect();
    let n = words.len();
    if n >= 2
        && words[n - 2].eq_ignore_ascii_case("implementation")
        && words[n - 1].eq_ignore_ascii_case("plan")
    {
        words.truncate(n - 2);
    }
    let kept: Vec<&str> = words
        .into_iter()
        .filter(|w| {
            !(w.starts_with("http://") || w.starts_with("https://"))
                && parse_issue_refs(w).is_empty()
        })
        .collect();
    let joined = kept.join(" ");
    let mut chars = joined.chars();
    let text: String = match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    };
    // The 6 characters of `feat: ` come out of the 50.
    let cut = cut_text(&text, MAX_JOB_SUBJECT_CHARS - 6);
    if cut.is_empty() {
        "feat: factory job".to_string()
    } else {
        format!("feat: {cut}")
    }
}

/// One task section of a job message: a title and the bullets of its commit note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSection {
    pub title: String,
    pub bullets: Vec<String>,
}

/// The parts of a job message.
#[derive(Debug, Clone)]
pub struct JobMessage<'a> {
    pub subject: &'a str,
    pub why: Option<&'a str>,
    pub tasks: &'a [NoteSection],
    pub fixes: &'a [String],
    pub footers: &'a [String],
}

/// Builds the job message: subject, why paragraph, one section per task, the review fixes and
/// the footers. The body holds at most 60 bullets, the rest becomes one `- and N more changes`
/// under the last block that has bullets. A footer appears once, and not at all when a body line
/// equals it.
pub fn build_job_message(m: &JobMessage<'_>) -> String {
    let mut left = MAX_JOB_BULLETS;
    let mut dropped = 0usize;
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut take = |bullets: &[String], lines: &mut Vec<String>| {
        let kept = bullets.len().min(left);
        left -= kept;
        dropped += bullets.len() - kept;
        lines.extend(bullets[..kept].iter().map(|b| wrap_bullet(b, NOTE_WIDTH)));
        kept
    };
    for section in m.tasks {
        let mut lines = vec![format!("{}:", section.title)];
        let kept = take(&section.bullets, &mut lines);
        if kept > 0 || section.bullets.is_empty() {
            blocks.push(lines);
        }
    }
    if !m.fixes.is_empty() {
        let mut lines = vec!["Review fixes:".to_string()];
        if take(m.fixes, &mut lines) > 0 {
            blocks.push(lines);
        }
    }
    if dropped > 0 {
        let more = format!("- and {dropped} more changes");
        match blocks
            .iter_mut()
            .rev()
            .find(|b| b.iter().any(|l| l.starts_with("- ")))
        {
            Some(block) => block.push(more),
            None => blocks.push(vec![more]),
        }
    }
    let mut paragraphs: Vec<String> = vec![m.subject.to_string()];
    if let Some(why) = m.why.filter(|w| !w.trim().is_empty()) {
        paragraphs.push(wrap_paragraph(why, NOTE_WIDTH));
    }
    paragraphs.extend(blocks.into_iter().map(|b| b.join("\n")));
    let mut out = paragraphs.join("\n\n");
    let mut footers: Vec<&str> = Vec::new();
    for f in m.footers.iter().map(|f| f.as_str()) {
        if !footers.contains(&f) && !out.lines().any(|l| l == f) {
            footers.push(f);
        }
    }
    if !footers.is_empty() {
        out.push_str("\n\n");
        out.push_str(&footers.join("\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_scoped_and_breaking_subjects() {
        assert!(validate_message("feat: add a thing").is_ok());
        assert!(validate_message("fix(infra): stop the leak").is_ok());
        assert!(validate_message("feat(x)!: drop the old flag").is_ok());
        assert!(validate_message("chore: tidy\n\nBody paragraph.\nMore body.").is_ok());
    }

    #[test]
    fn subject_length_limit_is_72_characters() {
        let ok = format!("feat: {}", "a".repeat(72 - 6));
        assert_eq!(ok.chars().count(), 72);
        assert!(validate_message(&ok).is_ok());
        let long = format!("feat: {}", "a".repeat(72 - 5));
        assert_eq!(long.chars().count(), 73);
        assert!(validate_message(&long).is_err());
    }

    #[test]
    fn rejects_bad_subjects() {
        for bad in [
            "",
            "Added a thing",
            "feat add a thing",
            "feat:add a thing",
            "Feat: add a thing",
            "wip: add a thing",
            "feat(): add a thing",
            "feat(two words): add a thing",
            "feat(x: add a thing",
            "feat: ",
            "feat: add a thing.",
        ] {
            assert!(validate_message(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn a_body_needs_a_blank_line_after_the_subject() {
        assert!(validate_message("feat: add a thing\nbody right away").is_err());
        assert!(validate_message("feat: add a thing\n\nbody after a blank line").is_ok());
    }

    #[test]
    fn counts_characters_and_not_bytes() {
        let subject = format!("feat: {}", "é".repeat(66));
        assert_eq!(subject.chars().count(), 72);
        assert!(validate_message(&subject).is_ok());
    }

    #[test]
    fn rejects_a_subject_ending_with_whitespace() {
        assert!(validate_message("feat: x ").is_err());
    }

    #[test]
    fn blocked_reason_reads_only_the_first_line() {
        assert_eq!(
            blocked_reason("BLOCKED: no skill\nmore"),
            Some("no skill".to_string())
        );
        assert_eq!(
            blocked_reason("  BLOCKED:   spaced  "),
            Some("spaced".to_string())
        );
        assert_eq!(blocked_reason("abc1234 feat: x\nBLOCKED: later line"), None);
        assert_eq!(blocked_reason(""), None);
    }

    #[test]
    fn builder_status_reads_a_first_line_blocked() {
        assert_eq!(
            builder_status("BLOCKED: no schema\nmore"),
            BuilderStatus::Blocked("no schema".to_string())
        );
    }

    #[test]
    fn builder_status_reads_a_status_line_in_a_longer_reply() {
        let reply =
            "I did the work.\n\n- **Status:** NEEDS_CONTEXT the API is unclear\nWhich endpoint?";
        assert_eq!(
            builder_status(reply),
            BuilderStatus::NeedsContext("the API is unclear\nWhich endpoint?".to_string())
        );
        assert_eq!(
            builder_status("ok\nStatus: DONE_WITH_CONCERNS\nslow"),
            BuilderStatus::DoneWithConcerns("slow".to_string())
        );
        assert_eq!(
            builder_status("ok\nStatus: BLOCKED: no db"),
            BuilderStatus::Blocked("no db".to_string())
        );
        assert_eq!(builder_status("Status: DONE"), BuilderStatus::Done);
    }

    #[test]
    fn builder_status_ignores_blocked_inside_a_fence() {
        let reply = "Done.\n```\nBLOCKED: quoted\nStatus: BLOCKED\n```\nThe text above is a sample.\nBLOCKED: mid text";
        assert_eq!(builder_status(reply), BuilderStatus::Done);
        assert_eq!(parse_builder_status(reply), None);
    }

    #[test]
    fn builder_status_defaults_to_done_without_a_status_line() {
        assert_eq!(builder_status(""), BuilderStatus::Done);
        assert_eq!(builder_status("all good"), BuilderStatus::Done);
        assert_eq!(parse_builder_status("all good"), None);
        assert_eq!(builder_status("Status: MAYBE"), BuilderStatus::Done);
        assert_eq!(parse_builder_status("Status: MAYBE"), None);
    }

    #[test]
    fn builder_status_accepts_marked_up_forms() {
        for reply in [
            "Status: **BLOCKED**",
            "Status: `BLOCKED`",
            "**Status**: BLOCKED",
            "**Status: BLOCKED**",
            "- **Status:** BLOCKED",
            "`Status: BLOCKED`",
        ] {
            assert_eq!(
                parse_builder_status(reply),
                Some(BuilderStatus::Blocked(String::new())),
                "{reply}"
            );
        }
        assert_eq!(
            parse_builder_status("ok\n**Status**: `NEEDS_CONTEXT` which table?"),
            Some(BuilderStatus::NeedsContext("which table?".to_string()))
        );
        assert_eq!(
            parse_builder_status("**Status: DONE_WITH_CONCERNS**\nslow"),
            Some(BuilderStatus::DoneWithConcerns("slow".to_string()))
        );
    }

    #[test]
    fn builder_status_skips_an_unknown_value_and_keeps_searching() {
        let reply = "Status: tests green\nwork done\nStatus: BLOCKED: no db";
        assert_eq!(
            parse_builder_status(reply),
            Some(BuilderStatus::Blocked("no db".to_string()))
        );
        assert_eq!(
            parse_builder_status("Status: tests green\nStatus: maybe"),
            None
        );
    }

    #[test]
    fn concern_text_stops_at_the_first_ruling_line_and_skips_fences() {
        let reply = "Status: DONE_WITH_CONCERNS\nThe cap is tight.\n```\nquoted\n```\nIt may be slow.\n\
                     Ruling: kept the name \u{2014} the plan says so \u{2014} a rename later\nRuling: second \u{2014} why \u{2014} cost";
        assert_eq!(
            parse_builder_status(reply),
            Some(BuilderStatus::DoneWithConcerns(
                "The cap is tight.\nIt may be slow.".to_string()
            ))
        );
        assert_eq!(builder_rulings(reply).len(), 2);
    }

    #[test]
    fn a_blocked_reason_stops_at_the_first_ruling_line() {
        let reply = "Status: BLOCKED\nThe schema file is gone.\nRuling: stopped \u{2014} nothing to build on \u{2014} a retry";
        assert_eq!(
            parse_builder_status(reply),
            Some(BuilderStatus::Blocked(
                "The schema file is gone.".to_string()
            ))
        );
        assert_eq!(
            builder_rulings(reply),
            vec!["stopped \u{2014} nothing to build on \u{2014} a retry".to_string()]
        );
        let reply = "Status: NEEDS_CONTEXT which table?\nRuling: a \u{2014} b \u{2014} c";
        assert_eq!(
            parse_builder_status(reply),
            Some(BuilderStatus::NeedsContext("which table?".to_string()))
        );
    }

    #[test]
    fn builder_rulings_strips_prefix_and_drops_empty_bodies() {
        let reply = "x\nRuling: kept the old name\n  Ruling:   \nRuling:\n```\nRuling: fenced\n```\nRuling: second one  ";
        assert_eq!(
            builder_rulings(reply),
            vec!["kept the old name".to_string(), "second one".to_string()]
        );
    }

    #[test]
    fn refs_read_hash_numbers_and_urls() {
        let refs =
            parse_issue_refs("fix #20 and https://github.com/Snehal1112/rocket/issues/76 now");
        assert_eq!(
            refs,
            vec![
                IssueRef {
                    number: 20,
                    repo: None
                },
                IssueRef {
                    number: 76,
                    repo: Some("Snehal1112/rocket".to_string())
                },
            ]
        );
    }

    #[test]
    fn url_followed_by_punctuation() {
        let refs = parse_issue_refs("see (https://github.com/o/r/issues/76).");
        assert_eq!(
            refs,
            vec![IssueRef {
                number: 76,
                repo: Some("o/r".to_string())
            }]
        );
        let refs = parse_issue_refs("https://github.com/o/r/issues/76.");
        assert_eq!(
            refs,
            vec![IssueRef {
                number: 76,
                repo: Some("o/r".to_string())
            }]
        );
    }

    #[test]
    fn url_with_fragment_counts_once() {
        let refs = parse_issue_refs("https://github.com/o/r/issues/76#issuecomment-5");
        assert_eq!(
            refs,
            vec![IssueRef {
                number: 76,
                repo: Some("o/r".to_string())
            }]
        );
    }

    #[test]
    fn hash_ref_needs_a_boundary_after_the_digits() {
        assert!(parse_issue_refs("fix #1a2b3c now").is_empty());
        assert!(parse_issue_refs("see #123abc").is_empty());
        assert!(parse_issue_refs("see #12_x").is_empty());
    }

    #[test]
    fn hash_ref_may_end_at_punctuation() {
        for text in ["#12.", "(#12)", "#12,", "[#12]", "done #12", "#12"] {
            let refs = parse_issue_refs(text);
            assert_eq!(
                refs,
                vec![IssueRef {
                    number: 12,
                    repo: None
                }],
                "{text}"
            );
        }
    }

    #[test]
    fn hash_ref_alone_or_zero_is_not_a_ref() {
        for text in ["#", "#0", "x #", "# 12"] {
            assert!(parse_issue_refs(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn owner_repo_shorthand_carries_the_repo() {
        let refs = parse_issue_refs("fix other/repo#5 later");
        assert_eq!(
            refs,
            vec![IssueRef {
                number: 5,
                repo: Some("other/repo".to_string())
            }]
        );
        let refs = parse_issue_refs("Closes o/r#7.");
        assert_eq!(
            refs,
            vec![IssueRef {
                number: 7,
                repo: Some("o/r".to_string())
            }]
        );
    }

    #[test]
    fn shorthand_inside_a_longer_path_is_not_a_ref() {
        assert!(parse_issue_refs("x.com/a/b#12").is_empty());
        assert!(parse_issue_refs("/a/b#12").is_empty());
    }

    #[test]
    fn parse_issues_ignores_a_shorthand_ref_to_another_repository() {
        assert!(parse_issues("fix other/repo#5 later").is_empty());
        assert_eq!(parse_issues("fix other/repo#5 and #7"), vec![7]);
        assert_eq!(parse_issues("see https://github.com/o/r/issues/9"), vec![9]);
    }

    #[test]
    fn pull_request_url_is_not_an_issue() {
        assert!(parse_issue_refs("https://github.com/o/r/pull/9").is_empty());
        assert!(parse_issue_refs("https://example.com/o/r/issues/9").is_empty());
    }

    #[test]
    fn refs_are_sorted_and_deduplicated() {
        let refs = parse_issue_refs("#9 #3 #9 https://github.com/o/r/issues/3");
        assert_eq!(
            refs,
            vec![
                IssueRef {
                    number: 3,
                    repo: None
                },
                IssueRef {
                    number: 3,
                    repo: Some("o/r".to_string())
                },
                IssueRef {
                    number: 9,
                    repo: None
                },
            ]
        );
    }

    #[test]
    fn parse_issues_keeps_hash_behavior_and_adds_url_numbers() {
        assert_eq!(parse_issues("fix #20, #21 and #20"), vec![20, 21]);
        assert_eq!(parse_issues("no issues here"), Vec::<u32>::new());
        assert_eq!(parse_issues("abc#5 and #0 and #x"), Vec::<u32>::new());
        assert_eq!(
            parse_issues("https://github.com/o/r/issues/76 and #5"),
            vec![5, 76]
        );
    }

    #[test]
    fn refs_skip_inline_code_spans() {
        let refs = parse_issue_refs("fix `#42` and #7");
        assert_eq!(
            refs,
            vec![IssueRef {
                number: 7,
                repo: None
            }]
        );
    }

    #[test]
    fn refs_skip_fenced_blocks() {
        let text = "text #7\n```\n#42\nhttps://github.com/o/r/issues/5\n```\n";
        assert_eq!(
            parse_issue_refs(text),
            vec![IssueRef {
                number: 7,
                repo: None
            }]
        );
    }

    #[test]
    fn refs_skip_an_unclosed_fence() {
        assert_eq!(
            parse_issue_refs("#7\n```\n#42\n"),
            vec![IssueRef {
                number: 7,
                repo: None
            }]
        );
    }

    #[test]
    fn a_lone_backtick_does_not_hide_a_ref() {
        assert_eq!(
            parse_issue_refs("a ` b #7"),
            vec![IssueRef {
                number: 7,
                repo: None
            }]
        );
    }

    #[test]
    fn urls_and_shorthand_in_code_are_not_refs() {
        let text = "`o/r#5` and `https://github.com/o/r/issues/6`";
        assert!(parse_issue_refs(text).is_empty());
    }

    #[test]
    fn parse_issues_skips_code_too() {
        assert_eq!(parse_issues("Closes `#42`, #7"), vec![7]);
    }

    #[test]
    fn parse_issues_reads_hash_numbers_only() {
        assert_eq!(
            parse_issues("GitHub issue #20 (https://github.com/o/r/issues/20)"),
            vec![20]
        );
        assert_eq!(parse_issues("fix #21, #20 and #21"), vec![20, 21]);
        assert_eq!(
            parse_issues("C# and a#1 and #abc and #0 and #99999999999"),
            Vec::<u32>::new()
        );
        assert_eq!(parse_issues("(#7)"), vec![7]);
        assert_eq!(parse_issues("#3\n#4"), vec![3, 4]);
    }

    #[test]
    fn footers_are_built_and_checked() {
        assert_eq!(issue_footer("Relates to", &[]), None);
        assert_eq!(
            issue_footer("Fixes", &[20, 21]).as_deref(),
            Some("Fixes: #20, #21")
        );
        assert!(has_footer("feat: x", "Relates to", &[]));
        assert!(has_footer(
            "feat: x\n\nRelates to: #20",
            "Relates to",
            &[20]
        ));
        assert!(!has_footer(
            "feat: x\n\nRelates to: #20",
            "Relates to",
            &[20, 21]
        ));
        assert!(!has_footer("feat: x", "Relates to", &[20]));
    }

    #[test]
    fn append_footers_adds_each_line_once() {
        let lines = vec!["Fixes: #20".to_string(), "Factory-Job: 001".to_string()];
        assert_eq!(
            append_footers("feat: x\n", &lines),
            "feat: x\n\nFixes: #20\nFactory-Job: 001"
        );
        assert_eq!(
            append_footers("feat: x\n\nFixes: #20", &lines),
            "feat: x\n\nFixes: #20\nFactory-Job: 001"
        );
        assert_eq!(append_footers("feat: x", &[]), "feat: x");
    }

    #[test]
    fn append_footers_joins_an_existing_footer_block() {
        let lines = vec!["Fixes: #20".to_string()];
        assert_eq!(
            append_footers("feat: x\n\nbody\n\nSigned-off-by: A", &lines),
            "feat: x\n\nbody\n\nSigned-off-by: A\nFixes: #20"
        );
    }

    fn note(subject: &str, body: &str) -> CommitNote {
        CommitNote {
            subject: subject.to_string(),
            body: body.to_string(),
        }
    }

    const NOTE_REPLY: &str = "Status: DONE\nRuling: a — b — c\n---BEGIN COMMIT---\nAdd a retry endpoint\n\nWhy it matters.\n\n- Add POST retry\n---END COMMIT---\n";

    #[test]
    fn a_note_is_parsed_from_the_end_of_a_reply() {
        let got = parse_commit_note(NOTE_REPLY).expect("a note");
        assert_eq!(got.subject, "Add a retry endpoint");
        assert_eq!(got.body, "Why it matters.\n\n- Add POST retry");
    }

    #[test]
    fn a_missing_note_is_none() {
        assert_eq!(parse_commit_note("Status: DONE"), None);
        assert_eq!(parse_commit_note("---BEGIN COMMIT---\nAdd x\n"), None);
        assert_eq!(
            parse_commit_note("---BEGIN COMMIT---\n\n---END COMMIT---"),
            None
        );
    }

    #[test]
    fn a_fenced_note_is_accepted() {
        let fenced = format!("```text\n{NOTE_REPLY}```\n");
        assert_eq!(parse_commit_note(&fenced), parse_commit_note(NOTE_REPLY));
        assert!(parse_commit_note(&fenced).is_some());
    }

    #[test]
    fn the_last_note_wins() {
        let reply = "---BEGIN COMMIT---\nFirst\n\n- a\n---END COMMIT---\n---BEGIN COMMIT---\nSecond\n\n- b\n---END COMMIT---";
        assert_eq!(parse_commit_note(reply).expect("a note").subject, "Second");
    }

    #[test]
    fn check_note_refuses_each_rule() {
        let err = |n: CommitNote| check_note(&n).expect_err("refused");
        assert_eq!(err(note("", "- a")), "the subject is empty");
        assert_eq!(
            err(note(&"a".repeat(51), "- a")),
            "the subject is 51 characters, the limit is 50"
        );
        assert_eq!(err(note("Add x.", "- a")), "the subject ends with a period");
        assert_eq!(err(note("Add x", "")), "the body is empty");
        assert!(check_note(&note(&"a".repeat(50), "- a")).is_ok());
    }

    #[test]
    fn check_note_rewraps_long_bullets() {
        let long = format!("- {}", vec!["word"; 20].join(" "));
        let got = check_note(&note("Add x", &long)).expect("accepted");
        let lines: Vec<&str> = got.body.lines().collect();
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.chars().count() <= 72));
        assert!(lines[0].starts_with("- "));
        assert!(lines[1..].iter().all(|l| l.starts_with("  ")));
        let star = check_note(&note("Add x", "* one\n  more")).expect("accepted");
        assert_eq!(star.body, "- one more");
    }

    #[test]
    fn note_bullets_joins_continuations_and_skips_prose() {
        assert_eq!(
            note_bullets("Prose line.\n\n- one\n  more\n- two"),
            vec!["one more", "two"]
        );
        assert_eq!(note_bullets("- one\nmore\n* two"), vec!["one more", "two"]);
    }

    #[test]
    fn empty_bullets_are_skipped() {
        assert_eq!(note_bullets("- one\n- \n- two"), vec!["one", "two"]);
        let got = check_note(&note("Add x", "- one\n- \n- two")).expect("accepted");
        assert_eq!(got.body, "- one\n- two");
        assert_eq!(
            check_note(&note("Add x", "- ")).expect_err("refused"),
            "the body is empty"
        );
    }

    #[test]
    fn a_note_round_trips_through_its_file_text() {
        let n = note("Add x", "Why.\n\n- one\n  more\n- two");
        assert_eq!(CommitNote::from_file_text(&n.to_file_text()), Some(n));
        assert_eq!(CommitNote::from_file_text("\n\nbody"), None);
    }

    #[test]
    fn fallback_note_lists_twenty_files_then_a_count() {
        let files: Vec<String> = (0..23).map(|i| format!("f{i}.rs")).collect();
        let got = fallback_note("Add x", &files);
        let lines: Vec<&str> = got.body.lines().collect();
        assert_eq!(lines.len(), 21);
        assert_eq!(lines[0], "- f0.rs");
        assert_eq!(lines[19], "- f19.rs");
        assert_eq!(lines[20], "- and 3 more files");
        assert_eq!(fallback_note("Add x", &[]).body, "- no files changed");
        let title = "word ".repeat(14);
        let cut = fallback_note(&title, &[]).subject;
        assert!(cut.chars().count() <= 50);
        assert!(cut.split(' ').all(|w| w == "word"));
        let one = fallback_note(&"a".repeat(60), &[]).subject;
        assert_eq!(one.chars().count(), 50);
        assert_eq!(fallback_note("...", &[]).subject, "Factory task");
    }

    #[test]
    fn cut_text_handles_long_words_and_periods() {
        assert_eq!(cut_text("Fix A. Then B", 6), "Fix A");
        assert_eq!(cut_text(&"b".repeat(60), 50), "b".repeat(50));
        assert_eq!(cut_text("...", 10), "");
    }

    #[test]
    fn the_status_text_stops_at_the_commit_note() {
        let reply = "Status: DONE_WITH_CONCERNS\nslow test\n---BEGIN COMMIT---\nAdd x\n\n- y\n---END COMMIT---";
        assert_eq!(
            parse_builder_status(reply),
            Some(BuilderStatus::DoneWithConcerns("slow test".to_string()))
        );
    }

    fn sample_sections() -> Vec<NoteSection> {
        vec![
            NoteSection {
                title: "Add the endpoint".to_string(),
                bullets: vec!["a".to_string(), "b".to_string()],
            },
            NoteSection {
                title: "Show the button".to_string(),
                bullets: vec!["c".to_string()],
            },
        ]
    }

    fn sample_message() -> String {
        let tasks = sample_sections();
        let fixes = vec!["Refuse approve while a retry is queued".to_string()];
        let footers = vec!["Fixes: #142".to_string(), "Factory-Job: 042".to_string()];
        build_job_message(&JobMessage {
            subject: "feat(retry): retry a parked job",
            why: Some("The dashboard could not continue a parked job."),
            tasks: &tasks,
            fixes: &fixes,
            footers: &footers,
        })
    }

    fn msg_with_footers(footers: &[&str]) -> String {
        let tasks = sample_sections();
        let fixes = vec!["Refuse approve while a retry is queued".to_string()];
        let footers: Vec<String> = footers.iter().map(|f| f.to_string()).collect();
        build_job_message(&JobMessage {
            subject: "feat(retry): retry a parked job",
            why: Some("The dashboard could not continue a parked job."),
            tasks: &tasks,
            fixes: &fixes,
            footers: &footers,
        })
    }

    #[test]
    fn a_repeated_footer_appears_once() {
        let msg = msg_with_footers(&["Fixes: #142", "Factory-Job: 042", "Fixes: #142"]);
        assert_eq!(msg.matches("Fixes: #142").count(), 1, "{msg}");
        assert!(msg.ends_with("Fixes: #142\nFactory-Job: 042"), "{msg}");
    }

    #[test]
    fn a_footer_equal_to_a_body_line_is_left_out() {
        let msg = msg_with_footers(&["- a", "Factory-Job: 042"]);
        assert_eq!(msg.matches("- a").count(), 1, "{msg}");
        assert!(
            msg.ends_with("- Refuse approve while a retry is queued\n\nFactory-Job: 042"),
            "{msg}"
        );
    }

    #[test]
    fn and_n_more_goes_under_the_last_section_with_bullets() {
        let big = NoteSection {
            title: "Big".to_string(),
            bullets: (0..61).map(|i| format!("b{i}")).collect(),
        };
        let later = NoteSection {
            title: "Later".to_string(),
            bullets: Vec::new(),
        };
        let msg = build_job_message(&JobMessage {
            subject: "feat: x",
            why: None,
            tasks: &[big, later],
            fixes: &[],
            footers: &[],
        });
        assert!(
            msg.contains("- b59\n- and 1 more changes\n\nLater:"),
            "{msg}"
        );
        assert!(msg.ends_with("Later:"), "{msg}");
    }

    #[test]
    fn the_job_message_has_subject_why_sections_fixes_and_footers() {
        let expected = "feat(retry): retry a parked job\n\nThe dashboard could not continue a parked job.\n\nAdd the endpoint:\n- a\n- b\n\nShow the button:\n- c\n\nReview fixes:\n- Refuse approve while a retry is queued\n\nFixes: #142\nFactory-Job: 042";
        assert_eq!(sample_message(), expected);
    }

    #[test]
    fn a_message_without_why_or_fixes_leaves_them_out() {
        let tasks = sample_sections();
        let msg = build_job_message(&JobMessage {
            subject: "feat: x",
            why: Some("  "),
            tasks: &tasks,
            fixes: &[],
            footers: &[],
        });
        assert_eq!(
            msg,
            "feat: x\n\nAdd the endpoint:\n- a\n- b\n\nShow the button:\n- c"
        );
        assert!(!msg.contains("Review fixes:"));
    }

    #[test]
    fn the_job_message_passes_validate_message() {
        assert_eq!(validate_message(&sample_message()), Ok(()));
    }

    #[test]
    fn the_body_is_capped_at_sixty_bullets() {
        let tasks: Vec<NoteSection> = (0..3)
            .map(|i| NoteSection {
                title: format!("Task {i}"),
                bullets: (0..25).map(|j| format!("item {i} {j}")).collect(),
            })
            .collect();
        let msg = build_job_message(&JobMessage {
            subject: "feat: x",
            why: None,
            tasks: &tasks,
            fixes: &[],
            footers: &[],
        });
        let bullets = msg.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(bullets, 61);
        assert!(msg.ends_with("- item 2 9\n- and 15 more changes"));
    }

    #[test]
    fn a_section_with_no_bullet_left_after_the_cap_is_left_out() {
        let tasks = vec![NoteSection {
            title: "Big".to_string(),
            bullets: (0..60).map(|j| format!("b{j}")).collect(),
        }];
        let fixes = vec!["late fix".to_string()];
        let msg = build_job_message(&JobMessage {
            subject: "feat: x",
            why: None,
            tasks: &tasks,
            fixes: &fixes,
            footers: &[],
        });
        assert!(!msg.contains("Review fixes:"));
        assert!(msg.ends_with("- b59\n- and 1 more changes"));
    }

    #[test]
    fn check_job_subject_limits_fifty_characters() {
        let ok = format!("feat: {}", "a".repeat(44));
        assert_eq!(ok.chars().count(), 50);
        assert_eq!(check_job_subject(&ok), Ok(()));
        let long = format!("feat: {}", "a".repeat(45));
        assert_eq!(
            check_job_subject(&long),
            Err("the subject is 51 characters, the limit is 50".to_string())
        );
        assert!(check_job_subject("Add x").is_err());
        assert!(check_job_subject("feat: x.").is_err());
    }

    #[test]
    fn title_subject_strips_the_plan_suffix_and_issue_refs() {
        assert_eq!(
            title_subject("Retry at the parked stage Implementation Plan"),
            "feat: retry at the parked stage"
        );
        assert_eq!(
            title_subject("Fix #12 crash https://github.com/o/r/issues/12"),
            "feat: fix crash"
        );
        assert_eq!(title_subject(""), "feat: factory job");
    }

    #[test]
    fn title_subject_cuts_on_a_word_boundary_with_multibyte_text() {
        let title =
            "Résumé é handling — across the whole pipeline é and — every other stage of the run";
        let subject = title_subject(title);
        assert!(subject.chars().count() <= 50);
        assert_eq!(check_job_subject(&subject), Ok(()));
        let text = subject.strip_prefix("feat: ").expect("prefix");
        let lowered = title.replacen('R', "r", 1);
        assert!(lowered.starts_with(text));
        assert!(lowered[text.len()..].starts_with(' '));
        let long = title_subject(&"x".repeat(60));
        assert_eq!(long.chars().count(), 50);
        assert!(long.starts_with("feat: "));
    }

    #[test]
    fn footers_never_join_a_footer_like_section() {
        let tasks = vec![NoteSection {
            title: "Fix: x".to_string(),
            bullets: vec!["Add: y".to_string()],
        }];
        let footers = vec!["Factory-Job: 042".to_string()];
        let msg = build_job_message(&JobMessage {
            subject: "feat: x",
            why: None,
            tasks: &tasks,
            fixes: &[],
            footers: &footers,
        });
        assert!(msg.ends_with("- Add: y\n\nFactory-Job: 042"));
    }
}
