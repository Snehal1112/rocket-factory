#!/usr/bin/env bash
# A stand-in for `claude`, used only by the live smoke script test.
# It answers `plugin list --json` and the stream JSON of one headless run.

if [ "${1:-}" = "plugin" ]; then
    echo '[]'
    exit 0
fi

args="$*"
stdin_text="$(cat)"
text="$args
$stdin_text"

emit() { printf '%s\n' "$1"; }

# Escapes backslash, quote and newline for a JSON string.
json_escape() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | awk 'BEGIN{ORS="\\n"} {print}'
}

result() {
    emit "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"$(json_escape "$1")\",\"session_id\":\"shim-1\"}"
}

emit '{"type":"system","subtype":"init","session_id":"shim-1"}'
emit '{"type":"assistant","message":{"id":"m1","content":[],"usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":5}},"session_id":"shim-1"}'

case "$text" in
*factory-spec-reviewer*)
    result "The spec is fine.
VERDICT: APPROVE"
    ;;
*factory-builder*)
    # Appending makes every fix round change the tree, like a real builder.
    echo hello >>greeting.txt
    result "built greeting.txt"
    ;;
*factory-committer*)
    git add -A
    git -c user.name=shim -c user.email=shim@example.com commit -q -m "feat: add greeting.txt"
    result "committed"
    ;;
*factory-code-reviewer*)
    if [ "${SHIM_REVIEW:-}" = "changes" ]; then
        result "Not fine.
- [blocker] x
VERDICT: CHANGES"
    else
        result "fine
VERDICT: APPROVE"
    fi
    ;;
*)
    result "NONE"
    ;;
esac
