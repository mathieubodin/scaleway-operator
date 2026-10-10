#!/usr/bin/env bash
# Fonctions partagées par release-prepare.sh et release-publish.sh.
# Tout accès à GitHub passe par l'authentification `gh` du shell local :
# une PR ou une release créée avec ce token déclenche les workflows,
# contrairement au token standard des Actions.

RELEASE_PLEASE_VERSION="${RELEASE_PLEASE_VERSION:-17.11.2}"
RELEASE_REPO="${RELEASE_REPO:-mathieubodin/scaleway-operator}"
RELEASE_PENDING_LABEL="autorelease: pending"

require_gh_repo_scope() {
    local scopes
    if ! scopes=$(gh api -i user 2>/dev/null | grep -i '^x-oauth-scopes:'); then
        echo "ERREUR : gh n'est pas authentifié. Lancer : gh auth login" >&2
        exit 1
    fi
    if ! grep -qw 'repo' <<<"$scopes"; then
        echo "ERREUR : le token gh local n'a pas le scope 'repo', requis pour ouvrir la PR de release." >&2
        echo "Lancer : gh auth refresh -s repo" >&2
        exit 1
    fi
}

run_release_please() {
    npx --yes "release-please@${RELEASE_PLEASE_VERSION}" "$@" \
        --token="$(gh auth token)" \
        --repo-url="$RELEASE_REPO" \
        --config-file=release-please-config.json \
        --manifest-file=.release-please-manifest.json
}

RELEASE_BRANCH="${RELEASE_BRANCH:-release-please--branches--main}"

# Numéro de la PR de release dans l'état donné (open ou merged), vide si aucune.
# La PR est retrouvée par sa branche et non par une recherche sur son label :
# l'index de recherche de GitHub a du retard sur une PR qui vient d'être créée.
release_pr_number() {
    gh pr list --repo "$RELEASE_REPO" --state "$1" --head "$RELEASE_BRANCH" --limit 20 \
        --json number,labels \
        --jq "map(select(any(.labels[]; .name == \"${RELEASE_PENDING_LABEL}\"))) | .[0].number // empty"
}
