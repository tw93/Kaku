#!/bin/bash
# Kaku config version check

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

CURRENT_CONFIG_VERSION=6
VERSION_FILE="$HOME/.config/kaku/.kaku_config_version"

# Determine resource dir
if [[ -d "/Applications/Kaku.app/Contents/Resources" ]]; then
	RESOURCE_DIR="/Applications/Kaku.app/Contents/Resources"
else
	RESOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

user_version=0
if [[ -f "$VERSION_FILE" ]]; then
	user_version=$(cat "$VERSION_FILE")
fi

# Skip if already up to date or new user
if [[ $user_version -eq 0 || $user_version -ge $CURRENT_CONFIG_VERSION ]]; then
	exit 0
fi

echo ""
echo -e "${BOLD}Kaku config update available!${NC} v$user_version -> v$CURRENT_CONFIG_VERSION"
echo ""

# Show what's new
echo -e "${BOLD}What's new:${NC}"
if [[ $user_version -lt 2 ]]; then
	echo "  • 40% faster ZSH startup"
	echo "  • Deferred syntax highlighting"
	echo "  • Delta - syntax-highlighted git diffs"
	echo "  • Better aliases"
fi
if [[ $user_version -lt 3 ]]; then
	echo "  • More reliable setup path detection"
	echo "  • Respect ZDOTDIR when patching .zshrc"
	echo "  • Prevent repeated first-run onboarding loops"
fi
if [[ $user_version -lt 4 ]]; then
	echo "  • Delta defaults to side-by-side with line numbers"
	echo "  • Mouse wheel scrolling enabled in diff pager"
	echo "  • Cleaner file labels and theme-aligned highlighting"
fi
if [[ $user_version -lt 5 ]]; then
	echo "  • Refined diff header display to avoid duplicate file hints"
	echo "  • Updated Delta default theme and label readability"
	echo "  • Better protection for user custom kaku.lua during onboarding"
fi
if [[ $user_version -lt 6 ]]; then
	echo "  • Added zsh-completions to default shell setup"
	echo "  • Richer command and subcommand Tab completion coverage"
	echo "  • Tab now accepts inline autosuggestions first"
	echo "  • If no suggestion is shown, Tab still performs normal completion"
fi
echo ""

read -p "Apply update? [Y/n] " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Nn]$ ]]; then
	mkdir -p "$(dirname "$VERSION_FILE")"
	echo "$CURRENT_CONFIG_VERSION" >"$VERSION_FILE"
	echo -e "${YELLOW}Skipped${NC}"
	echo ""
	echo "Press any key to continue..."
	read -n 1 -s
	exit 0
fi

echo ""

# Apply updates
if [[ -f "$RESOURCE_DIR/setup_zsh.sh" ]]; then
	bash "$RESOURCE_DIR/setup_zsh.sh" --update-only
fi

if ! command -v delta &>/dev/null; then
	if [[ -f "$RESOURCE_DIR/install_delta.sh" ]]; then
		echo ""
		read -p "Install Delta for better git diffs? [Y/n] " -n 1 -r
		echo ""
		if [[ ! $REPLY =~ ^[Nn]$ ]]; then
			bash "$RESOURCE_DIR/install_delta.sh"
		fi
	fi
fi

mkdir -p "$(dirname "$VERSION_FILE")"
echo "$CURRENT_CONFIG_VERSION" >"$VERSION_FILE"

echo ""
echo -e "${GREEN}${BOLD}Updated to v$CURRENT_CONFIG_VERSION!${NC}"
echo ""
echo "Press any key to start..."
read -n 1 -s

# Start a new shell instead of exiting
exec zsh -l
