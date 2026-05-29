#!/usr/bin/env bash
set -euo pipefail

# One-time setup: registers the agentic-assistant-1 GitHub App identity for Claude Code.
# Run from the repo root after creating the App and placing credentials in ~/.config/gh-apps/.

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PEM_FILE="$HOME/.config/gh-apps/agentic-assistant-1.pem"
ENV_FILE="$HOME/.config/gh-apps/agentic-assistant-1.env"
HOOK_SCRIPT="$REPO_ROOT/.claude/hooks/gh-app-token.sh"
SETTINGS_FILE="$REPO_ROOT/.claude/settings.local.json"

echo "=== setup-agent-identity.sh ==="
echo ""

# ---------------------------------------------------------------------------
# 1. Prérequis
# ---------------------------------------------------------------------------
echo "1. Vérification des prérequis..."

if ! command -v gh &>/dev/null; then
  printf "${RED}✗ gh CLI introuvable.${NC} Installer depuis https://cli.github.com\n"
  exit 1
fi

if ! command -v jq &>/dev/null; then
  printf "${RED}✗ jq introuvable.${NC} Installer via le gestionnaire de paquets système.\n"
  exit 1
fi

if ! gh token generate --help &>/dev/null 2>&1; then
  echo "  → Installation de gh extension Link-/gh-token..."
  gh extension install Link-/gh-token
fi

if [ ! -f "$PEM_FILE" ]; then
  printf "${RED}✗ Clé privée absente :${NC} $PEM_FILE\n"
  echo "  Télécharger depuis : https://github.com/settings/apps/agentic-assistant-1 → Generate a private key"
  exit 1
fi

if [ ! -f "$ENV_FILE" ]; then
  printf "${RED}✗ Fichier .env absent :${NC} $ENV_FILE\n"
  echo "  Créer le fichier avec APP_ID et INSTALLATION_ID (voir CONTRIBUTING.md)"
  exit 1
fi

# shellcheck source=/dev/null
source "$ENV_FILE"

if [ -z "${APP_ID:-}" ] || [ -z "${INSTALLATION_ID:-}" ]; then
  printf "${RED}✗ APP_ID ou INSTALLATION_ID manquant dans${NC} $ENV_FILE\n"
  exit 1
fi

printf "  ${GREEN}✓ Prérequis OK${NC}\n"

# ---------------------------------------------------------------------------
# 2. Génération du token (déclenche la création lazy du bot user GitHub)
# ---------------------------------------------------------------------------
echo ""
echo "2. Génération du token d'installation..."

token=$("$HOOK_SCRIPT" 2>/dev/null) || token=""

if [ -z "$token" ]; then
  printf "${RED}✗ Échec de la génération du token.${NC}\n"
  echo "  Vérifier APP_ID, INSTALLATION_ID et la clé PEM."
  exit 1
fi

# First authenticated call — GitHub creates the bot user entry lazily on this call.
GH_TOKEN="$token" gh api /app/installations --jq '.[0].id' > /dev/null 2>&1 || true

printf "  ${GREEN}✓ Token généré${NC}\n"

# ---------------------------------------------------------------------------
# 3. Récupération du BOT_USER_ID (avec retry)
# ---------------------------------------------------------------------------
echo ""
echo "3. Récupération du BOT_USER_ID..."

bot_user_id=""
for attempt in 1 2 3; do
  bot_user_id=$(GH_TOKEN="$token" gh api "/users/agentic-assistant-1[bot]" --jq '.id' 2>/dev/null) || bot_user_id=""
  if [ -n "$bot_user_id" ]; then
    break
  fi
  if [ "$attempt" -lt 3 ]; then
    echo "  Tentative $attempt/3 échouée — attente 5s..."
    sleep 5
  fi
done

if [ -z "$bot_user_id" ]; then
  printf "${RED}✗ BOT_USER_ID introuvable après 3 tentatives.${NC}\n"
  echo ""
  echo "  Le bot user GitHub est créé de façon lazy lors du premier appel API authentifié."
  echo "  Solution : exécuter manuellement la commande suivante, puis relancer ce script :"
  echo ""
  echo "    GH_TOKEN=$token gh api /app"
  echo ""
  exit 1
fi

printf "  ${GREEN}✓ BOT_USER_ID=$bot_user_id${NC}\n"

# Écriture dans .env (ajout ou mise à jour)
if grep -q '^BOT_USER_ID=' "$ENV_FILE"; then
  grep -v '^BOT_USER_ID=' "$ENV_FILE" > "$ENV_FILE.tmp"
  echo "BOT_USER_ID=$bot_user_id" >> "$ENV_FILE.tmp"
  mv "$ENV_FILE.tmp" "$ENV_FILE"
else
  echo "BOT_USER_ID=$bot_user_id" >> "$ENV_FILE"
fi
chmod 600 "$ENV_FILE"

# ---------------------------------------------------------------------------
# 4. Enregistrement du hook dans settings.local.json
# ---------------------------------------------------------------------------
echo ""
echo "4. Enregistrement du hook pre-tool-use..."

[ ! -f "$SETTINGS_FILE" ] && echo '{}' > "$SETTINGS_FILE"

HOOK_ENTRY='{"matcher":"Bash","hooks":[{"type":"command","command":".claude/hooks/agent-identity.sh"}]}'

jq --argjson entry "$HOOK_ENTRY" '
  if (.hooks.PreToolUse // [])
     | map(select(.hooks[]?.command // "" | test("agent-identity\\.sh")))
     | length > 0
  then .
  else .hooks.PreToolUse = ((.hooks.PreToolUse // []) + [$entry])
  end
' "$SETTINGS_FILE" > "$SETTINGS_FILE.tmp" \
  && mv "$SETTINGS_FILE.tmp" "$SETTINGS_FILE"

printf "  ${GREEN}✓ Hook enregistré dans .claude/settings.local.json${NC}\n"

# ---------------------------------------------------------------------------
# 5. Validation end-to-end
# ---------------------------------------------------------------------------
echo ""
echo "5. Validation end-to-end..."

app_name=$(GH_TOKEN="$token" gh api /app --jq '.name' 2>/dev/null) || app_name=""
if [ "$app_name" = "agentic-assistant-1" ]; then
  printf "  ${GREEN}✓ App authentifiée : $app_name${NC}\n"
else
  printf "  ${YELLOW}⚠ Impossible de valider l'identité de l'App (reçu : '$app_name')${NC}\n"
fi

# ---------------------------------------------------------------------------
# Résumé
# ---------------------------------------------------------------------------
echo ""
printf "${GREEN}=== Setup terminé ===${NC}\n"
echo ""
echo "Identité agent active :"
echo "  GIT_AUTHOR_NAME  = agentic-assistant-1[bot]"
echo "  GIT_AUTHOR_EMAIL = ${bot_user_id}+agentic-assistant-1[bot]@users.noreply.github.com"
echo ""
echo "Le token GH App est injecté automatiquement sur toutes les commandes gh dans Claude Code."
echo "Pour les mutations Projects v2, préfixer explicitement :"
echo "  GH_TOKEN=\$GH_PROJECT_TOKEN gh api graphql ..."
