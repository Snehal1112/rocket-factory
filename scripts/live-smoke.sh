#!/usr/bin/env bash
# Runs a tiny job through the real agent CLI and fails unless it merges.
#
# Usage: ./scripts/live-smoke.sh [--harness claude|codex] [--no-probe] [--keep]
#            [--key-from <project>] [--out <dir>]
#
# It builds a one-task fixture repo in a temp dir, registers it under a
# throwaway FACTORY_HOME, probes the harness, then runs job 001 and checks that
# it merged and that the transcripts are saved. It never writes your real
# factory home or repos and never runs `factory install`. Its one read of the
# real home is `factory project show <key project>`, to copy the api_key_file
# and allow_api_key settings. The key file itself is never read or saved.
#
# Exit codes: 0 pass, 1 smoke failure, 2 usage error or missing tool.
#
# Env: FACTORY_BIN (the factory binary), LIVE_SMOKE_AGENT_PROGRAM (the program
# checked with `command -v`, default the harness name; used by tests), TMUX_BIN
# (the tmux program for --interactive, default tmux).
#
# See docs/operations/releasing.md.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PROJECT="live-smoke"
JOB="001"

harness="claude"
interactive=0
no_probe=0
keep=0
out=""
key_from=""
real_home="${FACTORY_HOME:-$HOME/.factory}"
key_lines=""
credentials=""
work=""
passed=0
session=""
started=$SECONDS

usage() {
    echo "Usage: $0 [--harness claude|codex] [--no-probe] [--keep] [--key-from <project>]"
    echo "          [--out <dir>]"
    echo ""
    echo "  --harness <h>  the agent CLI to run, claude (default) or codex"
    echo "  --interactive  also drive the CLI in tmux and check /factory status"
    echo "  --no-probe     skip 'factory probe'; only for the fake-agent test, which"
    echo "                 cannot answer the probe like a real agent"
    echo "  --keep         keep the temp work dir after a pass (a failure always keeps it)"
    echo "  --key-from <p> the registered project whose API key settings the run copies"
    echo "                 (default: the project that holds this checkout)"
    echo "  --out <dir>    where the evidence goes (default: a UTC-stamped folder under"
    echo "                 \${TMPDIR:-/tmp}/factory-live-smoke)"
    echo "  -h, --help     show this help"
    echo ""
    echo "Exit codes: 0 pass, 1 smoke failure, 2 usage error or missing tool."
}

die_usage() {
    echo -e "${RED}$*${NC}" >&2
    usage >&2
    exit 2
}

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo -e "${RED}Missing tool: $1${NC}" >&2
        exit 2
    fi
}

fail() {
    echo -e "${RED}FAIL: $*${NC}" >&2
    write_summary "FAIL: $*"
    exit 1
}

write_summary() {
    [ -n "$out" ] || return 0
    mkdir -p "$out"
    {
        echo "$1"
        echo "harness: $harness"
        [ -z "$credentials" ] || echo "$credentials"
        echo "cli version: $cli_version"
        echo "job: $JOB"
        echo "stage: ${stage:-unknown}"
        echo "seconds: $((SECONDS - started))"
    } >"$out/summary.txt"
}

cleanup() {
    if [ -n "$session" ] && "$TMUX_BIN" has-session -t "$session" 2>/dev/null; then
        "$TMUX_BIN" kill-session -t "$session" 2>/dev/null || true
    fi
    if [ -n "$work" ] && [ -d "$work" ]; then
        if [ "$passed" -eq 1 ] && [ "$keep" -eq 0 ]; then
            rm -rf "$work"
        else
            echo -e "${YELLOW}Work dir kept: $work${NC}" >&2
        fi
    fi
}

factory() {
    "$FACTORY_BIN" "$@"
}

