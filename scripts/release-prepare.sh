#!/usr/bin/env bash
# Ouvre ou met à jour la PR de release, puis y réapplique les versions de chart du README.
# release-please régénère sa branche à chaque exécution : il ne doit être lancé que par ce
# script, qui rejoue le commit du README juste après.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/release-common.sh
source scripts/release-common.sh

require_gh_repo_scope

merged=$(release_pr_number merged)
if [ -n "$merged" ]; then
    echo "ERREUR : la PR de release #${merged} est mergée mais pas encore publiée." >&2
    echo "Lancer d'abord : make release-publish" >&2
    exit 1
fi

echo "[release] Mise à jour de la PR de release..."
run_release_please release-pr

pr=$(release_pr_number open)
if [ -z "$pr" ]; then
    echo "[release] Aucun commit à publier : pas de PR de release."
    exit 0
fi
branch=$(gh pr view "$pr" --repo "$RELEASE_REPO" --json headRefName --jq '.headRefName')

# release-please ne reconstruit sa branche que si le contenu de la PR change. Quand main
# avance sans rien changer aux notes de release, la branche reste sur l'ancien main et
# finit en conflit. Modifier la description de la PR l'oblige à tout reconstruire.
git fetch --quiet origin main "$branch"
if ! git merge-base --is-ancestor origin/main "origin/$branch"; then
    echo "[release] La PR de release #${pr} est en retard sur main : reconstruction forcée..."
    body=$(gh pr view "$pr" --repo "$RELEASE_REPO" --json body --jq '.body')
    gh pr edit "$pr" --repo "$RELEASE_REPO" --body "${body}

<!-- rebuild -->" >/dev/null
    run_release_please release-pr
    git fetch --quiet origin "$branch"
    if ! git merge-base --is-ancestor origin/main "origin/$branch"; then
        echo "ERREUR : la PR de release #${pr} reste en retard sur main après reconstruction." >&2
        exit 1
    fi
fi

worktree=$(mktemp -d)
trap 'git worktree remove --force "$worktree" >/dev/null 2>&1 || true' EXIT
git worktree add --quiet --detach "$worktree" "origin/$branch"

chart_version() {
    local version
    version=$(grep '^version:' "$worktree/$1/Chart.yaml" | awk '{print $2}')
    if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "ERREUR : version invalide dans $1/Chart.yaml : '${version}'" >&2
        exit 1
    fi
    echo "$version"
}
crds_version=$(chart_version charts/scaleway-operator-crds)
operator_version=$(chart_version charts/scaleway-operator)

sed -i "/ghcr\.io\/mathieubodin\/charts\/scaleway-operator-crds/{n;s/--version [0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*/--version ${crds_version}/}" "$worktree/README.md"
sed -i "/ghcr\.io\/mathieubodin\/charts\/scaleway-operator[^-]/{n;s/--version [0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*/--version ${operator_version}/}" "$worktree/README.md"

if git -C "$worktree" diff --quiet README.md; then
    echo "[release] README déjà à jour (crds=${crds_version}, operator=${operator_version})."
else
    git -C "$worktree" commit --quiet --no-verify README.md \
        -m "chore: sync README helm versions (crds=${crds_version}, operator=${operator_version})"
    git -C "$worktree" push --quiet origin "HEAD:${branch}"
    echo "[release] README mis à jour sur ${branch} (crds=${crds_version}, operator=${operator_version})."
fi

echo "[release] PR de release : $(gh pr view "$pr" --repo "$RELEASE_REPO" --json url --jq '.url')"
