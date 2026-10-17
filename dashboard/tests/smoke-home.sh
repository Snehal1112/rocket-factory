#!/usr/bin/env bash
# End-to-end check of home mode: a real factoryd, two registered projects and factory-server --home.
set -u

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tool="$(cd "$here/.." && pwd)"
repo_root="$(cd "$tool/.." && pwd)"

tmp="$(mktemp -d "${TMPDIR:-/tmp}/factory-smoke-home.XXXXXX")"
fd_pid=""
srv_pid=""
fails=0
cleanup() {
  if [ -n "$srv_pid" ]; then kill "$srv_pid" 2>/dev/null; fi
  if [ -n "$fd_pid" ]; then kill "$fd_pid" 2>/dev/null; fi
  pkill -f "$tmp/stub" 2>/dev/null
  if [ "$fails" -ne 0 ]; then
    for f in server.log factoryd.log; do
      if [ -f "$tmp/$f" ]; then echo "--- $f ---"; cat "$tmp/$f"; fi
    done
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

cargo build -j4 --manifest-path "$tool/Cargo.toml" -q || { echo "FAIL: build dashboard"; exit 1; }
cargo build -j4 -p factory-cli -p factory-supervisor -q --manifest-path "$repo_root/Cargo.toml" || { echo "FAIL: build factory"; exit 1; }
server_bin="$tool/target/debug/factory-server"
factory_bin="$repo_root/target/debug/factory"
factoryd_bin="$repo_root/target/debug/factoryd"

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

export FACTORY_HOME="$tmp/home"
export GIT_AUTHOR_NAME=smoke GIT_AUTHOR_EMAIL=smoke@example.com GIT_COMMITTER_NAME=smoke GIT_COMMITTER_EMAIL=smoke@example.com
mkdir -p "$FACTORY_HOME"

# A stand-in for `factory`: `new` answers like the real one, every other command just waits.
cat >"$tmp/stub" <<'STUB'
#!/usr/bin/env bash
if [ "${1:-}" = "--project" ]; then shift 2; fi
if [ "${1:-}" = "new" ]; then echo "created job 001 in stub"; exit 0; fi
sleep 20
STUB
chmod +x "$tmp/stub"

for name in alpha beta; do
  mkdir -p "$tmp/repos/$name"
  git -C "$tmp/repos/$name" init -q -b main
  echo "$name" >"$tmp/repos/$name/README.md"
  git -C "$tmp/repos/$name" add README.md
  git -C "$tmp/repos/$name" commit -q -m init
  "$factory_bin" project add "$tmp/repos/$name" --preset rust --name "$name" >/dev/null || { echo "FAIL: project add $name"; exit 1; }
done

api_port="$(free_port)" || { echo "FAIL: no free port"; exit 1; }
printf 'listen = "127.0.0.1:%s"\n' "$api_port" >"$FACTORY_HOME/config.toml"
FACTORY_BIN="$tmp/stub" "$factoryd_bin" run >"$tmp/factoryd.log" 2>&1 &
fd_pid=$!
for _ in $(seq 1 100); do
  if [ -f "$FACTORY_HOME/api.token" ] && (exec 3<>"/dev/tcp/127.0.0.1/$api_port") 2>/dev/null; then break; fi
  sleep 0.1
done
token="$(cat "$FACTORY_HOME/api.token" 2>/dev/null)"

port="$(free_port)" || { echo "FAIL: no free port"; exit 1; }
url="http://127.0.0.1:$port"
"$server_bin" --home "$FACTORY_HOME" --api "127.0.0.1:$api_port" --port "$port" >"$tmp/server.log" 2>&1 &
srv_pid=$!
for _ in $(seq 1 100); do
  if curl -fs "$url/healthz" >/dev/null 2>&1; then break; fi
  sleep 0.1
done

check "/api/projects lists both projects" bash -c "curl -fs $url/api/projects | jq -e 'map(.name) == [\"alpha\",\"beta\"]'"
check "the state of a project answers with no jobs" bash -c "curl -fs $url/api/projects/alpha/state | jq -e '.jobs == []'"
check "an unknown project is not found" bash -c "[ \"\$(curl -s -o /dev/null -w '%{http_code}' $url/api/projects/nope/state)\" = 404 ]"
check "the token is not in the page, the scripts or the project list" bash -c "! (curl -fs $url/ $url/app.js $url/projects.js $url/api/projects | grep -q '$token')"
check "the response headers do not carry the token" bash -c "! (curl -sI $url/api/projects | grep -q '$token')"

body='{"spec":"/etc/hostname","plans":"'"$tmp"'","run":true}'
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -H 'Origin: http://evil.example' -d "$body" "$url/api/projects/alpha/jobs")"
if [ "$code" = "403" ]; then pass "a write with a bad Origin is refused"; else fail "a write with a bad Origin is refused (got $code)"; fi
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -d "$body" "$url/api/projects/alpha/jobs")"
if [ "$code" = "403" ]; then pass "a write without Origin is refused"; else fail "a write without Origin is refused (got $code)"; fi
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: text/plain' -H "Origin: $url" -d "$body" "$url/api/projects/alpha/jobs")"
if [ "$code" = "415" ]; then pass "a write with the wrong Content-Type is refused"; else fail "a write with the wrong Content-Type is refused (got $code)"; fi
code="$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'Content-Type: application/json' -H "Origin: $url" -H 'Host: evil.example' -d "$body" "$url/api/projects/alpha/jobs")"
if [ "$code" = "403" ]; then pass "a write with a foreign Host is refused"; else fail "a write with a foreign Host is refused (got $code)"; fi
check "nothing was queued by the refused writes" bash -c "[ -z \"\$(ls '$FACTORY_HOME/queue' 2>/dev/null)\" ] && [ -z \"\$(ls '$FACTORY_HOME/results' 2>/dev/null)\" ]"

