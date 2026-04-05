#!/bin/bash
# Kaku CLI tools bootstrap
# Installs required external tools via Homebrew and migrates legacy bundled binaries.

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

case "${KAKU_TARGET_SHELL:-${SHELL:-/bin/zsh}}" in
*fish|fish)
	USER_BIN_DIR="$HOME/.config/kaku/fish/bin"
	RELOAD_CMD="exec fish"
	;;
*)
	USER_BIN_DIR="$HOME/.config/kaku/zsh/bin"
	RELOAD_CMD="exec zsh"
	;;
esac
MISSING_TOOLS=()
LEGACY_MIGRATED=0
BREW_BIN=""

resolve_brew_bin() {
	if command -v brew >/dev/null 2>&1; then
		command -v brew
		return 0
	fi

	local candidates=(/opt/homebrew/bin/brew /usr/local/bin/brew)

	# Support non-standard Homebrew prefix via HOMEBREW_PREFIX env var
	if [[ -n "${HOMEBREW_PREFIX:-}" ]]; then
		candidates=("$HOMEBREW_PREFIX/bin/brew" "${candidates[@]}")
	fi

	# Common user-installed locations (e.g. ~/homebrew or ~/.homebrew)
	for user_dir in "$HOME/homebrew" "$HOME/.homebrew"; do
		candidates+=("$user_dir/bin/brew")
	done

	for candidate in "${candidates[@]}"; do
		if [[ -x "$candidate" ]]; then
			echo "$candidate"
			return 0
		fi
	done

	return 1
}

