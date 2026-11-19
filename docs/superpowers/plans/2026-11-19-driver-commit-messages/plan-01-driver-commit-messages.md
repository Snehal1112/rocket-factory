# Driver commit messages: notes, job message and the plan commit line (plan 1 of 4)

> **Spec:** docs/superpowers/specs/2026-11-19-driver-commit-messages-design.md (phase 1)
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Pure building blocks for driver-written commit messages: the builder's commit note parser and checks, the job message assembler, and the plan's `**Commit:**` line with its lint, so later plans only wire them in.

**Commit:** feat(commit): parse notes and build job messages

**Architecture:** Everything here is additive and has no caller in the run loop yet. Note parsing, wrapping, subject rules and message assembly are pure functions in `factory-core/src/commitmsg.rs`. Reading the `**Commit:**` line of a plan or spec is in `factory-engine/src/tasks.rs`, the refusal is in `planlint.rs` and `validate_series`, and `--one-job` writes the spec's line into the combined plan.

**Tech Stack:** Rust, factory-core, factory-engine. No new dependency.

## Global Constraints

- The job subject is at most 50 characters (`MAX_JOB_SUBJECT_CHARS = 50`), passes `validate_message` (Conventional Commits, lowercase type from its list, no trailing period), and the shared `MAX_SUBJECT_CHARS = 72` stays unchanged for every other message.
- Commit note markers are exactly `---BEGIN COMMIT---` and `---END COMMIT---`.
- Body lines are wrapped at 72 characters. A wrapped bullet continues with a two-space hanging indent.
- The job message body (task sections plus review fixes) holds at most 60 bullets (`MAX_JOB_BULLETS = 60`); the rest becomes one line `- and N more changes`.
- A message problem never fails or parks anything: every function here returns a usable value or a reason, never panics.
- Cargo: `-j4`, one crate at a time (`-p <crate>`), never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")`, never the unwrap call. Code comments are short full sentences that end with a period.

## Review Focus

1. A builder reply whose note sits inside a code fence (the way the builder file shows it): the note is still found, the fence lines are not part of it. Pinned by `a_fenced_note_is_accepted`.
2. A reply with a note but no `Ruling:` lines: `parse_builder_status` text for DONE_WITH_CONCERNS does not contain the note. Pinned by `the_status_text_stops_at_the_commit_note`.
3. A subject or title with multi-byte characters near the cut: no split character and no panic. Pinned by `title_subject_cuts_on_a_word_boundary_with_multibyte_text`.
4. A plan whose `**Commit:**` line is wrapped in backticks or bold marks: read as the bare subject. Pinned by `the_commit_line_ignores_backticks_and_bold`.
5. A `**Commit:**` line below the header (inside a task section or a code fence): not read as the plan's subject. Pinned by `a_commit_line_inside_a_task_or_fence_is_ignored`.

## File Structure

- `crates/factory-core/src/commitmsg.rs`: note markers, `CommitNote`, `parse_commit_note`, `check_note`, `fallback_note`, `note_bullets`, `wrap_paragraph`, `wrap_bullet`, `check_job_subject`, `title_subject`, `NoteSection`, `JobMessage`, `build_job_message`; `parse_builder_status` stops at the note marker.
- `crates/factory-core/src/lib.rs`: exports.
- `crates/factory-engine/src/tasks.rs`: `header_commit_line`, `goal_line` (moved from `combine.rs`).
- `crates/factory-engine/src/planlint.rs`: refusal for a broken `**Commit:**` line.
- `crates/factory-engine/src/runloop.rs`: `validate_series_inner` refuses a spec with a broken `**Commit:**` line.
- `crates/factory-engine/src/one_job.rs`, `crates/factory-engine/src/combine.rs`: `with_commit_line` and its use for `--one-job`.
- Tests: unit tests in `commitmsg.rs`, `tasks.rs`, `planlint.rs`, `combine.rs`; `crates/factory-engine/tests/one_job.rs`; `crates/factory-engine/src/runloop.rs` test module.

---

