#!/usr/bin/env bash
# Recopie sur une PR les métadonnées de l'issue qu'elle ferme par `Closes #N`,
# ou à défaut de la première qu'elle cite par `Refs #N` :
# milestone, labels, assignation, et ajout au Project GitHub avec Axis, Priority et Effort.
# GitHub ne propage rien de l'issue vers la PR : sans cela, la PR n'affiche ni son
# milestone ni son projet.
#
# Les vues du Project filtrent sur `is:issue`, pour que la PR et son issue ne fassent
# pas doublon sur le board.
#
# Usage : scripts/sync-pr-metadata.sh [PR]   (PR de la branche courante par défaut)
set -euo pipefail

PROJECT_OWNER="mathieubodin"
PROJECT_NUMBER=2
PROJECT_ID="PVT_kwHOAAJUjc4BYpzh"
COPIED_FIELDS=("Axis" "Priority" "Effort")

cd "$(dirname "$0")/.."

pr="${1:-}"
if [ -z "$pr" ]; then
    pr=$(gh pr view --json number --jq '.number' 2>/dev/null || true)
fi
if [ -z "$pr" ]; then
    echo "ERREUR : aucune PR pour la branche courante. Préciser : make sync-pr-metadata PR=<numéro>" >&2
    exit 1
fi

pr_json=$(gh pr view "$pr" --json url,state,body,closingIssuesReferences)
pr_url=$(jq -r '.url' <<<"$pr_json")
pr_state=$(jq -r '.state' <<<"$pr_json")
# Issue de référence : celle que la PR ferme, sinon la première citée par `Refs #N`
# (PR qui complète une issue sans la fermer).
issue=$(jq -r '.closingIssuesReferences[0].number // empty' <<<"$pr_json")
if [ -z "$issue" ]; then
    issue=$(jq -r '.body // ""' <<<"$pr_json" | grep -oiE '\brefs #[0-9]+' | head -1 | grep -oE '[0-9]+' || true)
fi

gh pr edit "$pr" --add-assignee "@me" >/dev/null

fields=$(gh project field-list "$PROJECT_NUMBER" --owner "$PROJECT_OWNER" --format json)
field_id() { jq -r --arg f "$1" '.fields[] | select(.name == $f) | .id' <<<"$fields"; }
option_id() { jq -r --arg f "$1" --arg o "$2" '.fields[] | select(.name == $f) | .options[] | select(.name == $o) | .id' <<<"$fields"; }
set_option() { # item_id, champ, valeur
    local option
    option=$(option_id "$2" "$3")
    [ -n "$option" ] || return 0
    gh project item-edit --project-id "$PROJECT_ID" --id "$1" \
        --field-id "$(field_id "$2")" --single-select-option-id "$option" >/dev/null
}

pr_item=$(gh project item-add "$PROJECT_NUMBER" --owner "$PROJECT_OWNER" --url "$pr_url" --format json --jq '.id')
# L'automatisation native ne passe en Done qu'au moment du merge : poser l'état nous-mêmes
# couvre aussi une PR ajoutée après coup.
case "$pr_state" in
    MERGED) set_option "$pr_item" "Status" "Done" ;;
    OPEN) set_option "$pr_item" "Status" "Review" ;;
esac

if [ -z "$issue" ]; then
    echo "[metadata] PR #${pr} : assignée et ajoutée au Project. Aucune issue liée par Closes ni citée par Refs : milestone, labels et champs à compléter."
    exit 0
fi

issue_json=$(gh issue view "$issue" --json milestone,labels)
milestone=$(jq -r '.milestone.title // empty' <<<"$issue_json")
labels=$(jq -r '[.labels[].name] | join(",")' <<<"$issue_json")
[ -z "$milestone" ] || gh pr edit "$pr" --milestone "$milestone" >/dev/null
[ -z "$labels" ] || gh pr edit "$pr" --add-label "$labels" >/dev/null

items=$(gh project item-list "$PROJECT_NUMBER" --owner "$PROJECT_OWNER" --limit 500 --format json)
for field in "${COPIED_FIELDS[@]}"; do
    key=$(tr '[:upper:]' '[:lower:]' <<<"$field")
    value=$(jq -r --argjson n "$issue" --arg k "$key" '.items[] | select(.content.number == $n and .content.type == "Issue") | .[$k] // empty' <<<"$items")
    [ -z "$value" ] || set_option "$pr_item" "$field" "$value"
done

echo "[metadata] PR #${pr} ← issue #${issue} : milestone « ${milestone:-aucun} », labels [${labels}], assignée, ajoutée au Project."
