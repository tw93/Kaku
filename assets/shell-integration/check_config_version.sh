#!/bin/bash
# Kaku config version check

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

CONFIG_DIR="$HOME/.config/kaku"
STATE_FILE="$CONFIG_DIR/state.json"
LEGACY_VERSION_FILE="$CONFIG_DIR/.kaku_config_version"
LEGACY_GEOMETRY_FILE="$CONFIG_DIR/.kaku_window_geometry"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMON_SCRIPT="$SCRIPT_DIR/state_common.sh"

if [[ ! -f "$COMMON_SCRIPT" ]]; then
	echo -e "${YELLOW}Error: missing shared state script: $COMMON_SCRIPT${NC}"
	exit 1
fi
# shellcheck source=state_common.sh
source "$COMMON_SCRIPT"

CURRENT_CONFIG_VERSION="$(read_bundled_config_version "$SCRIPT_DIR")"

detect_kaku_setup_script() {
	case "${KAKU_TARGET_SHELL:-${SHELL:-/bin/zsh}}" in
		*fish|fish)
			printf 'setup_fish.sh'
			;;
		*)
			printf 'setup_zsh.sh'
			;;
	esac
}

RESOURCE_DIR="$SCRIPT_DIR"
SETUP_SCRIPT="$RESOURCE_DIR/$(detect_kaku_setup_script)"
TOOLS_SCRIPT="$RESOURCE_DIR/install_cli_tools.sh"

user_version="$(read_config_version)"

if [[ $user_version -eq 0 && ! -f "$STATE_FILE" ]]; then
	if [[ -f "$LEGACY_VERSION_FILE" || -f "$LEGACY_GEOMETRY_FILE" ]]; then
		legacy_version=0
		if [[ -f "$LEGACY_VERSION_FILE" ]]; then
			candidate="$(tr -d '[:space:]' < "$LEGACY_VERSION_FILE" || true)"
			if [[ "$candidate" =~ ^[0-9]+$ ]]; then
				legacy_version="$candidate"
			fi
		fi

		if [[ $legacy_version -eq 0 ]]; then
			legacy_version="$CURRENT_CONFIG_VERSION"
		fi

		persist_config_version "$legacy_version"
		user_version="$legacy_version"
	fi
fi

# Corrupted state file fallback: repair and continue with safe defaults.
if [[ -f "$STATE_FILE" && $user_version -eq 0 ]]; then
	persist_config_version
	user_version="$CURRENT_CONFIG_VERSION"
fi

# Skip if already up to date or new user
if [[ $user_version -eq 0 || $user_version -ge $CURRENT_CONFIG_VERSION ]]; then
	exit 0
fi

echo -e "${BOLD}Kaku shell integration update${NC}  ${YELLOW}v$user_version${NC} → ${GREEN}v$CURRENT_CONFIG_VERSION${NC}"
echo ""

echo -e "${BOLD}What's new:${NC}"
if ! print_config_update_highlights "$SCRIPT_DIR" "$user_version" "$CURRENT_CONFIG_VERSION"; then
	echo "  • Shell integration and reliability improvements"
	echo "  • See project release notes for full details"
fi
echo ""

read -p "Apply now? Press Enter to continue, or type n to skip: " -n 1 -r
echo

if [[ $REPLY =~ ^[Nn]$ ]]; then
	persist_config_version
	echo -e "${YELLOW}Skipped${NC}"
	echo ""
	echo "Press any key to continue..."
	read -n 1 -s
	exit 0
fi

# Apply updates
if [[ -f "$SETUP_SCRIPT" ]]; then
	if ! KAKU_SKIP_TOOL_BOOTSTRAP=1 bash "$SETUP_SCRIPT" --update-only; then
		echo ""
		echo -e "${YELLOW}Update failed. Run 'kaku init' manually to retry.${NC}"
		echo ""
		echo "Press any key to continue..."
		read -n 1 -s
		exit 1
	fi
else
	echo -e "${YELLOW}Error: missing setup script at $SETUP_SCRIPT${NC}"
	exit 1
fi

if [[ -f "$TOOLS_SCRIPT" ]]; then
	if ! KAKU_AUTO_INSTALL_TOOLS=1 bash "$TOOLS_SCRIPT"; then
		echo ""
		echo -e "${YELLOW}Optional tool installation failed.${NC}"
	fi
fi

persist_config_version

echo ""
echo -e "\033[1;32m🎃 Kaku environment is ready! Enjoy coding.\033[0m"
echo ""
echo "Press any key to continue..."
read -n 1 -s

# Replace current process with the user's login shell
TARGET_SHELL="$(detect_login_shell)"
exec "$TARGET_SHELL" -l