### Task 1: The builder's commit note

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs` (`parse_builder_status` at 338-373, add new items after `extract_message` at 394-404), `crates/factory-core/src/lib.rs` (exports)
- Test: the `tests` module of `commitmsg.rs`

**Interfaces:**
- Produces:
  - `pub const COMMIT_BEGIN: &str = "---BEGIN COMMIT---";` and `pub const COMMIT_END: &str = "---END COMMIT---";`
  - `pub const MAX_JOB_SUBJECT_CHARS: usize = 50;`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct CommitNote { pub subject: String, pub body: String }`, with `pub fn to_file_text(&self) -> String` (`"{subject}\n\n{body}\n"`) and `pub fn from_file_text(text: &str) -> Option<CommitNote>` (first line is the subject, the rest after one blank line is the body, trimmed at the end; `None` when the subject is empty).
  - `pub fn parse_commit_note(reply: &str) -> Option<CommitNote>`: searches the raw text (fenced or not) for the last `COMMIT_BEGIN` that has a `COMMIT_END` after it; inside the pair, lines whose trimmed start is ```` ``` ```` are dropped; the first non-blank line is the subject (trimmed), the remaining lines, trimmed at both ends as a block, are the body. `None` when there is no pair or the subject is empty.
  - **Bullet grouping**, shared by `check_note` and `note_bullets`: paragraphs are separated by blank lines. Within a paragraph, a line whose trimmed start is `- ` or `* ` starts a bullet, and every following non-blank line that does not start a bullet continues it, whatever its indent. Lines before the first bullet of a paragraph are prose.
  - `pub fn check_note(note: &CommitNote) -> Result<CommitNote, String>`: refuses an empty subject (`"the subject is empty"`), a subject over 50 characters (`"the subject is {n} characters, the limit is 50"`), a subject ending with `.` (`"the subject ends with a period"`), an empty body (`"the body is empty"`). On success returns the note with the body rewrapped: each bullet (continuations joined with one space, passed without its `- ` or `* ` marker, so `* ` bullets come back as `- `) through `wrap_bullet(_, 72)`, each prose run through `wrap_paragraph(_, 72)`, paragraphs still separated by one blank line. A body of prose with no bullet is accepted; its task section in the job message is then only a title (a known choice).
  - `pub fn wrap_paragraph(text: &str, width: usize) -> String`: folds whitespace and wraps on word boundaries at `width` characters (counted in chars); a single word longer than `width` stays on its own line.
  - `pub fn wrap_bullet(text: &str, width: usize) -> String`: `text` is one bullet with or without its leading `- `; the result starts with `- ` and continuation lines start with two spaces.
  - `pub fn note_bullets(body: &str) -> Vec<String>`: the bullets of a note body by the bullet grouping above, as single-line strings without the `- ` or `* ` prefix, continuations joined with one space; prose is skipped.
  - **Cutting text to a limit** (used by `fallback_note` here and `title_subject` in the next task): fold whitespace, cut at the last word boundary that fits; when no word boundary fits (one long word), cut at the character limit; after cutting, trim trailing whitespace and `.`; when nothing remains, use the function's default. Characters are counted as chars, never bytes. Implement it once as a private `fn cut_text(text: &str, limit: usize) -> String` (empty result means "use the default").
  - `pub fn fallback_note(task_title: &str, files: &[String]) -> CommitNote`: subject is the task title cut to 50 characters by the rule above (default `"Factory task"`); body is one `- <path>` line per file for the first 20 files, then `- and N more files` when there are more; `- no files changed` when the list is empty.
  - `parse_builder_status` stops collecting text at the first line whose trim equals `COMMIT_BEGIN`, as well as at `Ruling:`.

- [ ] **Step 1: Read** `commitmsg.rs` from `blocked_reason` to the end of `extract_message`, and the existing tests of `parse_builder_status`.
- [ ] **Step 2: Write the failing tests**:
  - `a_note_is_parsed_from_the_end_of_a_reply`: reply `"Status: DONE\nRuling: a — b — c\n---BEGIN COMMIT---\nAdd a retry endpoint\n\nWhy it matters.\n\n- Add POST retry\n---END COMMIT---\n"` gives subject `"Add a retry endpoint"` and body `"Why it matters.\n\n- Add POST retry"`.
  - `a_missing_note_is_none`: no markers, only a begin marker, and an empty pair all give `None`.
  - `a_fenced_note_is_accepted`: the same note wrapped in a ```` ```text ```` fence gives the same `CommitNote`.
  - `the_last_note_wins`: two pairs, the second subject is returned.
  - `check_note_refuses_each_rule`: empty subject, a 51-character subject, a subject ending with `.`, an empty body each give the error text from the Interfaces; a 50-character subject passes.
  - `check_note_rewraps_long_bullets`: a 100-character bullet comes back as lines of at most 72 chars, the first starting `- `, the rest starting with two spaces.
  - `note_bullets_joins_continuations_and_skips_prose`: body `"Prose line.\n\n- one\n  more\n- two"` gives `["one more", "two"]`; body `"- one\nmore\n* two"` gives `["one more", "two"]`.
  - `a_note_round_trips_through_its_file_text`: a note whose body has a prose paragraph and bullets separated by a blank line comes back equal from `from_file_text(&note.to_file_text())`.
  - `fallback_note_lists_twenty_files_then_a_count`: 23 files give 20 `- <path>` lines and `- and 3 more files`; an empty list gives `- no files changed`; a 70-character title is cut to at most 50 chars on a word boundary; a 60-character single word is cut to exactly 50 chars.
  - `cut_text_handles_long_words_and_periods`: `cut_text("Fix A. Then B", 6)` is `"Fix A"`; a 60-char single word cut to 50 is its first 50 chars; `cut_text("...", 10)` is empty.
  - `the_status_text_stops_at_the_commit_note`: `"Status: DONE_WITH_CONCERNS\nslow test\n---BEGIN COMMIT---\nAdd x\n\n- y\n---END COMMIT---"` gives `DoneWithConcerns("slow test")`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core commitmsg` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items and export them from `lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`. Expected: pass.

### Task 2: The job message

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs` (after the note items), `crates/factory-core/src/lib.rs`
- Test: the `tests` module of `commitmsg.rs`

