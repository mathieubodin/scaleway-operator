#!/usr/bin/env bash
# Crée les tags et les releases de la PR de release mergée, seulement si les tests
# d'intégration sont verts sur son commit de merge. La publication de l'image et des
# charts est ensuite déclenchée par release.yml.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/release-common.sh
source scripts/release-common.sh

require_gh_repo_scope

pr=$(release_pr_number merged)
if [ -z "$pr" ]; then
    echo "[release] Aucune PR de release mergée en attente de publication."
    exit 0
fi

sha=$(gh pr view "$pr" --repo "$RELEASE_REPO" --json mergeCommit --jq '.mergeCommit.oid')
conclusion=$(gh run list --repo "$RELEASE_REPO" --workflow integration.yml --commit "$sha" \
    --json status,conclusion --jq '.[0] | if . == null then "absent" elif .status != "completed" then "en cours" else .conclusion end')

if [ "$conclusion" != "success" ]; then
    echo "ERREUR : tests d'intégration non verts sur ${sha} (état : ${conclusion})." >&2
    echo "Rien n'a été publié. Vérifier : gh run list --workflow integration.yml --commit ${sha}" >&2
    exit 1
fi

echo "[release] Tests d'intégration verts sur ${sha} : création des tags et releases de la PR #${pr}..."
run_release_please github-release