build_fixture() {
    repo="$work/repo"
    mkdir -p "$repo"
    cp -R "$REPO_ROOT/scripts/live-smoke/fixture/." "$repo/"
    mkdir -p "$work/plans"
    mv "$repo/plans/"* "$work/plans/"
    rmdir "$repo/plans"
    mv "$repo/spec.md" "$work/spec.md"
    git -C "$repo" init -q -b main
    git -C "$repo" add -A
    git -C "$repo" -c user.name=smoke -c user.email=smoke@example.com commit -q -m "chore: fixture"
}

# Prints the key setting lines of a project that are not defaults. It reads the
# real factory home through `project show` and never the key file.
key_lines_of() {
    FACTORY_HOME="$real_home" factory project show "$1" 2>/dev/null |
        sed -n 's/^ *\(\(api_key_file\|allow_api_key\) = .*\)$/\1/p' |
        grep -v '(default)$' || true
}

# Sets $key_lines for the throwaway project and $credentials for the summary.
copy_key_settings() {
    local source="$key_from" found file
    if [ ! -d "$real_home" ]; then
        credentials="credentials: login (no factory home)"
        return 0
    fi
    if [ -z "$source" ]; then
        source="$(cd "$REPO_ROOT" && FACTORY_HOME="$real_home" "$FACTORY_BIN" project which 2>/dev/null)" || source=""
    fi
    if [ -z "$source" ]; then
        credentials="credentials: login (no key project)"
        return 0
    fi
    found="$(key_lines_of "$source")"
    if [ -z "$found" ]; then
        credentials="credentials: login (no key settings in $source)"
        return 0
    fi
    # Drop the source note and make a leading ~/ absolute.
    key_lines="$(printf '%s\n' "$found" | sed -e 's/ ([^()]*)$//' -e "s|= \"~/|= \"$HOME/|")"
    file="$(printf '%s\n' "$key_lines" | sed -n 's/^api_key_file = "\(.*\)"$/\1/p')"
    if [ -n "$file" ]; then
        credentials="credentials: api_key_file $file (from $source)"
    else
        credentials="credentials: $(printf '%s' "$key_lines" | tr '\n' ' ')(from $source)"
    fi
}

register_project() {
    copy_key_settings
    export FACTORY_HOME="$work/home"
    factory project add "$repo" --preset node --name "$PROJECT" --agent-cli "$harness" >/dev/null
    local toml="$FACTORY_HOME/projects/$PROJECT/project.toml"
    [ -f "$toml" ] || fail "project.toml not found at $toml."
    cp "$toml" "$work/project.toml.orig"
    # The budget key goes into [settings], the check table at the end of the file.
    sed -i 's/^\[settings\]$/[settings]\nbudget_weighted = 300000/' "$toml"
    if [ -n "$key_lines" ]; then
        printf '%s\n' "$key_lines" >"$work/key-lines"
        sed -i "/^\[settings\]\$/r $work/key-lines" "$toml"
    fi
    echo "$credentials"
    cat >>"$toml" <<'EOF'

[[settings.checks]]
paths = ["**"]
commands = ["sh check.sh"]
EOF
    local shown
    shown="$(factory project show "$PROJECT")"
    if ! printf '%s\n' "$shown" | tail -n 1 | grep -q '^problems: none$'; then
        # An unreadable key file is a warning here: the first agent run reports it.
        local problems
        problems="$(printf '%s\n' "$shown" | sed -n '/^problems:$/,$p' | tail -n +2 | grep -v 'api_key_file' || true)"
        if [ -n "$problems" ] || ! printf '%s\n' "$shown" | grep -q '^problems:$'; then
            printf '%s\n' "$shown" >&2
            fail "the throwaway project has problems; see above."
        fi
        echo -e "${YELLOW}Warning: the key file is not readable; see 'factory project show'.${NC}" >&2
    fi
    mkdir -p "$out"
    cp "$toml" "$out/project.toml"
}

