## Harness notes: Codex

### Task list

Keep the visible task list with `update_plan`.

### Shell and background runs

Run commands with `exec_command`. If it returns a still-running session handle, keep the handle and send input or read output with `write_stdin` only when the user asks. Never poll. If it returns a final exit result, including a non-zero exit, continue at once and do not report it as running.

### Skills

Use only the skill catalog the run preface allows. Follow the shared text for the rest.

### Subagents

No subagents are available in factory runs. Do the work yourself.

### File tools

Change files with `apply_patch`. Read and search files with shell commands.

### Interview

Follow the shared text.

### Plan writing

Follow the shared text.

### Commit

Follow the shared text.

### Model option

`--opus` is a Claude-only option. If it appears at the start or end of a feature request, explain that this Codex skill does not accept it and ask the user to select a Codex model in the project configuration; stop before creating a draft or job. A feature may start or end with the single token `--one-job`: remove it from the feature text and slug, and pass `--one-job` to `<factory> --project <name> new`. Skill invocation as `$factory` follows the shared text.
