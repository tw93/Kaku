#!/bin/bash
# Kaku First Run Experience
# This script is launched automatically on the first run of Kaku.

set -euo pipefail

CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/kaku"
STATE_FILE="$CONFIG_DIR/state.json"
LEGACY_VERSION_FILE="$CONFIG_DIR/.kaku_config_version"
LEGACY_GEOMETRY_FILE="$CONFIG_DIR/.kaku_window_geometry"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMON_SCRIPT="$SCRIPT_DIR/state_common.sh"

if [[ ! -f "$COMMON_SCRIPT" ]]; then
	echo "Error: missing shared state script: $COMMON_SCRIPT"
	exit 1
fi
# shellcheck source=state_common.sh
source "$COMMON_SCRIPT"

CURRENT_CONFIG_VERSION="$(read_bundled_config_version "$SCRIPT_DIR")"

# Resources directory resolution
if [[ -f "$SCRIPT_DIR/setup_zsh.sh" ]]; then
	RESOURCES_DIR="$SCRIPT_DIR"
elif [[ -f "/Applications/Kaku.app/Contents/Resources/setup_zsh.sh" ]]; then
	RESOURCES_DIR="/Applications/Kaku.app/Contents/Resources"
elif [[ -f "$HOME/Applications/Kaku.app/Contents/Resources/setup_zsh.sh" ]]; then
	RESOURCES_DIR="$HOME/Applications/Kaku.app/Contents/Resources"
else
	# Fallback for dev environment
	RESOURCES_DIR="$SCRIPT_DIR"
fi

# Route to the correct setup script based on the login shell
_login_shell="$(basename "${SHELL:-/bin/zsh}")"
if [[ "$_login_shell" == "fish" ]]; then
	SETUP_SCRIPT="$RESOURCES_DIR/setup_fish.sh"
else
	SETUP_SCRIPT="$RESOURCES_DIR/setup_zsh.sh"
fi
TOOLS_SCRIPT="$RESOURCES_DIR/install_cli_tools.sh"

resolve_kaku_cli() {
	local candidates=(
		"$RESOURCES_DIR/../MacOS/kaku"
		"/Applications/Kaku.app/Contents/MacOS/kaku"
		"$HOME/Applications/Kaku.app/Contents/MacOS/kaku"
	)

	local candidate
	for candidate in "${candidates[@]}"; do
		if [[ -x "$candidate" ]]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done

	if command -v kaku >/dev/null 2>&1; then
		command -v kaku
		return 0
	fi

	return 1
}

# Clear screen
clear

# Display Welcome Message
echo -e "\033[1;35m"
echo "  _  __      _          "
echo " | |/ /     | |         "
echo " | ' / __ _ | | __ _   _ "
echo " |  < / _\` || |/ /| | | |"
echo " | . \ (_| ||   < | |_| |"
echo " |_|\_\__,_||_|\_\ \__,_|"
echo -e "\033[0m"
echo "Welcome to Kaku!"
echo "A fast, out-of-the-box terminal built for AI coding."
echo "--------------------------------------------------------"
echo "Kaku's enhanced shell features include:"
if [[ "$_login_shell" == "fish" ]]; then
echo "  - Starship prompt (if installed)"
echo "  - Zoxide integration (if installed)"
echo "  - OSC 7/133/1337 sequences for AI fix hooks"
echo "  - Kaku Yazi theme sync"
echo "  - Optional CLI tools via Homebrew: Starship, Delta, Lazygit, Yazi"
echo ""
echo "Shell config model (fish):"
echo "  - Kaku writes managed config to ~/.config/kaku/fish/kaku.fish"
echo "  - ~/.config/fish/conf.d/kaku.fish sources it automatically"
else
echo "  - z - Smart Directory Jumper"
echo "  - zsh-completions - Rich Tab Completions"
echo "  - Zsh Syntax Highlighting"
echo "  - Zsh Autosuggestions"
echo "  - Kaku Theme"
echo "  - Optional CLI tools via Homebrew: Starship, Delta, Lazygit, Yazi"
echo "  - If Homebrew is missing, Kaku can offer to install it"
echo ""
echo "Shell config model (zsh):"
echo "  - Kaku writes managed shell config to ~/.config/kaku/zsh/kaku.zsh"
echo "  - .zshrc gets one PATH line plus one source line for the managed Kaku shell config"
fi
echo "  - You can roll back anytime with: kaku reset"
echo "--------------------------------------------------------"
echo ""