**Interfaces:**
- Consumes: `wrap_paragraph`, `wrap_bullet`, `MAX_JOB_SUBJECT_CHARS`, `validate_message`, `append_footers`, `parse_issue_refs`.
- Produces:
  - `pub const MAX_JOB_BULLETS: usize = 60;`
  - `pub fn check_job_subject(subject: &str) -> Result<(), String>`: `validate_message(subject)` plus `"the subject is {n} characters, the limit is 50"` over 50 chars.
  - `pub fn title_subject(title: &str) -> String`: `feat: <text>` where text is the title with a trailing `Implementation Plan` (any case) removed, then every whitespace-separated word dropped that starts with `http://` or `https://` or for which `parse_issue_refs(word)` is non-empty, whitespace folded, the first letter lowercased; the text is cut with `cut_text(text, 44)` (50 minus the 6 characters of `feat: `) and the subject is `feat: {cut}`; when the cut is empty the subject is `feat: factory job`.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct NoteSection { pub title: String, pub bullets: Vec<String> }`
  - `#[derive(Debug, Clone)] pub struct JobMessage<'a> { pub subject: &'a str, pub why: Option<&'a str>, pub tasks: &'a [NoteSection], pub fixes: &'a [String], pub footers: &'a [String] }`
  - `pub fn build_job_message(m: &JobMessage<'_>) -> String`, laid out as: subject; blank line; `why` through `wrap_paragraph(_, 72)` when `Some` and non-blank, then a blank line; each task section as `<title>:` followed by its bullets through `wrap_bullet(_, 72)`, sections separated by a blank line, a section with no bullets is only its title line; `Review fixes:` with the `fixes` bullets when `fixes` is non-empty; then one blank line and the footer lines (do not call `append_footers` on the body, because note titles such as `Fix: x:` and bullets such as `- Add: y` look like footers: drop each footer line that already appears as a line of the body, then append one blank line and the remaining footers joined by `\n`; when none remain, append nothing). Bullets are counted across sections and fixes in that order; after 60, the rest are dropped and one `- and N more changes` line closes the last section printed. A task section, or the `Review fixes:` heading, with no bullet left after the cap is left out; a section that had no bullets to begin with still prints its title. No trailing newline.

