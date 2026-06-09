#!/usr/bin/env bash
set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HOOKS_SRC="$REPO_ROOT/.claude/hooks"
HOOKS_DEST="$HOME/.claude/hooks"
SETTINGS_FILE="$HOME/.claude/settings.json"
GIT_HOOKS_DIR="$REPO_ROOT/.git/hooks"

echo "=== setup-dev.sh ==="
echo ""

# ---------------------------------------------------------------------------
# 1. Prérequis
# ---------------------------------------------------------------------------
echo "1. Vérification des prérequis..."

if ! command -v jq &>/dev/null; then
    printf "${RED}✗ jq introuvable.${NC} Installer via le gestionnaire de paquets système.\n"
    exit 1
fi

printf "  ${GREEN}✓ jq OK${NC}\n"

# ---------------------------------------------------------------------------
# 2. Copie des hooks Claude Code vers ~/.claude/hooks/
# ---------------------------------------------------------------------------
echo ""
echo "2. Installation des hooks Claude Code..."

mkdir -p "$HOOKS_DEST"

for hook in session-start-tokens.sh stop-tokens.sh; do
    src="$HOOKS_SRC/$hook"
    dest="$HOOKS_DEST/$hook"
    if [ ! -f "$src" ]; then
        printf "  ${RED}✗ Source absente : $src${NC}\n"
        exit 1
    fi
    cp "$src" "$dest"
    chmod +x "$dest"
    printf "  ${GREEN}✓ $hook → $dest${NC}\n"
done

# ---------------------------------------------------------------------------
# 3. Enregistrement dans ~/.claude/settings.json
# ---------------------------------------------------------------------------
echo ""
echo "3. Enregistrement dans ~/.claude/settings.json..."

if [ ! -f "$SETTINGS_FILE" ]; then
    printf "  ${YELLOW}⚠ settings.json absent — création d'un fichier vide${NC}\n"
    echo '{}' > "$SETTINGS_FILE"
fi

# Backup
cp "$SETTINGS_FILE" "${SETTINGS_FILE}.bak"
printf "  Backup → ${SETTINGS_FILE}.bak\n"

SESSION_START_CMD="$HOOKS_DEST/session-start-tokens.sh"
STOP_CMD="$HOOKS_DEST/stop-tokens.sh"

# Idempotent SessionStart entry
if jq -e --arg cmd "$SESSION_START_CMD" \
    '(.hooks.SessionStart // []) | map(.hooks[]?.command // "") | map(select(. == $cmd)) | length > 0' \
    "$SETTINGS_FILE" >/dev/null 2>&1; then
    printf "  ${YELLOW}⚠ SessionStart déjà enregistré — skip${NC}\n"
else
    jq --arg cmd "$SESSION_START_CMD" \
        '.hooks.SessionStart = ((.hooks.SessionStart // []) + [{"matcher":".*","hooks":[{"type":"command","command":$cmd}]}])' \
        "$SETTINGS_FILE" > "${SETTINGS_FILE}.tmp"
    jq . "${SETTINGS_FILE}.tmp" >/dev/null
    mv "${SETTINGS_FILE}.tmp" "$SETTINGS_FILE"
    printf "  ${GREEN}✓ SessionStart enregistré${NC}\n"
fi

# Idempotent Stop entry
if jq -e --arg cmd "$STOP_CMD" \
    '(.hooks.Stop // []) | map(.command // "") | map(select(. == $cmd)) | length > 0' \
    "$SETTINGS_FILE" >/dev/null 2>&1; then
    printf "  ${YELLOW}⚠ Stop déjà enregistré — skip${NC}\n"
else
    jq --arg cmd "$STOP_CMD" \
        '.hooks.Stop = ((.hooks.Stop // []) + [{"type":"command","command":$cmd,"async":true}])' \
        "$SETTINGS_FILE" > "${SETTINGS_FILE}.tmp"
    jq . "${SETTINGS_FILE}.tmp" >/dev/null
    mv "${SETTINGS_FILE}.tmp" "$SETTINGS_FILE"
    printf "  ${GREEN}✓ Stop enregistré${NC}\n"
fi

# ---------------------------------------------------------------------------
# 4. Symlink prepare-commit-msg dans .git/hooks/
# ---------------------------------------------------------------------------
echo ""
echo "4. Installation du hook git prepare-commit-msg..."

HOOK_SRC="$REPO_ROOT/.githooks/prepare-commit-msg"
HOOK_DEST="$GIT_HOOKS_DIR/prepare-commit-msg"

if [ ! -f "$HOOK_SRC" ]; then
    printf "  ${RED}✗ Source absente : $HOOK_SRC${NC}\n"
    exit 1
fi

chmod +x "$HOOK_SRC"

if [ -L "$HOOK_DEST" ] && [ "$(readlink "$HOOK_DEST")" = "$HOOK_SRC" ]; then
    printf "  ${YELLOW}⚠ Symlink déjà en place — skip${NC}\n"
else
    ln -sf "$HOOK_SRC" "$HOOK_DEST"
    printf "  ${GREEN}✓ .git/hooks/prepare-commit-msg → $HOOK_SRC${NC}\n"
fi

# ---------------------------------------------------------------------------
# Résumé
# ---------------------------------------------------------------------------
echo ""
printf "${GREEN}=== Setup terminé ===${NC}\n"
echo ""
echo "Hooks actifs après redémarrage de Claude Code :"
echo "  SessionStart : $SESSION_START_CMD"
echo "  Stop         : $STOP_CMD"
echo "  git          : .git/hooks/prepare-commit-msg"
echo ""
echo "Chaque commit portera automatiquement :"
echo "  Claude-Session: <8-char session id>"
echo "  Claude-Tokens-Delta: <tokens depuis dernier commit>"
echo "  Claude-Tokens-Total: <cumul session>"