run_probe() {
    if [ "$no_probe" -eq 1 ]; then
        echo -e "${YELLOW}Probe skipped (--no-probe).${NC}"
        return 0
    fi
    echo -e "${BLUE}Probing $harness...${NC}"
    mkdir -p "$out/probe"
    local ok=1
    factory --project "$PROJECT" probe --harness "$harness" --json >"$out/probe/report.json" || ok=0
    # The probe writes its report and transcripts under the project state dir.
    [ ! -d "$state/probes" ] || cp -R "$state/probes/." "$out/probe/"
    if [ "$ok" -eq 0 ]; then
        # Each channel object prints name, expected, present, detail in that order.
        awk -F'"' '
            /"expected":/ { expected = ($0 ~ /true/) }
            /"present":/ { present = ($0 ~ /true/) }
            /"detail":/ && expected && !present { print "  missing: " $4 }
        ' "$out/probe/report.json" >&2
        fail "marker probe failed (see $out/probe). The harness is not loading the factory's agent text or skills as expected; no job was run."
    fi
}

run_job() {
    echo -e "${BLUE}Running job $JOB...${NC}"
    factory --project "$PROJECT" new greeting "Add greeting.txt" \
        --plans "$work/plans" --spec "$work/spec.md" >/dev/null
    # A parked job is a normal result here; the stage check below judges it.
    factory --project "$PROJECT" run "$JOB" >"$work/run.out" 2>&1 || true
    cat "$work/run.out"
}

save_evidence() {
    mkdir -p "$out"
    local f
    for f in board.json events.jsonl log.jsonl; do
        [ -f "$state/$f" ] && cp "$state/$f" "$out/$f"
    done
    if [ -d "$state/jobs" ]; then
        mkdir -p "$out/jobs"
        cp -R "$state/jobs/." "$out/jobs/"
    fi
    factory --project "$PROJECT" show "$JOB" >"$out/show-$JOB.json" 2>&1 || true
    factory --project "$PROJECT" status >"$out/status.txt" 2>&1 || true
}

