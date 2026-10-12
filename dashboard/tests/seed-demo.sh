#!/usr/bin/env bash
# Builds a demo factory root with jobs in every stage, for a visual check of the dashboard.
# Usage: seed-demo.sh <empty-dir>
set -eu

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
root="${1:?usage: seed-demo.sh <empty-dir>}"
ctl="$repo/.claude/factory/factory-ctl"
logger="$repo/.claude/factory/hooks/log-event"
log="$root/.superpowers/factory/events.jsonl"

mkdir -p "$root"
git init -q -b main "$root"
git -C "$root" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init
export FACTORY_ROOT="$root"
export FACTORY_EVENT_LOG="$log"

run_ctl() { python3 "$ctl" "$@" >/dev/null; }
event() { printf '%s' "$1" | python3 "$logger"; }

python3 "$ctl" new-job "Add a request timeout setting" --date 2026-10-13 >/dev/null
python3 "$ctl" new-job "Fix collection import crash on empty folders" --date 2026-10-13 >/dev/null
python3 "$ctl" new-job "Rename the environment variables panel" --date 2026-10-12 >/dev/null
python3 "$ctl" new-job "Show response size in the status bar" --date 2026-10-12 >/dev/null
python3 "$ctl" new-job "Add a gRPC reflection toggle" --date 2026-10-13 >/dev/null

run_ctl set-stage 1 build --builder-id b-1 --worktree "$root/.claude/worktrees/factory-001"
run_ctl set-stage 2 review --round 1 --review security=pass --review ux=skipped --review ui=skipped --review code=running
run_ctl set-stage 3 needs-human --round 2 --reason "touches the collection trust gate" \
  --review security=changes --review ux=pass --review ui=pass --review code=pass
run_ctl set-stage 4 merged --review security=pass --review ux=pass --review ui=pass --review code=pass
# Job 5 stays in the spec stage.

sleep 1
event '{"hook_event_name":"SubagentStart","agent_id":"b-1","agent_type":"factory-builder","cwd":"'"$root"'/.claude/worktrees/factory-001"}'
event '{"hook_event_name":"PostToolUse","agent_id":"b-1","tool_name":"Edit","tool_input":{"file_path":"src/components/request/TimeoutField.tsx"},"cwd":"'"$root"'/.claude/worktrees/factory-001"}'
event '{"hook_event_name":"PostToolUse","agent_id":"b-1","tool_name":"Bash","tool_input":{"command":"cargo check -j4"},"cwd":"'"$root"'/.claude/worktrees/factory-001"}'
event '{"hook_event_name":"SubagentStart","agent_id":"r-2","agent_type":"factory-code-reviewer","cwd":"'"$root"'"}'
event '{"hook_event_name":"PostToolUse","agent_id":"r-2","tool_name":"Read","tool_input":{"file_path":"crates/rocket-import/src/lib.rs"},"cwd":"'"$root"'"}'
event '{"hook_event_name":"SubagentStart","agent_id":"s-5","agent_type":"factory-spec-writer","cwd":"'"$root"'"}'
event '{"hook_event_name":"PostToolUse","agent_id":"s-5","tool_name":"Grep","tool_input":{"pattern":"reflection"},"cwd":"'"$root"'"}'

echo "Demo root ready: $root"
echo "Start the server:"
echo "  cargo run -j4 --manifest-path $repo/tools/factory-server/Cargo.toml -- --root $root --port 8765"
echo "Then open http://127.0.0.1:8765/"
