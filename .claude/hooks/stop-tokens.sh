#!/usr/bin/env bash
set -euo pipefail

command -v jq >/dev/null 2>&1 || exit 0

input=$(cat)

if printf '%s' "$input" | jq -e '.stop_hook_active == false' >/dev/null 2>&1; then
    exit 0
fi

transcript_path=$(printf '%s' "$input" | jq -r '.transcript_path // empty' 2>/dev/null) || exit 0
[ -n "$transcript_path" ] || exit 0
[ -f "$transcript_path" ] || exit 0

GIT_DIR=$(git rev-parse --git-dir 2>/dev/null) || exit 0

total=$(jq -s '[.[] | select(.type=="assistant") | (.usage.input_tokens // 0) + (.usage.output_tokens // 0) + (.usage.cache_creation_input_tokens // 0)] | add // 0' "$transcript_path" 2>/dev/null) || exit 0

tmpfile=$(mktemp)
printf '%d' "$total" > "$tmpfile"
mv "$tmpfile" "$GIT_DIR/claude_tokens_session"