assert_pass() {
    stage="$(grep -o '"stage": *"[^"]*"' "$out/show-$JOB.json" | head -n 1 | sed 's/.*: *"\(.*\)"/\1/')"
    stage="${stage:-unknown}"
    if [ "$stage" != "merged" ]; then
        local reason="none" park kind
        park="$(find "$out/jobs" -name park.json 2>/dev/null | head -n 1)"
        if [ -n "$park" ]; then
            reason="$(grep -o '"reason": *"[^"]*"' "$park" | head -n 1 | sed 's/.*: *"\(.*\)"/\1/')"
            kind="$(grep -o '"kind": *"[^"]*"' "$park" | head -n 1 | sed 's/.*: *"\(.*\)"/\1/')"
            reason="${kind:-unknown}: ${reason:-none}"
        fi
        fail "job $JOB ended in $stage ($reason), expected merged. See $out/jobs."
    fi
    git -C "$repo" cat-file -e main:greeting.txt 2>/dev/null ||
        fail "job merged but greeting.txt is not on main."
    [ -s "$out/board.json" ] || fail "transcript missing: board.json."
    [ -s "$out/events.jsonl" ] || fail "transcript missing: events.jsonl."
    # A clean first pass writes no builder-r*.md (only fix rounds do), so the
    # builder run is proven by its line in usage.jsonl.
    grep -qs '"role": *"builder"' "$out"/jobs/*/usage.jsonl ||
        fail "transcript missing: builder run in usage.jsonl."
    if [ -z "$(find "$out/jobs" -name 'review-code-r*.md' -size +0c | head -n 1)" ]; then
        fail "transcript missing: review-code-r*.md."
    fi
}

pane() {
    "$TMUX_BIN" capture-pane -p -t "$session" -S -200
}

# Polls the tmux pane once per second for 120 s until it matches an extended
# regex. Prints the last 20 pane lines and fails on a timeout.
wait_for() {
    local pattern="$1" what="$2" i
    for i in $(seq 1 120); do
        if pane | grep -Eiq -- "$pattern"; then
            return 0
        fi
        sleep 1
    done
    pane | tail -n 20 >&2
    fail "timed out waiting for $what."
}

# Drives claude in a detached tmux session and checks that /factory status
# reads the throwaway project. Follows the superpowers porting guide, Part 5.
interactive_step() {
    echo -e "${BLUE}Driving claude in tmux...${NC}"
    mkdir -p "$out/interactive"
    session="factory-smoke-$$"
    "$TMUX_BIN" new-session -d -s "$session" -x 200 -y 50 -c "$repo" \
        "env FACTORY_HOME=$FACTORY_HOME claude"
    wait_for 'trust this folder|\? for shortcuts' "the claude prompt"
    if pane | grep -qi 'trust this folder'; then
        "$TMUX_BIN" send-keys -t "$session" Enter
    fi
    wait_for '\? for shortcuts' "the input prompt"
    "$TMUX_BIN" send-keys -t "$session" -l '/factory status'
    sleep 1
    "$TMUX_BIN" send-keys -t "$session" Enter
    # The typed command is in the pane too, so match output words only.
    wait_for 'merged' "/factory status to show the merged job"
    pane >"$out/interactive/pane.txt"
    grep -q '001' "$out/interactive/pane.txt" ||
        fail "/factory status did not show job 001. See $out/interactive/pane.txt."
    "$TMUX_BIN" send-keys -t "$session" -l '/exit'
    sleep 1
    "$TMUX_BIN" send-keys -t "$session" Enter
    "$TMUX_BIN" kill-session -t "$session" 2>/dev/null || true
}

main() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --harness)
                [ $# -ge 2 ] || die_usage "--harness needs a value."
                harness="$2"
                shift 2
                ;;
            --interactive) interactive=1; shift ;;
            --no-probe) no_probe=1; shift ;;
            --keep) keep=1; shift ;;
            --out)
                [ $# -ge 2 ] || die_usage "--out needs a value."
                out="$2"
                shift 2
                ;;
            --key-from)
                [ $# -ge 2 ] || die_usage "--key-from needs a value."
                key_from="$2"
                shift 2
                ;;
            -h|--help) usage; exit 0 ;;
            *) die_usage "Unknown argument: $1" ;;
        esac
    done
    case "$harness" in
        claude|codex) ;;
        *) die_usage "Unknown harness: $harness" ;;
    esac

    if [ "$interactive" -eq 1 ] && [ "$harness" != "claude" ]; then
        die_usage "--interactive supports claude only."
    fi

    TMUX_BIN="${TMUX_BIN:-tmux}"
    if [ -z "${FACTORY_BIN:-}" ]; then
        if [ -x "$REPO_ROOT/target/debug/factory" ]; then
            FACTORY_BIN="$REPO_ROOT/target/debug/factory"
        else
            FACTORY_BIN="factory"
        fi
    fi
    need "$FACTORY_BIN"
    need git
    need "${LIVE_SMOKE_AGENT_PROGRAM:-$harness}"
    [ "$interactive" -eq 0 ] || need "$TMUX_BIN"

    if [ -z "$out" ]; then
        out="${TMPDIR:-/tmp}/factory-live-smoke/$(date -u +%Y%m%dT%H%M%SZ)-$harness"
    fi
    mkdir -p "$out"
    out="$(cd "$out" && pwd)"

    work="$(mktemp -d "${TMPDIR:-/tmp}/factory-live-smoke-work.XXXXXX")"
    trap cleanup EXIT INT TERM
    state="$work/home/projects/$PROJECT/state"
    cli_version="$("${LIVE_SMOKE_AGENT_PROGRAM:-$harness}" --version 2>/dev/null | head -n 1 || true)"
    cli_version="${cli_version:-unknown}"
    stage=""

    if [ "$interactive" -eq 1 ] && [ ! -f "$HOME/.claude/commands/factory.md" ]; then
        fail "/factory is not installed (run factory install first)."
    fi

    build_fixture
    register_project
    run_probe
    run_job
    save_evidence
    assert_pass
    [ "$interactive" -eq 0 ] || interactive_step

    passed=1
    write_summary "PASS"
    echo -e "${GREEN}PASS: job $JOB merged. Evidence: $out${NC}"
}

main "$@"