# Interactive Prompt
read -p "Set up Kaku now? Press Enter to continue, type n to skip: " -n 1 -r
echo ""

INSTALL_SHELL=false
if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
	INSTALL_SHELL=true
fi

INSTALL_THEME="$INSTALL_SHELL"
INITIALIZATION_COMPLETE=false

# Process Shell Features
if [[ "$INSTALL_SHELL" == "true" ]]; then
	if kaku_bin="$(resolve_kaku_cli)"; then
		if KAKU_SKIP_TOOL_BOOTSTRAP=1 "$kaku_bin" init; then
			INITIALIZATION_COMPLETE=true
		else
			echo ""
			echo "Warning: shell setup failed. You can retry manually:"
			echo "  KAKU_SKIP_TOOL_BOOTSTRAP=1 \"$kaku_bin\" init"
			if [[ -f "$SETUP_SCRIPT" ]]; then
				echo "Fallback:"
				echo "  KAKU_SKIP_TOOL_BOOTSTRAP=1 bash \"$SETUP_SCRIPT\""
			fi
		fi
	elif [[ -f "$SETUP_SCRIPT" ]]; then
		echo ""
		echo "Warning: Kaku CLI not found during first-run setup. Falling back to $(basename "$SETUP_SCRIPT")."
		if KAKU_SKIP_TOOL_BOOTSTRAP=1 bash "$SETUP_SCRIPT"; then
			INITIALIZATION_COMPLETE=true
		else
			echo ""
			echo "Warning: shell setup failed. You can retry manually:"
			echo "  KAKU_SKIP_TOOL_BOOTSTRAP=1 bash \"$SETUP_SCRIPT\""
		fi
	else
		echo "Error: neither kaku CLI nor $(basename "$SETUP_SCRIPT") was found for shell setup."
	fi
else
	INITIALIZATION_COMPLETE=true
	echo ""
	echo "Skipping shell setup. You can run it manually later:"
	if kaku_bin="$(resolve_kaku_cli)"; then
		echo "  \"$kaku_bin\" init"
	elif [[ -f "$SETUP_SCRIPT" ]]; then
		echo "  bash \"$SETUP_SCRIPT\""
	fi
fi

mkdir -p "$CONFIG_DIR"

ensure_user_config_via_cli() {
	local kaku_lua_dest="$CONFIG_DIR/kaku.lua"
	if [[ -f "$kaku_lua_dest" ]]; then
		echo "Keeping existing user config: $kaku_lua_dest"
		return 0
	fi

	local kaku_bin
	if ! kaku_bin="$(resolve_kaku_cli)"; then
		echo "Warning: kaku CLI not found, skipped config initialization."
		return 0
	fi

	if "$kaku_bin" config --ensure-only >/dev/null 2>&1; then
		echo "Created minimal user config: $kaku_lua_dest"
	else
		echo "Warning: failed to initialize user config via '$kaku_bin config --ensure-only'."
	fi
}

# Process Kaku Theme
if [[ "$INSTALL_THEME" == "true" ]]; then
	ensure_user_config_via_cli
fi

# Process optional CLI tool installation (single prompt)
if [[ "$INSTALL_SHELL" == "true" ]]; then
	if [[ -f "$TOOLS_SCRIPT" ]]; then
		echo ""
		if ! KAKU_AUTO_INSTALL_TOOLS=1 bash "$TOOLS_SCRIPT"; then
			echo "Warning: optional tool installation failed."
		fi
	else
		echo "Warning: install_cli_tools.sh not found at $TOOLS_SCRIPT"
	fi
fi

if [[ "$INITIALIZATION_COMPLETE" == "true" ]]; then
	echo -e "\n\033[1;32m🎃 Kaku environment is ready! Enjoy coding.\033[0m"
	# Persist only after the selected core setup path succeeds (or the user
	# explicitly skips it). Failed setup remains retryable on the next launch.
	persist_config_version
else
	echo -e "\n\033[1;31mKaku setup is incomplete. It will be offered again next launch.\033[0m"
fi

# Replace current process with the user's login shell
TARGET_SHELL="$(detect_login_shell)"
exec "$TARGET_SHELL" -l
