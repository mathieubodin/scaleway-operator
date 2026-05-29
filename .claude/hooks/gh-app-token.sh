#!/usr/bin/env bash
set -euo pipefail

ENV_FILE="$HOME/.config/gh-apps/agentic-assistant-1.env"
CACHE_DIR="$HOME/.cache/gh-apps"
CACHE_FILE="$CACHE_DIR/agentic-assistant-1.token"
TTL_SECONDS=3300  # 55 minutes

# Graceful degradation: credentials absent → silently exit (agent runs as maintainer)
[ -f "$ENV_FILE" ] || exit 0

# shellcheck source=/dev/null
source "$ENV_FILE"

# Use cached token if still fresh
if [ -f "$CACHE_FILE" ]; then
  age=$(( $(date +%s) - $(date -r "$CACHE_FILE" +%s) ))
  if [ "$age" -lt "$TTL_SECONDS" ]; then
    cat "$CACHE_FILE"
    exit 0
  fi
fi

# Generate new token via gh-token extension
mkdir -p "$CACHE_DIR"
token=$(gh token generate \
  --app-id "$APP_ID" \
  --key "$HOME/.config/gh-apps/agentic-assistant-1.pem" \
  --installation-id "$INSTALLATION_ID" \
  | jq -r '.token')

# Atomic write: pre-create with correct permissions, then rename
install -m 600 /dev/null "$CACHE_FILE.tmp"
printf '%s' "$token" > "$CACHE_FILE.tmp"
mv "$CACHE_FILE.tmp" "$CACHE_FILE"

printf '%s' "$token"