- [ ] **Step 1: Read** `validate_message`, `append_footers` and `parse_issue_refs`.
- [ ] **Step 2: Write the failing tests**:
  - `the_job_message_has_subject_why_sections_fixes_and_footers`: subject `feat(retry): retry a parked job`, why `"The dashboard could not continue a parked job."`, two sections with two and one bullets, fixes `["Refuse approve while a retry is queued"]`, footers `["Fixes: #142", "Factory-Job: 042"]` gives exactly:
    ```text
    feat(retry): retry a parked job

    The dashboard could not continue a parked job.

    Add the endpoint:
    - a
    - b

    Show the button:
    - c

    Review fixes:
    - Refuse approve while a retry is queued

    Fixes: #142
    Factory-Job: 042
    ```
  - `a_message_without_why_or_fixes_leaves_them_out`: no blank paragraph and no `Review fixes:` line.
  - `the_job_message_passes_validate_message`: the message from the first test passes `validate_message`.
  - `the_body_is_capped_at_sixty_bullets`: 3 sections of 25 bullets give 60 bullet lines plus `- and 15 more changes`.
  - `check_job_subject_limits_fifty_characters`: a valid 50-char subject passes, 51 fails with the limit text, `Add x` fails the type check, `feat: x.` fails the period check.
  - `title_subject_strips_the_plan_suffix_and_issue_refs`: `"Retry at the parked stage Implementation Plan"` gives `feat: retry at the parked stage`; `"Fix #12 crash https://github.com/o/r/issues/12"` gives `feat: fix crash`; `""` gives `feat: factory job`.
  - `title_subject_cuts_on_a_word_boundary_with_multibyte_text`: a 90-char title with `é` and `—` gives a subject of at most 50 chars that ends on a whole word and passes `check_job_subject`; a 60-char single word gives a 50-char subject starting `feat: `.
  - `footers_never_join_a_footer_like_section`: a section titled `Fix: x` with the bullet `Add: y` still has one blank line between its last bullet and `Factory-Job: 042`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core commitmsg` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items and export them.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`. Expected: pass.

### Task 3: The plan and spec commit line

**Files:**
- Modify: `crates/factory-engine/src/tasks.rs` (next to `plan_title` and `plan_header`), `crates/factory-engine/src/combine.rs` (`goal_line` at 400-411 moves to `tasks.rs`; add `with_commit_line`), `crates/factory-engine/src/planlint.rs` (`lint_plan` at 72), `crates/factory-engine/src/runloop.rs` (`validate_series_inner`, after `check_spec` at 404), `crates/factory-engine/src/one_job.rs` (after `combine_plans` at 117)
- Test: unit tests in `tasks.rs`, `planlint.rs`, `combine.rs`, the `runloop.rs` test module (near `validate_series_reports_a_spec_without_a_goal_as_invalid` at 970), `crates/factory-engine/tests/one_job.rs`

