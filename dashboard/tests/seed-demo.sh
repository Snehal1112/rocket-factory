#!/usr/bin/env bash
# Builds a demo factory root with jobs in every stage and one five-plan series, for a visual check of the dashboard.
# Usage: seed-demo.sh <dir>. Safe to run again on the same dir.
set -eu

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
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

# A five-plan series S1: plan 1 builds and plans 2 to 5 wait at spec. Plan 1's folder holds the spec.
sdd="$root/.superpowers/sdd"
titles=("" "Harness and fixtures" "Collection benches" "Request benches" "Environment benches" "Report and baseline")
for i in 1 2 3 4 5; do
  dir="$sdd/2026-10-13-$(printf '%03d' $((5 + i)))-demo"
  mkdir -p "$dir"
  {
    printf '# Plan: %s\n\n' "${titles[$i]}"
    for t in 1 2 3; do printf '%s Task %s: Step %s of %s\n\n' '###' "$t" "$t" "${titles[$i]}"; done
  } >"$dir/plan.md"
done
printf '# Spec: Baseline benchmarks\n\n## Goal\n\nMeasure the hot paths before any performance change.\n' \
  >"$sdd/2026-10-13-006-demo/spec.md"

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
  for i in 1 2 3 4 5; do
    id="$(printf '%03d' $((5 + i)))"
    if [ "$i" -eq 1 ]; then stage=build; else stage=spec; fi
    printf ','
    job "$id" "Baseline benchmarks" "$stage" null 0 "$none" "[{\"stage\":\"spec\",\"ts\":$((now - 300))}]" \
      "{\"plan_file\":\"plan-0$i.md\",\"index\":$i,\"total\":5,\"id\":\"S1\"}" null
  done
  printf ']}\n'
} >"$data/board.json.tmp"
mv "$data/board.json.tmp" "$data/board.json"

# Job 003 is parked with details, for a look at the popup.
sed -e "s/{\"id\":\"003\",/{\"id\":\"003\",\"park_kind\":\"checks-failed\",\"park_at\":$((now - 120)),/" "$data/board.json" >"$data/board.json.tmp"
mv "$data/board.json.tmp" "$data/board.json"
mkdir -p "$sdd/2026-10-13-003-demo"
cat >"$sdd/2026-10-13-003-demo/park.json" <<JSON
{"kind":"checks-failed","reason":"checks failed after 2 fix rounds","at":$((now - 120)),"round":3,
 "next":["factory rework 003 <note>"],
 "parts":[{"title":"Failing checks","source":"checks.md","truncated":false,
  "text":"## cargo test -j4 -p factory-engine (exit 101)\nrunning 3 tests\ntest ports::tests::usage_line_has_the_counters ... ok\ntest ports::tests::a_killed_run_names_its_stop_reason ... FAILED\n\nfailures:\n\n---- ports::tests::a_killed_run_names_its_stop_reason stdout ----\nthread 'ports::tests::a_killed_run_names_its_stop_reason' panicked at src/ports.rs:171:9:\nassertion \`left == right\` failed\n  left: \"done\"\n right: \"budget-stop\"\n\nerror: test failed, to rerun pass \`-p factory-engine --lib\`"}]}
JSON

# A cost summary and estimates, for a look at the panel and the card lines.
cat >"$data/costs.json" <<JSON
{"generated_at":$((now - 240)),"thresholds":{"small_max_bytes":12288,"medium_max_bytes":25600},
 "groups":{
  "small":{"n":4,"n_approx":4,"median":280000,"low":220000,"high":370000,"roles":{"builder":61,"committer":22,"reviewer":11,"message":6},"corrected":null},
  "medium":{"n":2,"n_approx":1,"median":300000,"low":250000,"high":350000,"roles":{"builder":64,"committer":20,"reviewer":10,"message":6},"corrected":{"n":1,"median":350000,"low":350000,"high":350000}},
  "complex":{"n":3,"n_approx":3,"median":450000,"low":420000,"high":480000,"roles":{"builder":70,"committer":15,"reviewer":10,"message":5},"corrected":null},
  "all":{"n":9,"n_approx":8,"median":310000,"low":220000,"high":480000,"roles":{"builder":65,"committer":19,"reviewer":10,"message":6},"corrected":{"n":1,"median":350000,"low":350000,"high":350000}}},
 "roles":{"builder":65,"committer":19,"reviewer":10,"message":6}}
JSON
sed -e "s/{\"id\":\"001\",/{\"id\":\"001\",\"estimate\":{\"group\":\"small\",\"median\":280000,\"low\":220000,\"high\":370000,\"n\":4,\"n_approx\":4},/" \
    -e "s/{\"id\":\"004\",/{\"id\":\"004\",\"estimate\":{\"group\":\"medium\",\"median\":300000,\"low\":250000,\"high\":350000,\"n\":2,\"n_approx\":1},/" \
    -e "s/{\"id\":\"005\",/{\"id\":\"005\",\"estimate\":{\"group\":\"all\",\"median\":310000,\"low\":220000,\"high\":480000,\"n\":1,\"n_approx\":0},/" \
    "$data/board.json" >"$data/board.json.tmp"
mv "$data/board.json.tmp" "$data/board.json"

event() { printf '%s\n' "$1" >>"$log"; }
event "{\"ts\":$((now - 60)),\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"b-1\",\"agent_type\":\"factory-builder\",\"job_id\":\"001\"}"
event "{\"ts\":$((now - 30)),\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"b-1\",\"tool_name\":\"Edit\",\"tool_target\":\"src/components/request/TimeoutField.tsx\",\"job_id\":\"001\",\"tokens\":1200}"
event "{\"ts\":$((now - 20)),\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"r-2\",\"agent_type\":\"factory-code-reviewer\",\"job_id\":\"002\"}"
event "{\"ts\":$((now - 10)),\"hook_event_name\":\"PostToolUse\",\"agent_id\":\"r-2\",\"tool_name\":\"Read\",\"tool_target\":\"crates/rocket-import/src/lib.rs\",\"job_id\":\"002\"}"
event "{\"ts\":$((now - 5)),\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"s-5\",\"agent_type\":\"factory-spec-writer\"}"

echo "Demo root ready: $root"
echo "Start the server:"
echo "  cargo run -j4 --manifest-path $repo/dashboard/Cargo.toml -- --root $root --port 8765"
echo "Then open http://127.0.0.1:8765/"
