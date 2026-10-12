#!/usr/bin/env bash
# End-to-end check: factory-ctl and the log hook feed the real server, and the API shows it.
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
repo="$(cd "$tool/../.." && pwd)"
ctl="$repo/.claude/factory/factory-ctl"
logger="$repo/.claude/factory/hooks/log-event"
export PYTHONDONTWRITEBYTECODE=1

tmp="$(mktemp -d "${TMPDIR:-/tmp}/factory-smoke.XXXXXX")"
pid=""
cleanup() {
  if [ -n "$pid" ]; then kill "$pid" 2>/dev/null; fi
  rm -rf "$tmp"
}
trap cleanup EXIT

fails=0
pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; fails=$((fails + 1)); }
check() {
  local description="$1"
  shift
  if "$@" >/dev/null 2>&1; then pass "$description"; else fail "$description"; fi
}

cargo build -j4 --manifest-path "$tool/Cargo.toml" -q || { echo "FAIL: build"; exit 1; }
bin="$tool/target/debug/factory-server"

root="$tmp/root"
git init -q -b main "$root"
git -C "$root" -c user.email=t@t -c user.name=t commit -q --allow-empty -m init
export FACTORY_ROOT="$root"
export FACTORY_EVENT_LOG="$root/.superpowers/factory/events.jsonl"

port="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
python3 "$ctl" new-job "Smoke test job" --date 2026-10-13 >/dev/null

"$bin" --root "$root" --port "$port" >"$tmp/server.log" 2>&1 &
pid=$!
for _ in $(seq 1 50); do
  if curl -fs "http://127.0.0.1:$port/healthz" >/dev/null 2>&1; then break; fi
  sleep 0.1
done

api() { curl -fs "http://127.0.0.1:$port/api/state"; }

check "health endpoint answers" curl -fs "http://127.0.0.1:$port/healthz"
check "new job appears in the spec stage" bash -c "curl -fs http://127.0.0.1:$port/api/state | jq -e '.jobs[0] | select(.id==\"001\" and .stage==\"spec\" and .created==\"2026-10-13\")'"

sleep 1
printf '%s' "{\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"s1\",\"agent_type\":\"factory-spec-writer\",\"cwd\":\"$root\"}" | python3 "$logger"
printf '%s' "{\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"s1\",\"tool_name\":\"Read\",\"tool_input\":{\"file_path\":\"CLAUDE.md\"},\"cwd\":\"$root\"}" | python3 "$logger"
sleep 1.5
check "the spec writer attaches to the job with its live tool" bash -c "curl -fs http://127.0.0.1:$port/api/state | jq -e '.jobs[0].agents[0] | select(.agent_type==\"factory-spec-writer\" and .last_tool==\"Read\" and .status==\"running\")'"

python3 "$ctl" set-stage 1 build --builder-id b1 >/dev/null
check "a stage change shows up on the next poll" bash -c "curl -fs http://127.0.0.1:$port/api/state | jq -e '.jobs[0] | select(.stage==\"build\")'"

printf '%s' "{\"hook_event_name\":\"SubagentStop\",\"agent_id\":\"s1\",\"last_assistant_message\":\"spec written\"}" | python3 "$logger"
sleep 1.5
check "a stopped agent is marked done" bash -c "curl -fs http://127.0.0.1:$port/api/state | jq -e '.unassigned_agents[0] | select(.status==\"done\")'"

check "the page is served" bash -c "curl -fs http://127.0.0.1:$port/ | grep -q 'Software factory'"
check "board.js is served" bash -c "curl -fs http://127.0.0.1:$port/board.js | grep -q FactoryBoard"
code="$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: evil.example' "http://127.0.0.1:$port/api/state")"
if [ "$code" = "403" ]; then pass "a foreign Host header gets 403"; else fail "a foreign Host header gets 403 (got $code)"; fi

echo "garbage line" >>"$FACTORY_EVENT_LOG"
sleep 1
check "a garbage line in the log does not stop the server" curl -fs "http://127.0.0.1:$port/healthz"

if [ "$fails" -eq 0 ]; then echo "all passed"; else echo "$fails failed"; fi
exit $((fails > 0))