**Interfaces:**
- Consumes: `check_job_subject`, `title_subject` from factory-core.
- Produces:
  - `pub fn header_commit_line(text: &str) -> Option<String>` in `tasks.rs`: the value of the first `**Commit:**` (or `Commit:`) line outside code fences that comes before the first `## ` heading and before the first `### Task` heading; leading `>` quote marks, bold marks and surrounding backticks are removed from the value; `None` when there is no such line or the value is empty. Implement the line match once as `pub(crate) fn commit_line_value(line: &str) -> Option<&str>` in `tasks.rs`, used by both `header_commit_line` and `with_commit_line`.
  - `pub(crate) fn goal_line(header: &str) -> Option<String>` moved to `tasks.rs` unchanged; `combine.rs` imports it.
  - `lint_plan`: when `header_commit_line(plan)` is `Some(subject)` and `check_job_subject(&subject)` fails, `refuse = Some(format!("{name} **Commit:** line `{subject}`: {why}"))`. A plan with no line gets no refusal and no warning.
  - `validate_series_inner`: inside the `if has_spec` block, when the spec text has a `header_commit_line` that fails `check_job_subject`, return `EngineError::Msg(format!("the spec **Commit:** line `{subject}`: {why}"))` before any job exists. `validate_series` maps `Msg` to `EngineError::Invalid`, so the test matches `EngineError::Invalid(m) if m.contains("**Commit:** line")`, like the test it sits next to.
  - `pub fn with_commit_line(text: &str, subject: &str) -> String` in `combine.rs`: removes exactly the lines for which `commit_line_value` is `Some` in the region `header_commit_line` reads, collapses runs of blank lines left by the removal to one, and inserts `**Commit:** {subject}` on its own paragraph right after the `<!-- Combined from: ... -->` line (or after the `# ` title line when there is no such comment).
  - In `one_job.rs`, when the job has a spec text: `subject = header_commit_line(spec).unwrap_or_else(|| title_subject(&plan_title(spec)))`, and the combined text is `with_commit_line(&combined.text, &subject)` before it is written to `plan.md`. Without a spec text the combined plan is written as today. A one-plan chain takes the early return at `one_job.rs:81-92` and keeps its plan's own `**Commit:**` line; that path is not changed.

- [ ] **Step 1: Read** `plan_title`, `plan_header`, `lines_with_fence` and `is_task_heading` in `tasks.rs`; `goal_line` and the output assembly of `combine_plans` (`combine.rs:162-219`); `lint_plan`; `validate_series_inner` from 349 to 420; `one_job.rs` 95-135 and one test in `tests/one_job.rs`.
- [ ] **Step 2: Write the failing tests**:
  - `tasks.rs`: `the_commit_line_is_read_from_the_header` (`"# T\n\n**Goal:** g\n\n**Commit:** feat(x): add y\n\n### Task 1: a\n"` gives `Some("feat(x): add y")`); `the_commit_line_ignores_backticks_and_bold` (`` **Commit:** `fix: z` `` and `> **Commit:** fix: z` both give `fix: z`); `a_commit_line_inside_a_task_or_fence_is_ignored`; `a_spec_commit_line_counts_only_before_the_first_section` (`**Commit:**` after `## Goal` gives `None`).
  - `planlint.rs`: `a_broken_commit_line_refuses` (a 51-char subject, a subject without a type, a subject ending with `.`: `refuse` contains `**Commit:** line`); `a_valid_or_missing_commit_line_does_not_refuse`.
  - `runloop.rs`: `validate_series_refuses_a_spec_with_a_broken_commit_line` (same fixture style as the test at 970, spec header with `**Commit:** Add x`).
  - `combine.rs`: `with_commit_line_replaces_the_plan_line_after_the_comment` (a combined text with plan 1's `**Commit:** feat: a` gets exactly one `**Commit:**` line, `feat: whole series`, after the comment line); `with_commit_line_leaves_fenced_and_task_lines_alone` (a fenced `**Commit:** x` inside a task body survives; a checkbox-form plan with no `### Task` heading keeps a `**Commit:**` line written below its first `## ` heading).
  - `tests/one_job.rs`: `one_job_uses_the_spec_commit_line` (the plans' header gets `**Commit:** feat: plan one` for this test; spec header `**Commit:** feat(x): the whole series`; the job's `plan.md` has that line once and not plan 1's line); `one_job_without_a_spec_commit_line_uses_the_spec_title` (spec `# Retry design` gives `**Commit:** feat: retry design`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --lib -- tasks planlint combine runloop` and `cargo test -j4 -p factory-engine --test one_job`, and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo check -j4 -p factory-cli`. Expected: pass.

Next plan: docs/superpowers/plans/2026-11-19-driver-commit-messages/plan-02-driver-commit-messages.md
