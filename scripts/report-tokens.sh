#!/usr/bin/env bash
# Reporte le coût en tokens d'une PR dans le champ Tokens de son élément du Project GitHub,
# celui qu'affiche le panneau « Projects » de la PR.
#
# Le coût est porté par la PR et non par l'issue : une PR qui ferme plusieurs issues
# n'est comptée qu'une fois, et une issue traitée par plusieurs PRs ne perd aucun relevé.
#
# Chiffre retenu : les tokens produits par l'assistant (output_tokens), sous-agents
# compris. Les tokens de cache sont exclus : ils dépendent des pauses dans la session,
# pas du travail accompli.
#
# Attribution : les tokens produits depuis le dernier relevé de la PR précédente dans
# la même session. Le repère est gardé dans .git/claude-token-report.json (non versionné).
#
# Usage :
#   scripts/report-tokens.sh [PR]     relevé manuel (PR de la branche courante par défaut)
#   scripts/report-tokens.sh --hook   appelé par un hook PostToolUse de Claude Code
#                                     (lance aussi sync-pr-metadata.sh après `gh pr create`)
set -euo pipefail

PROJECT_OWNER="mathieubodin"
PROJECT_NUMBER=2
PROJECT_ID="PVT_kwHOAAJUjc4BYpzh"
TOKENS_FIELD_ID="PVTF_lAHOAAJUjc4BYpzhzhT6_3I"

cd "$(dirname "$0")/.."
STATE_FILE="$(git rev-parse --git-dir)/claude-token-report.json"

hook_mode=false
pr=""
transcript="${CLAUDE_TRANSCRIPT:-}"

if [ "${1:-}" = "--hook" ]; then
    hook_mode=true
    # Un hook ne doit jamais faire échouer la commande qui l'a déclenché.
    trap 'exit 0' ERR
    input=$(cat)
    command=$(jq -r '.tool_input.command // ""' <<<"$input")
    case "$command" in
        *"gh pr create"* | *"git push"*) ;;
        *) exit 0 ;;
    esac
    transcript=$(jq -r '.transcript_path // ""' <<<"$input")
    # À l'ouverture d'une PR, lui recopier d'abord les métadonnées de son issue.
    case "$command" in
        *"gh pr create"*) bash scripts/sync-pr-metadata.sh || true ;;
    esac
else
    pr="${1:-}"
fi

if [ -z "$pr" ]; then
    pr=$(gh pr view --json number --jq '.number' 2>/dev/null || true)
fi
if [ -z "$pr" ]; then
    $hook_mode && exit 0
    echo "ERREUR : aucune PR pour la branche courante. Préciser : make report-tokens PR=<numéro>" >&2
    exit 1
fi

if [ -z "$transcript" ]; then
    # Journal de session le plus récent pour ce dépôt.
    slug=$(pwd | sed 's#[/.]#-#g')
    transcript=$(ls -t "$HOME/.claude/projects/$slug"/*.jsonl 2>/dev/null | head -1 || true)
fi
if [ -z "$transcript" ] || [ ! -f "$transcript" ]; then
    echo "ERREUR : journal de session Claude Code introuvable (CLAUDE_TRANSCRIPT pour le préciser)." >&2
    exit 1
fi
session=$(basename "$transcript" .jsonl)
subagents=("${transcript%.jsonl}"/subagents/*.jsonl)
[ -f "${subagents[0]}" ] || subagents=()

[ -f "$STATE_FILE" ] || echo '{"cursors": {}, "prs": {}}' >"$STATE_FILE"
now=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)

# Début de la période : inchangé si la session travaille toujours sur cette PR,
# sinon le dernier relevé de la PR précédente (ou le début de la session).
since=$(jq -r --arg s "$session" --arg pr "$pr" '
    .cursors[$s] as $c
    | if $c == null then ""
      elif ($c.pr | tostring) == $pr then $c.since
      else $c.last end' "$STATE_FILE")

session_tokens=$(cat "$transcript" "${subagents[@]}" | jq -c --arg since "$since" '
    select(.type == "assistant" and .message.usage != null and .timestamp != null and .timestamp > $since)
    | {id: .message.id, tokens: (.message.usage.output_tokens // 0)}' |
    jq -s 'unique_by(.id) | map(.tokens) | add // 0')

tmp=$(mktemp)
jq --arg s "$session" --arg pr "$pr" --arg since "$since" --arg now "$now" --argjson t "$session_tokens" '
    .cursors[$s] = {pr: ($pr | tonumber), since: $since, last: $now}
    | .prs[$pr][$s] = $t' "$STATE_FILE" >"$tmp" && mv "$tmp" "$STATE_FILE"

# Une PR travaillée sur plusieurs sessions cumule leurs relevés.
total=$(jq --arg pr "$pr" '.prs[$pr] | add' "$STATE_FILE")

# `item-add` est idempotent : il renvoie l'élément existant si la PR est déjà dans le Project.
pr_url=$(gh pr view "$pr" --json url --jq '.url')
item_id=$(gh project item-add "$PROJECT_NUMBER" --owner "$PROJECT_OWNER" --url "$pr_url" --format json --jq '.id')
gh project item-edit --project-id "$PROJECT_ID" --id "$item_id" \
    --field-id "$TOKENS_FIELD_ID" --number "$total" >/dev/null
echo "[tokens] PR #${pr} : ${total} tokens produits."