reply="$(curl -s -X POST -H 'Content-Type: application/json' -H "Origin: $url" -d "$body" "$url/api/projects/alpha/jobs")"
request_id="$(printf '%s' "$reply" | jq -r '.requestId // empty')"
if [ -n "$request_id" ]; then pass "a valid create answers with a request id"; else fail "a valid create answers with a request id ($reply)"; fi
check "the request reached factoryd" bash -c "grep -rl '$request_id' '$FACTORY_HOME' >/dev/null"
check "the create reply does not carry the token" bash -c "! printf '%s' '$reply' | grep -q '$token'"

# The daily budget strip: absent without a limit, present with one and a usage fixture for today.
check "/api/usage has no dailyBudget without a limit" bash -c "curl -fs $url/api/usage | jq -e 'has(\"dailyBudget\") | not'"
check "/api/usage refuses a POST with 405" bash -c "[ \"\$(curl -s -o /dev/null -w '%{http_code}' -X POST $url/api/usage)\" = 405 ]"
jobs_dir="$(curl -fs -H "Authorization: Bearer $token" "http://127.0.0.1:$api_port/projects" | jq -r '.[] | select(.name == "alpha") | .jobsDir')"
mkdir -p "$jobs_dir/001"
printf '{"input":1000,"cache_read":0,"ts":%s}\n' "$(date +%s)" >"$jobs_dir/001/usage.jsonl"
printf 'daily_budget_weighted = 5000\n' >>"$FACTORY_HOME/config.toml"
check "/api/usage shows dailyBudget with a limit" bash -c "curl -fs $url/api/usage | jq -e '.dailyBudget.limit == 5000 and .dailyBudget.used == 1000 and (.dailyBudget.resetsAt | test(\"Z\$\"))'"
check "/api/usage does not carry the token" bash -c "! (curl -fs $url/api/usage | grep -q '$token')"

if [ "$fails" -eq 0 ]; then echo "all passed"; else echo "$fails failed"; fi
exit $((fails > 0))
