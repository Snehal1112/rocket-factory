#!/usr/bin/env bash
# Builds a demo factory root with jobs in every stage, for a visual check of the dashboard.
# Usage: seed-demo.sh <dir>. Safe to run again on the same dir.
set -eu

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
root="${1:?usage: seed-demo.sh <dir>}"
data="$root/.superpowers/factory"
marker="$data/.demo"

if [ -d "$data" ] && [ ! -f "$marker" ] && [ -n "$(ls -A "$data")" ]; then
  echo "refusing to overwrite $data: it is not a demo root" >&2
  exit 1
fi
mkdir -p "$data"
touch "$marker"
log="$data/events.jsonl"
: >"$log"
chmod 600 "$log"

now="$(date +%s)"
reviews() { printf '{"security":"%s","ux":"%s","ui":"%s","code":"%s"}' "$1" "$2" "$3" "$4"; }
job() {
  # id feature stage reason round reviews history series tokens
  printf '{"id":"%s","dir":"2026-10-13-%s-demo","slug":"demo","created":"2026-10-13","feature":"%s","branch":"factory/%s-demo","stage":"%s","reason":%s,"round":%s,"stage_since":%s,"reviews":%s,"stage_history":%s,"series":%s,"tokens":%s}' \
    "$1" "$1" "$2" "$1" "$3" "$4" "$5" "$((now - 300))" "$6" "$7" "$8" "$9"
}
none='{"security":"pending","ux":"pending","ui":"pending","code":"pending"}'
hist() { printf '[{"stage":"spec","ts":%s},{"stage":"build","ts":%s},{"stage":"%s","ts":%s}]' "$((now - 1500))" "$((now - 900))" "$1" "$((now - 300))"; }

{
  printf '{"jobs":['
  job 001 "Add a request timeout setting" build null 0 "$none" "$(hist build)" '{"plan_file":"p3.md","index":2,"total":3}' '{"weighted":2500000,"raw":14000000}'
  printf ','
  job 002 "Fix collection import crash on empty folders" review null 1 "$(reviews pass skipped skipped running)" "$(hist review)" null '{"weighted":900000,"raw":5200000}'
  printf ','
  job 003 "Rename the environment variables panel" needs-human '"touches the collection trust gate"' 2 "$(reviews changes pass pass pass)" "$(hist needs-human)" null '{"weighted":3100000,"raw":17000000}'
  printf ','
  job 004 "Show response size in the status bar" merged null 0 "$(reviews pass pass pass pass)" "$(hist merged)" null '{"weighted":400000,"raw":2300000}'
  printf ','
  job 005 "Add a gRPC reflection toggle" spec null 0 "$none" "[{\"stage\":\"spec\",\"ts\":$((now - 300))}]" null null
  printf ']}\n'
} >"$data/board.json.tmp"
mv "$data/board.json.tmp" "$data/board.json"

event() { printf '%s\n' "$1" >>"$log"; }
event "{\"ts\":$((now - 60)),\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"b-1\",\"agent_type\":\"factory-builder\",\"job_id\":\"001\"}"
event "{\"ts\":$((now - 30)),\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"b-1\",\"tool_name\":\"Edit\",\"tool_target\":\"src/components/request/TimeoutField.tsx\",\"job_id\":\"001\",\"tokens\":1200}"
event "{\"ts\":$((now - 20)),\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"r-2\",\"agent_type\":\"factory-code-reviewer\",\"job_id\":\"002\"}"
event "{\"ts\":$((now - 10)),\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"r-2\",\"tool_name\":\"Read\",\"tool_target\":\"crates/rocket-import/src/lib.rs\",\"job_id\":\"002\"}"
event "{\"ts\":$((now - 5)),\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"s-5\",\"agent_type\":\"factory-spec-writer\"}"

echo "Demo root ready: $root"
echo "Start the server:"
echo "  cargo run -j4 --manifest-path $repo/tools/factory-server/Cargo.toml -- --root $root --port 8765"
echo "Then open http://127.0.0.1:8765/"
