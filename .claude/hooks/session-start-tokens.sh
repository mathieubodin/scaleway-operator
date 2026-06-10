#!/usr/bin/env bash
set -euo pipefail

input=$(cat)

GIT_DIR=$(git rev-parse --git-dir 2>/dev/null) || exit 0

session_id=$(printf '%s' "$input" | jq -r '.session_id // empty' 2>/dev/null) || exit 0
[ -n "$session_id" ] || exit 0

prev_session_id=$(cat "$GIT_DIR/claude_session_id" 2>/dev/null) || prev_session_id=""

if [ "$prev_session_id" = "$session_id" ]; then
    exit 0
fi

tokens_session=$(cat "$GIT_DIR/claude_tokens_session" 2>/dev/null) || tokens_session=0
tokens_last=$(cat "$GIT_DIR/claude_tokens_last_commit" 2>/dev/null) || tokens_last=0

[[ "$tokens_session" =~ ^[0-9]+$ ]] || tokens_session=0
[[ "$tokens_last"    =~ ^[0-9]+$ ]] || tokens_last=0

carryover=$(( tokens_session - tokens_last ))
[ "$carryover" -lt 0 ] && carryover=0

printf '%s' "$session_id" > "$GIT_DIR/claude_session_id"
printf '%d' "$carryover" > "$GIT_DIR/claude_tokens_carryover"
# Nouvelle session : repartir d'une baseline vierge — le cumul de l'ancienne
# session est entièrement capturé par le carryover ci-dessus.
printf '0' > "$GIT_DIR/claude_tokens_session"
printf '0' > "$GIT_DIR/claude_tokens_last_commit"
