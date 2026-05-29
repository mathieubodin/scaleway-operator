#!/usr/bin/env bash
set -euo pipefail

ENV_FILE="$HOME/.config/gh-apps/agentic-assistant-1.env"
HOOK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Read full JSON payload from stdin
input=$(cat)

# Only act on Bash tool calls
tool_name=$(printf '%s' "$input" | jq -r '.tool_name')
[ "$tool_name" = "Bash" ] || exit 0

# Graceful degradation: no credentials → pass through unchanged
[ -f "$ENV_FILE" ] || exit 0

# shellcheck source=/dev/null
source "$ENV_FILE"

command=$(printf '%s' "$input" | jq -r '.tool_input.command')
modified="$command"

# --- GH_TOKEN injection ---
# Match `gh ` at CLI boundary: start-of-string, space, semicolon, pipe, or ampersand.
# Skip if command already sets GH_TOKEN= explicitly (e.g. GH_TOKEN=$GH_PROJECT_TOKEN gh api graphql).
if printf '%s' "$command" | grep -qE '(^|[ ;|&])gh ' && \
   ! printf '%s' "$command" | grep -q 'GH_TOKEN='; then

  token=$("$HOOK_DIR/gh-app-token.sh" 2>/dev/null) || token=""

  if [ -n "$token" ]; then
    # Non-blocking warning: gh api graphql without explicit GH_TOKEN= will fail
    # with "Resource not accessible by integration" if it targets Projects v2.
    if printf '%s' "$command" | grep -q 'gh api graphql'; then
      printf '[agent-identity] warning: gh api graphql without explicit GH_TOKEN= — Projects v2 mutations must prefix GH_TOKEN=$GH_PROJECT_TOKEN\n' >&2
    fi
    modified="GH_TOKEN=$token $modified"
  fi
fi

# --- GIT_AUTHOR injection ---
# BOT_USER_ID is populated by scripts/setup-agent-identity.sh and stored in the .env file.
if printf '%s' "$command" | grep -qE '(^|[ ;|&])git +commit' && \
   [ -n "${BOT_USER_ID:-}" ]; then
  bot_email="${BOT_USER_ID}+agentic-assistant-1[bot]@users.noreply.github.com"
  modified="GIT_AUTHOR_NAME='agentic-assistant-1[bot]' GIT_AUTHOR_EMAIL='$bot_email' $modified"
fi

# Output modified JSON only if command changed; otherwise exit silently (no output = pass through)
if [ "$modified" != "$command" ]; then
  printf '%s' "$input" | jq --arg cmd "$modified" '.tool_input.command = $cmd'
fi
