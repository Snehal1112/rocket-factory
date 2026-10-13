#!/usr/bin/env bash
# End-to-end check: board.json and events.jsonl feed the real server, and the API shows them.
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/factory-smoke.XXXXXX")"
pid=""
fails=0
cleanup() {
  if [ -n "$pid" ]; then kill "$pid" 2>/dev/null; fi
  if [ "$fails" -ne 0 ] && [ -f "$tmp/server.log" ]; then
    echo "--- server.log ---"
    cat "$tmp/server.log"
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT

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
data="$root/.superpowers/factory"
mkdir -p "$data"
board="$data/board.json"
log="$data/events.jsonl"

# Writes the board with one job in the given stage. The token counters use the driver's shape.
write_board() {
  local stage="$1" now
  now="$(date +%s)"
  cat >"$board.tmp" <<JSON
{"jobs":[{"id":"001","dir":"2026-10-13-001-smoke","slug":"smoke","created":"2026-10-13","feature":"Smoke test job","branch":"factory/001-smoke","stage":"$stage","round":0,"stage_since":$now,"reviews":{"code":"pending"},"stage_history":[{"stage":"spec","ts":$((now - 120))},{"stage":"$stage","ts":$now}],"series":{"plan_file":"p2.md","index":2,"total":3},"tokens":{"input":300000,"cache_creation":100000,"output":100000,"cache_read":1000000}}]}
JSON
  mv "$board.tmp" "$board"
}

event() { printf '%s\n' "$1" >>"$log"; }

# Finds a free local port by trying random ones in the dynamic range.
free_port() {
  local candidate
  for _ in $(seq 1 50); do
    candidate=$((20000 + RANDOM % 20000))
    if ! (exec 3<>"/dev/tcp/127.0.0.1/$candidate") 2>/dev/null; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

port="$(free_port)" || { echo "FAIL: no free port"; exit 1; }
url="http://127.0.0.1:$port"
state() { curl -fs "$url/api/state"; }

# Polls until a jq filter is true on the state, for at most 10 seconds.
wait_for() {
  local filter="$1"
  for _ in $(seq 1 100); do
    if state 2>/dev/null | jq -e "$filter" >/dev/null 2>&1; then return 0; fi
    sleep 0.1
  done
  return 1
}
await() {
  local description="$1" filter="$2"
  if wait_for "$filter"; then pass "$description"; else fail "$description"; fi
}

write_board build
: >"$log"
chmod 600 "$log"
"$bin" --root "$root" --port "$port" >"$tmp/server.log" 2>&1 &
pid=$!
for _ in $(seq 1 100); do
  if curl -fs "$url/healthz" >/dev/null 2>&1; then break; fi
  sleep 0.1
done

now="$(date +%s)"
check "health endpoint answers" curl -fs "$url/healthz"
await "the job shows its stage, series and tokens" '.jobs[0] | select(.id=="001" and .stage=="build" and .series.index==2 and .token_view.weighted==600000 and .token_view.fraction==0.2)'
await "per-stage durations come from the stage history" '.jobs[0].stage_durations | map(.stage) == ["spec","build"]'

event "{\"ts\":$now,\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"b1\",\"agent_type\":\"factory-builder\",\"job_id\":\"001\"}"
event "{\"ts\":$now,\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"b1\",\"tool_name\":\"Read\",\"tool_target\":\"CLAUDE.md\",\"job_id\":\"001\",\"tokens\":10}"
await "the builder attaches to the job by job_id with its live tool" '.jobs[0].agents[0] | select(.agent_type=="factory-builder" and .last_tool=="Read" and .status=="running" and .tokens_weighted==10)'

event "{\"ts\":$now,\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"x1\",\"agent_type\":\"factory-code-reviewer\"}"
await "an event without job_id stays unassigned" '.unassigned_agents | map(.agent_id) == ["x1"]'

write_board review
await "a stage change shows up on the next poll" '.jobs[0] | select(.stage=="review")'

event "{\"ts\":$((now + 5)),\"hook_event_name\":\"SubagentStop\",\"agent_id\":\"b1\",\"job_id\":\"001\",\"last_message\":\"done\"}"
await "a stopped agent is marked done" '.jobs[0].agents[0] | select(.status=="done")'

write_board merged
await "a merged job has no growing merged duration" '.jobs[0].stage_durations | all(.stage != "merged")'

check "the page is served" bash -c "curl -fs $url/ | grep -q 'Software factory'"
check "board.js and patch.js are served" bash -c "curl -fs $url/board.js | grep -q FactoryBoard && curl -fs $url/patch.js | grep -q FactoryPatch"
check "the CSP forbids base and form targets" bash -c "curl -sI $url/ | grep -i content-security-policy | grep -q \"base-uri 'none'; form-action 'none'\""
code="$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: evil.example' "$url/api/state")"
if [ "$code" = "403" ]; then pass "a foreign Host header gets 403"; else fail "a foreign Host header gets 403 (got $code)"; fi

echo "garbage line" >>"$log"
await "skipped log lines are counted in the state" '.skipped_lines >= 1'
check "a garbage line does not stop the server" curl -fs "$url/healthz"

# A broken new field must not turn the board into an error.
cat >"$board" <<JSON
{"jobs":[{"id":"001","dir":"d","slug":"s","created":"2026-10-13","feature":"f","branch":"b","stage":"build","round":0,"stage_since":1,"reviews":{},"series":{"index":"x"},"tokens":"lots","stage_history":["junk"]}]}
JSON
await "a malformed series, tokens or history does not give a board error" '.board_error == null and (.jobs | length) == 1'

if [ "$fails" -eq 0 ]; then echo "all passed"; else echo "$fails failed"; fi
exit $((fails > 0))