prompt_install_homebrew() {
	if [[ ! -t 0 || ! -t 1 ]]; then
		return 1
	fi

	echo ""
	echo "Homebrew is required to install Kaku's optional CLI tools:"
	if [[ ${#MISSING_TOOLS[@]} -gt 0 ]]; then
		echo "  ${MISSING_TOOLS[*]}"
	fi
	read -p "Install Homebrew now? Press Enter to continue, type n to skip: " -n 1 -r
	echo

	if [[ $REPLY =~ ^[Nn]$ ]]; then
		return 1
	fi

	return 0
}

install_homebrew() {
	if ! command -v curl >/dev/null 2>&1; then
		echo -e "${YELLOW}curl not found. Cannot install Homebrew automatically.${NC}"
		return 1
	fi

	echo -e "${BOLD}Installing Homebrew...${NC}"
	if ! /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"; then
		echo -e "${YELLOW}Homebrew installation failed.${NC}"
		return 1
	fi

	if BREW_BIN="$(resolve_brew_bin)"; then
		echo -e "  ${GREEN}✓${NC} ${BOLD}Homebrew${NC}    Installed successfully"
		return 0
	fi

	echo -e "${YELLOW}Homebrew installation finished, but brew was not found in PATH or standard locations.${NC}"
	return 1
}

ensure_homebrew_installation() {
	if BREW_BIN="$(resolve_brew_bin)"; then
		return 0
	fi

	echo -e "${YELLOW}Homebrew not found.${NC}"
	if prompt_install_homebrew && install_homebrew; then
		return 0
	fi

	echo -e "${YELLOW}Skipping optional CLI tools.${NC}"
	if [[ ${#MISSING_TOOLS[@]} -gt 0 ]]; then
		echo "Missing optional tools: ${MISSING_TOOLS[*]}"
	fi
	echo "Kaku shell integration will continue without them."
	echo "Install Homebrew later, then refresh optional tools with:"
	echo "  kaku init --update-only"
	echo "Homebrew install guide:"
	echo "  https://brew.sh"
	return 1
}

resolved_tool_path() {
	local tool_name="$1"
	if command -v "$tool_name" >/dev/null 2>&1; then
		command -v "$tool_name"
		return 0
	fi

	local candidates=("/opt/homebrew/bin/$tool_name" "/usr/local/bin/$tool_name" "/opt/local/bin/$tool_name")

	# If we already have a brew binary, derive the prefix for tool lookup
	if [[ -n "${BREW_BIN:-}" && -x "$BREW_BIN" ]]; then
		local brew_prefix
		brew_prefix="$("$BREW_BIN" --prefix 2>/dev/null || true)"
		if [[ -n "$brew_prefix" ]]; then
			candidates=("$brew_prefix/bin/$tool_name" "${candidates[@]}")
		fi
	fi

	for candidate in "${candidates[@]}"; do
		if [[ -x "$candidate" ]]; then
			echo "$candidate"
			return 0
		fi
	done

	return 1
}

is_legacy_tool_active() {
	local tool_name="$1"
	local resolved
	resolved="$(resolved_tool_path "$tool_name" 2>/dev/null || true)"
	[[ "$resolved" == "$USER_BIN_DIR/$tool_name" ]]
}

is_brew_formula_installed() {
	local formula_name="$1"

	if [[ -z "$BREW_BIN" ]]; then
		BREW_BIN="$(resolve_brew_bin 2>/dev/null || true)"
	fi

	if [[ -z "$BREW_BIN" ]]; then
		return 1
	fi

	"$BREW_BIN" list --formula --versions "$formula_name" >/dev/null 2>&1
}

should_install_formula() {
	local tool_name="$1"
	local formula_name="$2"
	local allow_legacy="${3:-0}"

	if resolved_tool_path "$tool_name" >/dev/null 2>&1; then
		if [[ "$allow_legacy" == "1" ]] && is_legacy_tool_active "$tool_name"; then
			return 0
		fi
		return 1
	fi

	# PATH can be minimal in non-interactive launch contexts.
	# If brew already has this formula installed, skip reinstall.
	if is_brew_formula_installed "$formula_name"; then
		return 1
	fi

	return 0
}

collect_missing_tools() {
	MISSING_TOOLS=()

	if should_install_formula "starship" "starship" 1; then
		MISSING_TOOLS+=("starship")
	fi
	if should_install_formula "delta" "git-delta" 1; then
		MISSING_TOOLS+=("git-delta")
	fi
	if should_install_formula "lazygit" "lazygit" 0; then
		MISSING_TOOLS+=("lazygit")
	fi
	if should_install_formula "yazi" "yazi" 0; then
		MISSING_TOOLS+=("yazi")
	fi
	# zoxide is only used by fish shell integration; zsh uses zsh-z instead
	case "${KAKU_TARGET_SHELL:-${SHELL:-/bin/zsh}}" in
	*fish|fish)
		if should_install_formula "zoxide" "zoxide" 0; then
			MISSING_TOOLS+=("zoxide")
		fi
		;;
	esac
}

migrate_legacy_binary_if_shadowed() {
	local binary_name="$1"
	local legacy_bin="$USER_BIN_DIR/$binary_name"

	if [[ ! -f "$legacy_bin" ]]; then
		return
	fi

	if ! ensure_homebrew_installation; then
		return
	fi

	local brew_prefix
	brew_prefix="$("$BREW_BIN" --prefix 2>/dev/null || true)"
	if [[ -z "$brew_prefix" ]]; then
		return
	fi

	local brew_bin="$brew_prefix/bin/$binary_name"
	if [[ ! -x "$brew_bin" ]]; then
		return
	fi

	rm -f "$legacy_bin"
	LEGACY_MIGRATED=1
	echo -e "  ${GREEN}✓${NC} ${BOLD}Migrate${NC}     Removed legacy $binary_name binary from $legacy_bin"
}

install_missing_tools() {
	collect_missing_tools

	if [[ ${#MISSING_TOOLS[@]} -eq 0 ]]; then
		return 0
	fi

	if ! ensure_homebrew_installation; then
		return 0
	fi

	if [[ -t 0 && -t 1 ]]; then
		echo ""
		if [[ "${KAKU_AUTO_INSTALL_TOOLS:-0}" == "1" ]]; then
			echo -e "${BOLD}Installing optional CLI tools automatically...${NC}"
		else
			echo -e "${BOLD}Optional CLI tools${NC}"
			echo "Kaku can install missing tools with Homebrew:"
			for tool in "${MISSING_TOOLS[@]}"; do
				echo "  - $tool"
			done
			read -p "Install missing tools now? Press Enter to continue, type n to skip: " -n 1 -r
			echo
			if [[ $REPLY =~ ^[Nn]$ ]]; then
				return 0
			fi
		fi
	else
		echo -e "${YELLOW}Non-interactive shell detected; skipped optional tool installation.${NC}"
		return 0
	fi

	echo "Installing: ${MISSING_TOOLS[*]}"
	if ! "$BREW_BIN" install "${MISSING_TOOLS[@]}"; then
		echo -e "${YELLOW}Tool installation failed. You can retry manually:${NC}"
		echo "  brew install ${MISSING_TOOLS[*]}"
		return 0
	fi

	echo -e "  ${GREEN}✓${NC} ${BOLD}Tools${NC}       Installed missing CLI tools via Homebrew"
	return 0
}

install_missing_tools
migrate_legacy_binary_if_shadowed "starship"
migrate_legacy_binary_if_shadowed "delta"

if [[ "$LEGACY_MIGRATED" == "1" ]]; then
	echo ""
	echo -e "${YELLOW}One-time action:${NC} run ${BOLD}${RELOAD_CMD}${NC} to reload prompt hooks after migration."
fi
