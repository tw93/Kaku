#!/bin/bash
# Kaku Zsh Setup Script
# This script configures a "batteries-included" Zsh environment using Kaku's bundled resources.
# It is designed to be safe: it backs up existing configurations and can be re-run.

set -euo pipefail

UPDATE_ONLY=false
for arg in "$@"; do
	case "$arg" in
	--update-only)
		UPDATE_ONLY=true
		;;
	esac
done

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Thin entrypoint: delegate to `kaku init` whenever possible.
# The rust command owns wrapper installation and orchestration.
if [[ "${KAKU_INIT_INTERNAL:-0}" != "1" ]]; then
	if [[ -n "${KAKU_BIN:-}" && -x "${KAKU_BIN}" ]]; then
		exec "${KAKU_BIN}" init "$@"
	fi

	for candidate in \
		"$SCRIPT_DIR/../MacOS/kaku" \
		"/Applications/Kaku.app/Contents/MacOS/kaku" \
		"$HOME/Applications/Kaku.app/Contents/MacOS/kaku"; do
		if [[ -x "$candidate" ]]; then
			exec "$candidate" init "$@"
		fi
	done
fi

# Resolve resources by script location first so setup works regardless of app install path.
# - App bundle:   setup_zsh.sh in Resources/, vendor in Resources/vendor
# - Dev checkout: setup_zsh.sh in assets/shell-integration/, vendor in assets/vendor
if [[ -d "$SCRIPT_DIR/vendor" ]]; then
	RESOURCES_DIR="$SCRIPT_DIR"
elif [[ -d "$SCRIPT_DIR/../vendor" ]]; then
	RESOURCES_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
elif [[ -d "/Applications/Kaku.app/Contents/Resources/vendor" ]]; then
	RESOURCES_DIR="/Applications/Kaku.app/Contents/Resources"
elif [[ -d "$HOME/Applications/Kaku.app/Contents/Resources/vendor" ]]; then
	RESOURCES_DIR="$HOME/Applications/Kaku.app/Contents/Resources"
else
	echo -e "${YELLOW}Error: Could not locate Kaku resources (vendor directory missing).${NC}"
	exit 1
fi

VENDOR_DIR="$RESOURCES_DIR/vendor"
# Allow test override so CI can provide stub plugin dirs without real downloads.
if [[ -n "${KAKU_VENDOR_DIR:-}" && -d "${KAKU_VENDOR_DIR}" ]]; then
	VENDOR_DIR="${KAKU_VENDOR_DIR}"
fi
TOOL_INSTALL_SCRIPT="$SCRIPT_DIR/install_cli_tools.sh"
if [[ ! -f "$TOOL_INSTALL_SCRIPT" ]]; then
	TOOL_INSTALL_SCRIPT="$RESOURCES_DIR/install_cli_tools.sh"
fi
USER_CONFIG_DIR="$HOME/.config/kaku/zsh"
KAKU_INIT_FILE="$USER_CONFIG_DIR/kaku.zsh"
KAKU_TMUX_DIR="$HOME/.config/kaku/tmux"
KAKU_TMUX_FILE="$KAKU_TMUX_DIR/kaku.tmux.conf"
STARSHIP_CONFIG="$HOME/.config/starship.toml"
YAZI_CONFIG_DIR="$HOME/.config/yazi"
YAZI_CONFIG_FILE="$YAZI_CONFIG_DIR/yazi.toml"
YAZI_KEYMAP_FILE="$YAZI_CONFIG_DIR/keymap.toml"
YAZI_THEME_FILE="$YAZI_CONFIG_DIR/theme.toml"
YAZI_FLAVORS_DIR="$YAZI_CONFIG_DIR/flavors"
YAZI_WRAPPER_FILE="$USER_CONFIG_DIR/bin/yazi"
KAKU_YAZI_THEME_MARKER_START="# ===== Kaku Yazi Flavor (managed) ====="
KAKU_YAZI_THEME_MARKER_END="# ===== End Kaku Yazi Flavor (managed) ====="
ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"
TMUXRC="$HOME/.tmux.conf"
BACKUP_SUFFIX=".kaku-backup-$(date +%s)"
ZSHRC_BACKED_UP=0
TMUXRC_BACKED_UP=0

if [[ -d "$SCRIPT_DIR/yazi-flavors" ]]; then
	KAKU_YAZI_FLAVOR_SOURCE_DIR="$SCRIPT_DIR/yazi-flavors"
else
	KAKU_YAZI_FLAVOR_SOURCE_DIR="$RESOURCES_DIR/yazi-flavors"
fi

backup_zshrc_once() {
	if [[ -f "$ZSHRC" ]] && [[ "$ZSHRC_BACKED_UP" -eq 0 ]]; then
		cp "$ZSHRC" "$ZSHRC$BACKUP_SUFFIX"
		ZSHRC_BACKED_UP=1
	fi
}

backup_tmuxrc_once() {
	if [[ -f "$TMUXRC" ]] && [[ "$TMUXRC_BACKED_UP" -eq 0 ]]; then
		cp "$TMUXRC" "$TMUXRC$BACKUP_SUFFIX"
		TMUXRC_BACKED_UP=1
	fi
}

default_kaku_config_path() {
	if [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
		printf '%s\n' "${XDG_CONFIG_HOME}/kaku/kaku.lua"
	else
		printf '%s\n' "${HOME}/.config/kaku/kaku.lua"
	fi
}

active_kaku_config_path() {
	if [[ -n "${KAKU_CONFIG_FILE:-}" ]]; then
		printf '%s\n' "${KAKU_CONFIG_FILE}"
	else
		default_kaku_config_path
	fi
}

system_kaku_flavor() {
	local flavor="kaku-dark"
	if command -v defaults >/dev/null 2>&1; then
		local appearance
		appearance="$(defaults read -g AppleInterfaceStyle 2>/dev/null || true)"
		if [[ "$appearance" != "Dark" ]]; then
			flavor="kaku-light"
		fi
	fi
	printf '%s\n' "$flavor"
}

resolve_kaku_flavor_from_config() {
	local config_file="$1"
	local system_flavor
	system_flavor="$(system_kaku_flavor)"

	if [[ -f "$config_file" ]]; then
		local scheme_line
		scheme_line="$(
			awk '
				/^[[:space:]]*--/ { next }
				/^[[:space:]]*config\.color_scheme[[:space:]]*=/ { print; exit }
			' "$config_file"
		)"
		if [[ -n "$scheme_line" ]]; then
			if [[ "$scheme_line" == *"Kaku Light"* ]]; then
				printf '%s\n' "kaku-light"
				return
			fi
			if [[ "$scheme_line" == *"Kaku Dark"* || "$scheme_line" == *"Kaku Theme"* ]]; then
				printf '%s\n' "kaku-dark"
				return
			fi
			if [[ "$scheme_line" == *"'Auto'"* || "$scheme_line" == *'"Auto"'* ]]; then
				printf '%s\n' "$system_flavor"
				return
			fi
			if [[ "$scheme_line" == *get_appearance* ]]; then
				printf '%s\n' "$system_flavor"
				return
			fi
			printf '%s\n' "kaku-dark"
			return
		fi
	fi

	printf '%s\n' "$system_flavor"
}

current_kaku_yazi_flavor() {
	resolve_kaku_flavor_from_config "$(active_kaku_config_path)"
}

kaku_yazi_theme_block() {
	local flavor="${1:-$(current_kaku_yazi_flavor)}"
	cat <<EOF
$KAKU_YAZI_THEME_MARKER_START
[flavor]
dark = "$flavor"
light = "$flavor"
$KAKU_YAZI_THEME_MARKER_END
EOF
}

is_legacy_kaku_yazi_theme_file() {
	if [[ ! -f "$YAZI_THEME_FILE" ]]; then
		return 1
	fi

	local normalized expected
	if grep -Fq '# Kaku-aligned theme for Yazi 26.x' "$YAZI_THEME_FILE"; then
		return 0
	fi
	normalized="$(sed -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' "$YAZI_THEME_FILE")"
	expected=$'[mgr]\nborder_symbol = "│"\nborder_style = { fg = "#555555" }\n[indicator]\npadding = { open = "", close = "" }'
	[[ "$normalized" == "$expected" ]]
}

sync_kaku_yazi_flavors() {
	if [[ ! -d "$KAKU_YAZI_FLAVOR_SOURCE_DIR" ]]; then
		echo -e "${YELLOW}Warning: bundled Yazi flavors are missing at $KAKU_YAZI_FLAVOR_SOURCE_DIR.${NC}"
		return
	fi

	mkdir -p "$YAZI_FLAVORS_DIR"

	local flavor source_dir target_dir
	for flavor in kaku-dark.yazi kaku-light.yazi; do
		source_dir="$KAKU_YAZI_FLAVOR_SOURCE_DIR/$flavor"
		target_dir="$YAZI_FLAVORS_DIR/$flavor"

		if [[ ! -d "$source_dir" ]]; then
			echo -e "${YELLOW}Warning: missing bundled Yazi flavor $source_dir.${NC}"
			continue
		fi

		if [[ ! -f "$source_dir/flavor.toml" ]]; then
			echo -e "${YELLOW}Warning: flavor.toml missing in $source_dir.${NC}"
			continue
		fi

		mkdir -p "$target_dir"
		cp "$source_dir/flavor.toml" "$target_dir/flavor.toml"
	done

	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Refreshed Kaku yazi flavors ${NC}(dark + light)${NC}"
}

ensure_kaku_yazi_theme() {
	mkdir -p "$YAZI_CONFIG_DIR"
	local managed_flavor
	managed_flavor="$(current_kaku_yazi_flavor)"

	if [[ ! -f "$YAZI_THEME_FILE" ]] || is_legacy_kaku_yazi_theme_file; then
		cat <<EOF >"$YAZI_THEME_FILE"
"\$schema" = "https://yazi-rs.github.io/schemas/theme.json"

# Kaku manages the [flavor] section below so Yazi matches the current Kaku theme.
# Add your own theme overrides in other sections if needed.
$(kaku_yazi_theme_block "$managed_flavor")
EOF
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized yazi theme ${NC}(managed Kaku flavor: $managed_flavor)${NC}"
		return
	fi

	if grep -Eq '^[[:space:]]*\[flavor\][[:space:]]*$' "$YAZI_THEME_FILE" && ! grep -Fq "$KAKU_YAZI_THEME_MARKER_START" "$YAZI_THEME_FILE"; then
		echo -e "  ${BLUE}•${NC} ${BOLD}Config${NC}      Preserved existing yazi [flavor] section ${NC}(user-managed)${NC}"
		return
	fi

	local tmp_theme
	tmp_theme="$(mktemp "${TMPDIR:-/tmp}/kaku-yazi-theme.XXXXXX")"

	awk -v start="$KAKU_YAZI_THEME_MARKER_START" -v end="$KAKU_YAZI_THEME_MARKER_END" '
		index($0, start) { skip = 1; next }
		index($0, end)   { skip = 0; next }
		!skip { print }
	' "$YAZI_THEME_FILE" >"$tmp_theme"

	{
		cat "$tmp_theme"
		printf '\n'
		kaku_yazi_theme_block "$managed_flavor"
		printf '\n'
	} >"${tmp_theme}.next"

	mv "${tmp_theme}.next" "$YAZI_THEME_FILE"
	rm -f "$tmp_theme"
	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Updated yazi theme ${NC}(managed Kaku flavor: $managed_flavor)${NC}"
}

install_yazi_wrapper() {
	cat <<'EOF' >"$YAZI_WRAPPER_FILE"
#!/bin/bash
set -euo pipefail

YAZI_THEME_FILE="${HOME}/.config/yazi/theme.toml"
MARKER_START="# ===== Kaku Yazi Flavor (managed) ====="
MARKER_END="# ===== End Kaku Yazi Flavor (managed) ====="
WRAPPER_PATH="${BASH_SOURCE[0]}"
WRAPPER_DIR="$(cd "$(dirname "$WRAPPER_PATH")" && pwd)"

system_kaku_flavor() {
	local flavor="kaku-dark"
	if command -v defaults >/dev/null 2>&1; then
		local appearance
		appearance="$(defaults read -g AppleInterfaceStyle 2>/dev/null || true)"
		if [[ "$appearance" != "Dark" ]]; then
			flavor="kaku-light"
		fi
	fi
	printf '%s\n' "$flavor"
}

default_kaku_config_path() {
	if [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
		printf '%s\n' "${XDG_CONFIG_HOME}/kaku/kaku.lua"
	else
		printf '%s\n' "${HOME}/.config/kaku/kaku.lua"
	fi
}

active_kaku_config_path() {
	if [[ -n "${KAKU_CONFIG_FILE:-}" ]]; then
		printf '%s\n' "${KAKU_CONFIG_FILE}"
	else
		default_kaku_config_path
	fi
}

resolve_kaku_flavor_from_config() {
	local config_file="$1"
	local system_flavor
	system_flavor="$(system_kaku_flavor)"

	if [[ -f "$config_file" ]]; then
		local scheme_line
		scheme_line="$(
			awk '
				/^[[:space:]]*--/ { next }
				/^[[:space:]]*config\.color_scheme[[:space:]]*=/ { print; exit }
			' "$config_file"
		)"
		if [[ -n "$scheme_line" ]]; then
			if [[ "$scheme_line" == *"Kaku Light"* ]]; then
				printf '%s\n' "kaku-light"
				return
			fi
			if [[ "$scheme_line" == *"Kaku Dark"* || "$scheme_line" == *"Kaku Theme"* ]]; then
				printf '%s\n' "kaku-dark"
				return
			fi
			if [[ "$scheme_line" == *"'Auto'"* || "$scheme_line" == *'"Auto"'* ]]; then
				printf '%s\n' "$system_flavor"
				return
			fi
			if [[ "$scheme_line" == *get_appearance* ]]; then
				printf '%s\n' "$system_flavor"
				return
			fi
			printf '%s\n' "kaku-dark"
			return
		fi
	fi

	printf '%s\n' "$system_flavor"
}

current_flavor() {
	resolve_kaku_flavor_from_config "$(active_kaku_config_path)"
}

managed_block() {
	local flavor="$1"
	cat <<BLOCK
$MARKER_START
[flavor]
dark = "$flavor"
light = "$flavor"
$MARKER_END
BLOCK
}

ensure_theme() {
	local flavor="$1"
	mkdir -p "$(dirname "$YAZI_THEME_FILE")"

	if [[ ! -f "$YAZI_THEME_FILE" ]]; then
		cat <<BLOCK >"$YAZI_THEME_FILE"
"\$schema" = "https://yazi-rs.github.io/schemas/theme.json"

# Kaku manages the [flavor] section below so Yazi matches the current Kaku theme.
$(managed_block "$flavor")
BLOCK
		return
	fi

	if grep -Eq '^[[:space:]]*\[flavor\][[:space:]]*$' "$YAZI_THEME_FILE" && ! grep -Fq "$MARKER_START" "$YAZI_THEME_FILE"; then
		return
	fi

	local tmp_theme
	tmp_theme="$(mktemp "${TMPDIR:-/tmp}/kaku-yazi-wrapper.XXXXXX")"
	awk -v start="$MARKER_START" -v end="$MARKER_END" '
		index($0, start) { skip = 1; next }
		index($0, end)   { skip = 0; next }
		!skip { print }
	' "$YAZI_THEME_FILE" >"$tmp_theme"

	{
		cat "$tmp_theme"
		printf '\n'
		managed_block "$flavor"
		printf '\n'
	} >"${tmp_theme}.next"

	mv "${tmp_theme}.next" "$YAZI_THEME_FILE"
	rm -f "$tmp_theme"
}

resolve_real_yazi() {
	local candidate

	# Check PATH first so any package manager (MacPorts, Nix, etc.) is found
	local path_entry
	IFS=':' read -r -a path_entries <<< "${PATH:-}"
	for path_entry in "${path_entries[@]}"; do
		[[ -z "$path_entry" || "$path_entry" == "$WRAPPER_DIR" ]] && continue
		candidate="$path_entry/yazi"
		if [[ -x "$candidate" && "$candidate" != "$WRAPPER_PATH" ]]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done

	# Fallback to well-known install locations for GUI-launched shells
	# where PATH may be minimal
	for candidate in /opt/homebrew/bin/yazi /usr/local/bin/yazi /opt/local/bin/yazi; do
		if [[ -x "$candidate" && "$candidate" != "$WRAPPER_PATH" ]]; then
			printf '%s\n' "$candidate"
			return 0
		fi
	done

	return 1
}

main() {
	local flavor real_bin
	flavor="$(current_flavor)"
	ensure_theme "$flavor"

	if ! real_bin="$(resolve_real_yazi)"; then
		echo "yazi not found. Install it with: brew install yazi" >&2
		exit 127
	fi

	exec "$real_bin" "$@"
}

main "$@"
EOF
	chmod +x "$YAZI_WRAPPER_FILE"
	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Installed yazi wrapper ${NC}(theme sync before launch)${NC}"
}

# Ensure vendor resources exist
if [[ ! -d "$VENDOR_DIR" ]]; then
	echo -e "${YELLOW}Error: Vendor resources not found in $VENDOR_DIR${NC}"
	exit 1
fi

install_kaku_terminfo() {
	# Skip explicit bootstrap when requested.
	if [[ "${KAKU_SKIP_TERMINFO_BOOTSTRAP:-0}" == "1" ]]; then
		return
	fi

	# If available in system/user databases already, no-op.
	if infocmp kaku >/dev/null 2>&1; then
		return
	fi

	local target_dir="$HOME/.terminfo"
	local compiled_entry="$RESOURCES_DIR/terminfo/6b/kaku"
	local source_entry=""

	# App bundles include compiled terminfo entries under Resources/terminfo.
	if [[ -f "$compiled_entry" ]]; then
		if mkdir -p "$target_dir/6b" 2>/dev/null && cp "$compiled_entry" "$target_dir/6b/kaku" 2>/dev/null; then
			if infocmp kaku >/dev/null 2>&1; then
				echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Installed kaku terminfo ${NC}(~/.terminfo)${NC}"
				return
			fi
		else
			echo -e "${YELLOW}Warning: could not copy compiled terminfo entry to ~/.terminfo, continuing.${NC}"
		fi
	fi

	# Dev checkout fallback: compile from source terminfo definition.
	for candidate in \
		"$RESOURCES_DIR/../termwiz/data/kaku.terminfo" \
		"$SCRIPT_DIR/../../termwiz/data/kaku.terminfo"; do
		if [[ -f "$candidate" ]]; then
			source_entry="$candidate"
			break
		fi
	done

	if [[ -n "$source_entry" ]]; then
		if ! command -v tic >/dev/null 2>&1; then
			echo -e "${YELLOW}Warning: tic not found, skipping kaku terminfo install.${NC}"
			return
		fi

		mkdir -p "$target_dir"
		if tic -x -o "$target_dir" "$source_entry" >/dev/null 2>&1 && infocmp kaku >/dev/null 2>&1; then
			echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Installed kaku terminfo ${NC}(~/.terminfo)${NC}"
			return
		fi
	fi

	echo -e "${YELLOW}Warning: failed to install kaku terminfo automatically.${NC}"
}

install_kaku_terminfo

echo -e "${BOLD}Setting up Kaku Shell Environment${NC}"

# 1. Prepare User Config Directory
mkdir -p "$USER_CONFIG_DIR"
mkdir -p "$USER_CONFIG_DIR/plugins"
mkdir -p "$USER_CONFIG_DIR/bin"

# 2. Optional external tools bootstrap (Homebrew-managed)
if [[ "${KAKU_SKIP_TOOL_BOOTSTRAP:-0}" != "1" ]]; then
	if [[ -f "$TOOL_INSTALL_SCRIPT" ]]; then
		if ! bash "$TOOL_INSTALL_SCRIPT"; then
			echo -e "${YELLOW}Warning: optional CLI tool bootstrap failed.${NC}"
		fi
	else
		echo -e "${YELLOW}Warning: missing tool bootstrap script at $TOOL_INSTALL_SCRIPT${NC}"
	fi
fi

# Validate required plugin directories up front.
# setup_zsh.sh may be run standalone, so provide a clear dependency hint.
for plugin in fast-syntax-highlighting zsh-autosuggestions zsh-completions zsh-z; do
	if [[ ! -d "$VENDOR_DIR/$plugin" ]]; then
		echo -e "${YELLOW}Error: Missing plugin vendor directory: $VENDOR_DIR/$plugin${NC}"
		echo -e "${YELLOW}Hint: Run scripts/download_vendor.sh before setup_zsh.sh.${NC}"
		exit 1
	fi
done

# Remove legacy plugins replaced in this version
if [[ -n "$USER_CONFIG_DIR" ]]; then
	rm -rf "$USER_CONFIG_DIR/plugins/zsh-syntax-highlighting"
fi
# Copy Plugins
cp -R "$VENDOR_DIR/fast-syntax-highlighting" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-autosuggestions" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-completions" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-z" "$USER_CONFIG_DIR/plugins/"
echo -e "  ${GREEN}✓${NC} ${BOLD}Tools${NC}       Installed Zsh plugins ${NC}(~/.config/kaku/zsh/plugins)${NC}"

# Copy Starship Config (if not exists)
if [[ ! -f "$STARSHIP_CONFIG" ]]; then
	if [[ -f "$VENDOR_DIR/starship.toml" ]]; then
		mkdir -p "$(dirname "$STARSHIP_CONFIG")"
		cp "$VENDOR_DIR/starship.toml" "$STARSHIP_CONFIG"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized starship.toml ${NC}(~/.config/starship.toml)${NC}"
	fi
fi

# Initialize Yazi layout config if the user has not created one yet.
if [[ ! -f "$YAZI_CONFIG_FILE" ]]; then
	mkdir -p "$YAZI_CONFIG_DIR"
	cat <<EOF >"$YAZI_CONFIG_FILE"
[mgr]
ratio = [3, 3, 10]

[preview]
max_width = 2000
max_height = 2400

[opener]
edit = [
  { run = "\${EDITOR:-vim} %s", desc = "edit", for = "unix", block = true },
]
EOF
	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized yazi.toml ${NC}(~/.config/yazi/yazi.toml)${NC}"
fi

ensure_yazi_preview_size_defaults() {
	if [[ ! -f "$YAZI_CONFIG_FILE" ]]; then
		return
	fi

	local preview_block
	preview_block="$(awk '
		BEGIN { in_preview = 0 }
		/^[[:space:]]*\[preview\][[:space:]]*$/ { in_preview = 1; next }
		/^[[:space:]]*\[[^]]+\][[:space:]]*$/ { in_preview = 0 }
		in_preview { print }
	' "$YAZI_CONFIG_FILE")"

	local has_preview_section=false
	local has_max_width=false
	local has_max_height=false

	if grep -Eq '^[[:space:]]*\[preview\][[:space:]]*$' "$YAZI_CONFIG_FILE"; then
		has_preview_section=true
	fi
	if grep -Eq '^[[:space:]]*max_width[[:space:]]*=' <<<"$preview_block"; then
		has_max_width=true
	fi
	if grep -Eq '^[[:space:]]*max_height[[:space:]]*=' <<<"$preview_block"; then
		has_max_height=true
	fi

	if [[ "$has_preview_section" == "false" ]]; then
		cat <<EOF >>"$YAZI_CONFIG_FILE"

[preview]
max_width = 2000
max_height = 2400
EOF
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Added default Yazi preview size ${NC}(2000x2400)${NC}"
		return
	fi

	if [[ "$has_max_width" == "true" ]] && [[ "$has_max_height" == "true" ]]; then
		return
	fi

	local tmp_yazi
	tmp_yazi="$(mktemp "${TMPDIR:-/tmp}/kaku-yazi-preview.XXXXXX")"

	awk -v need_width="$has_max_width" -v need_height="$has_max_height" '
		/^[[:space:]]*\[preview\][[:space:]]*$/ {
			print
			if (need_width != "true") {
				print "max_width = 2000"
			}
			if (need_height != "true") {
				print "max_height = 2400"
			}
			next
		}
		{ print }
	' "$YAZI_CONFIG_FILE" >"$tmp_yazi"

	mv "$tmp_yazi" "$YAZI_CONFIG_FILE"
	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Completed Yazi preview size defaults ${NC}(2000x2400)${NC}"
}

ensure_yazi_preview_size_defaults

ensure_yazi_edit_opener() {
	if [[ ! -f "$YAZI_CONFIG_FILE" ]]; then
		return
	fi

	# Only skip if the user already has an edit opener defined.
	if grep -Eq '^[[:space:]]*edit[[:space:]]*=' "$YAZI_CONFIG_FILE"; then
		return
	fi

	# If [opener] section exists but has no edit entry, append edit under it.
	if grep -Eq '^[[:space:]]*\[opener\][[:space:]]*$' "$YAZI_CONFIG_FILE"; then
		local tmp_yazi
		tmp_yazi="$(mktemp "${TMPDIR:-/tmp}/kaku-yazi-edit.XXXXXX")"
		awk '/^[[:space:]]*\[opener\][[:space:]]*$/ {
			print
			print "edit = ["
			print "  { run = \"${EDITOR:-vim} %s\", desc = \"edit\", for = \"unix\", block = true },"
			print "]"
			next
		}
		{ print }' "$YAZI_CONFIG_FILE" >"$tmp_yazi"
		mv "$tmp_yazi" "$YAZI_CONFIG_FILE"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Added default Yazi edit opener under existing [opener] section"
		return
	fi

	cat <<EOF >>"$YAZI_CONFIG_FILE"

[opener]
edit = [
  { run = "\${EDITOR:-vim} %s", desc = "edit", for = "unix", block = true },
]
EOF
	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Added default Yazi edit opener ${NC}(vim)${NC}"
}

ensure_yazi_edit_opener

# Initialize Yazi keymap tweaks if the user has not created one yet.
if [[ ! -f "$YAZI_KEYMAP_FILE" ]]; then
	mkdir -p "$YAZI_CONFIG_DIR"
	cat <<EOF >"$YAZI_KEYMAP_FILE"
"\$schema" = "https://yazi-rs.github.io/schemas/keymap.json"

[mgr]
prepend_keymap = [
  { on = "e", run = "open", desc = "Edit or open selected files" },
  { on = "o", run = "open", desc = "Edit or open selected files" },
  { on = "<Enter>", run = "enter", desc = "Enter the child directory" },
]
EOF
	echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized yazi keymap ${NC}(~/.config/yazi/keymap.toml)${NC}"
fi

sync_kaku_yazi_flavors
ensure_kaku_yazi_theme
install_yazi_wrapper

# 3. Create/Update Kaku Init File (managed by Kaku)
cat <<EOF >"$KAKU_INIT_FILE"
# Kaku Zsh Integration - DO NOT EDIT MANUALLY
# This file is managed by Kaku.app. Any changes may be overwritten.

export KAKU_ZSH_DIR="\$HOME/.config/kaku/zsh"

# Add Kaku managed bin to PATH (kaku wrapper and user tools)
export PATH="\$KAKU_ZSH_DIR/bin:\$PATH"

# Initialize Starship (Cross-shell prompt)
# Use system installation managed by Homebrew (or user PATH).
if command -v starship &> /dev/null; then
    eval "\$(starship init zsh)"

    # Kaku workaround: Fix Zsh + Starship bug where Ctrl-C prints the literal RPROMPT string.
    # When Zsh receives SIGINT during prompt evaluation, it aborts the command
    # substitution and prints the literal \$(starship...) string. Pre-evaluating
    # the right prompt in precmd avoids this entirely.
    _kaku_render_starship_rprompt() {
        command starship prompt --right \
            --terminal-width="\${COLUMNS:-}" \
            --keymap="\${KEYMAP:-}" \
            --status="\${STARSHIP_CMD_STATUS:-}" \
            --pipestatus="\${STARSHIP_PIPE_STATUS[*]:-}" \
            --cmd-duration="\${STARSHIP_DURATION:-}" \
            --jobs="\${STARSHIP_JOBS_COUNT:-0}" 2>/dev/null
    }

    _kaku_fix_starship_rprompt() {
        # Check if RPROMPT currently holds a dynamic starship command
        if [[ "\${RPROMPT:-}" == *'\$('*'starship'*'prompt --right'* ]]; then
            # Capture it and save it as our template
            _kaku_starship_rprompt_cmd="\$RPROMPT"
        fi

        # If we have a saved starship command template, we should evaluate it.
        # BUT we only overwrite RPROMPT if RPROMPT is exactly what we set it to last time,
        # or if it is the original starship command itself.
        # If the user sets RPROMPT="foo", we leave it alone.
        if [[ -n "\${_kaku_starship_rprompt_cmd:-}" ]]; then
            if [[ "\${RPROMPT:-}" == "\${_kaku_starship_rprompt_cmd}" ]] || [[ "\${RPROMPT:-}" == "\${_kaku_last_injected_rprompt:-}" ]]; then
                local evaled
                if [[ "\${_kaku_starship_rprompt_cmd}" == *starship*'prompt --right'* ]]; then
                    evaled="\$(_kaku_render_starship_rprompt)"
                else
                    local cmd="\${_kaku_starship_rprompt_cmd}"
                    # Avoid zsh pattern parsing here; strip a literal \$(
                    # prefix and trailing ) via slicing instead.
                    if [[ "\${cmd[1]}" == '$' && "\${cmd[2]}" == '(' && "\${cmd[-1]}" == ')' ]]; then
                        cmd="\${cmd[3,-2]}"
                    fi
                    evaled="\$(eval "\$cmd" 2>/dev/null)"
                fi
                RPROMPT="\$evaled"
                _kaku_last_injected_rprompt="\$evaled"
            fi
        fi
    }
    if [[ \${precmd_functions[(Ie)_kaku_fix_starship_rprompt]} -eq 0 ]]; then
        precmd_functions+=(_kaku_fix_starship_rprompt)
    fi
fi

# Enable color output for ls
export CLICOLOR=1
export LSCOLORS="gxfxcxdxbxegedabagacad"

# Smart History Configuration
HISTSIZE="\${HISTSIZE:-50000}"
SAVEHIST="\${SAVEHIST:-50000}"
if [[ -z "\${HISTFILE:-}" ]]; then
    HISTFILE="\${ZDOTDIR:-\$HOME}/.zsh_history"
fi
setopt HIST_IGNORE_ALL_DUPS      # Remove older duplicate when new entry is added
setopt HIST_FIND_NO_DUPS         # Do not display duplicates when searching history
setopt HIST_REDUCE_BLANKS        # Remove blank lines from history
setopt HIST_IGNORE_SPACE         # Skip commands that start with a space
setopt SHARE_HISTORY             # Share history between all sessions
setopt APPEND_HISTORY            # Append history to the history file (no overwriting)
setopt INC_APPEND_HISTORY        # Write each command to history file immediately
setopt EXTENDED_HISTORY          # Include timestamps in saved history

# Set default Zsh options
setopt interactive_comments
bindkey -e

# Prefix history search on Up/Down (e.g. type "curl" then press Up)
# This is shell behavior, not terminal behavior, so Kaku configures it here.
autoload -U up-line-or-beginning-search down-line-or-beginning-search
zle -N up-line-or-beginning-search
zle -N down-line-or-beginning-search
zmodload zsh/terminfo 2>/dev/null || true
for _kaku_keymap in emacs viins; do
    [[ -n "\${terminfo[kcuu1]:-}" ]] && bindkey -M "\$_kaku_keymap" "\${terminfo[kcuu1]}" up-line-or-beginning-search
    [[ -n "\${terminfo[kcud1]:-}" ]] && bindkey -M "\$_kaku_keymap" "\${terminfo[kcud1]}" down-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[[A' up-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[[B' down-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[OA' up-line-or-beginning-search
    bindkey -M "\$_kaku_keymap" '^[OB' down-line-or-beginning-search
done
unset _kaku_keymap

# Kaku line-selection widgets for modified arrows in prompt editing.
_kaku_select_left_char() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle backward-char
}
_kaku_select_right_char() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle forward-char
}
_kaku_select_line_start() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle beginning-of-line
}
_kaku_select_line_end() {
    emulate -L zsh
    if (( ! REGION_ACTIVE )); then
        zle set-mark-command
    fi
    zle end-of-line
}
_kaku_has_active_region() {
    emulate -L zsh
    # Require both an active region flag and a non-empty span. Either one can
    # be stale on its own and would cause false-positive kill-region deletes.
    (( REGION_ACTIVE && MARK != CURSOR ))
}
_kaku_deactivate_region() {
    emulate -L zsh
    if ! _kaku_has_active_region; then
        return 1
    fi
    if (( \${+widgets[deactivate-region]} )); then
        zle deactivate-region
    else
        MARK=\$CURSOR
        REGION_ACTIVE=0
        zle redisplay
    fi
    return 0
}
# Unconditional region deactivation helper (not bound to any key; called from
# _kaku_mv_* widgets below). Unlike _kaku_deactivate_region this always clears
# REGION_ACTIVE without checking MARK vs CURSOR, ensuring stale region flags
# are removed even when the selection span is empty.
_kaku_force_deactivate_region() {
    emulate -L zsh
    (( ! REGION_ACTIVE )) && return
    if (( \${+widgets[deactivate-region]} )); then
        zle deactivate-region
    else
        REGION_ACTIVE=0
        MARK=\$CURSOR
        zle redisplay
    fi
}
# Movement widgets that auto-deactivate any active region before moving.
# The Kaku GUI sends ^B/^F/^A/^E when collapsing a selection with a plain or
# Cmd+arrow key; these wrappers ensure zsh clears REGION_ACTIVE in the same
# keystroke, preventing spurious region-extension or stale region highlights.
_kaku_mv_backward_char() {
    emulate -L zsh
    _kaku_force_deactivate_region
    zle backward-char
}
_kaku_mv_forward_char() {
    emulate -L zsh
    _kaku_force_deactivate_region
    zle forward-char
}
_kaku_mv_beginning_of_line() {
    emulate -L zsh
    _kaku_force_deactivate_region
    zle beginning-of-line
}
_kaku_mv_end_of_line() {
    emulate -L zsh
    _kaku_force_deactivate_region
    zle end-of-line
}
zle -N _kaku_mv_backward_char
zle -N _kaku_mv_forward_char
zle -N _kaku_mv_beginning_of_line
zle -N _kaku_mv_end_of_line
zle -N _kaku_select_left_char
zle -N _kaku_select_right_char
zle -N _kaku_select_line_start
zle -N _kaku_select_line_end

# Terminal-assisted selection shortcuts (Kaku GUI sends these directly).
_kaku_cmd_a_select_all() {
    emulate -L zsh
    # Move to beginning first so MARK is anchored there, then extend to end.
    # If set-mark-command were called first, MARK would be at the current cursor
    # position and only the text after it would be selected.
    zle beginning-of-line
    zle set-mark-command
    zle end-of-line
}
_kaku_cmd_shift_left() {
    emulate -L zsh
    zle set-mark-command
    zle beginning-of-line
}
_kaku_cmd_shift_right() {
    emulate -L zsh
    zle set-mark-command
    zle end-of-line
}
zle -N _kaku_cmd_a_select_all
zle -N _kaku_cmd_shift_left
zle -N _kaku_cmd_shift_right

# Cancel selection without moving cursor (ESC key in Kaku GUI).
_kaku_cancel_selection() {
    emulate -L zsh
    _kaku_force_deactivate_region
}
zle -N _kaku_cancel_selection

# Shift+Left/Right: char expand; Shift+Home/End: to line boundary.
bindkey '^[[1;2D' _kaku_select_left_char
bindkey '^[[1;2C' _kaku_select_right_char
bindkey '^[[1;2H' _kaku_select_line_start
bindkey '^[[1;2F' _kaku_select_line_end

# Terminal-assisted selection shortcuts (distinct CSI sequences from Kaku GUI).
bindkey '^[[990~' _kaku_cmd_a_select_all
bindkey '^[[991~' _kaku_cmd_shift_left
bindkey '^[[992~' _kaku_cmd_shift_right
bindkey '^[[995~' _kaku_cancel_selection

# Emacs movement keys wrapped to auto-deactivate any active region.
# ^B/^F/^A/^E are sent by the Kaku GUI when collapsing a selection with a
# plain or Cmd+arrow key. Wrapping them (rather than using a custom CSI escape)
# avoids stray characters if the sequence is received in an unexpected context.
bindkey '^B' _kaku_mv_backward_char
bindkey '^F' _kaku_mv_forward_char
bindkey '^A' _kaku_mv_beginning_of_line
bindkey '^E' _kaku_mv_end_of_line

# Bind delete keys to native zsh widgets. The Kaku GUI handles selection-aware
# deletion directly (sending kill sequences via line_editor_selection), so the
# shell side does not need a wrapper here.
bindkey '^?' backward-delete-char
bindkey '^H' backward-delete-char
bindkey '^[[3~' delete-char
bindkey '^G' send-break

# Directory Navigation Options
setopt auto_cd
setopt auto_pushd
setopt pushd_ignore_dups
setopt pushdminus

# Common Aliases (Intuitive defaults)
alias ll='ls -lhF'   # Detailed list (human-readable sizes, no hidden files)
alias la='ls -lAhF'  # List all (including hidden, except . and ..)
alias l='ls -CF'     # Compact list

# Directory Navigation
alias ...='../..'
alias ....='../../..'
alias .....='../../../..'
alias ......='../../../../..'

alias md='mkdir -p'
alias rd=rmdir

# Grep Colors
alias grep='grep --color=auto'
alias egrep='grep -E --color=auto'
alias fgrep='grep -F --color=auto'

# Common Git Aliases (The Essentials)
alias g='git'
alias ga='git add'
alias gaa='git add --all'
alias gb='git branch'
alias gbd='git branch -d'
alias gc='git commit -v'
alias gcmsg='git commit -m'
alias gco='git checkout'
alias gcb='git checkout -b'
alias gd='git diff'
alias gds='git diff --staged'
alias gf='git fetch'
alias gl='git pull'
alias gp='git push'
alias gst='git status'
alias gss='git status -s'
alias glo='git log --oneline --decorate'
alias glg='git log --stat'
alias glgp='git log --stat -p'

# yazi launcher — cd into the directory yazi is in when you exit.
'y'() {
    emulate -L zsh
    setopt local_options no_sh_word_split

    local yazi_cmd="\$KAKU_ZSH_DIR/bin/yazi"
    if [[ ! -x "\$yazi_cmd" ]]; then
        yazi_cmd="\$(command -v yazi 2>/dev/null || true)"
    fi

    if [[ -z "\$yazi_cmd" ]]; then
        echo "yazi not found. Install it with: brew install yazi"
        return 127
    fi

    local tmp cwd
    tmp="\$(mktemp -t 'yazi-cwd.XXXXXX')"
    "\$yazi_cmd" "\$@" --cwd-file="\$tmp"
    if cwd="\$(command cat -- "\$tmp")" && [[ -n "\$cwd" && "\$cwd" != "\$PWD" ]]; then
        builtin cd -- "\$cwd"
    fi
    rm -f -- "\$tmp"
}

# Load Plugins (Performance Optimized)

# Load zsh-completions into fpath before compinit.
# If the user already added this path, do not duplicate it.
if [[ -d "\$KAKU_ZSH_DIR/plugins/zsh-completions/src" ]] && (( \${fpath[(Ie)\$KAKU_ZSH_DIR/plugins/zsh-completions/src]} == 0 )); then
    fpath=("\$KAKU_ZSH_DIR/plugins/zsh-completions/src" \$fpath)
fi

# Optimized compinit:
# - If completion system is already initialized by user config/plugin manager, skip.
# - Otherwise use cache and only rebuild when needed.
autoload -Uz compinit
if ! (( \${+functions[_main_complete]} )) || ! (( \${+_comps} )); then
    if [[ -n "\${ZDOTDIR:-\$HOME}/.zcompdump"(#qN.mh+24) ]]; then
        # Rebuild completion cache if older than 24 hours
        compinit
    else
        # Load from cache (much faster)
        compinit -C
    fi
fi

# Load zsh-z (smart directory jumping) if not already provided by user config.
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh" ]] && ! (( \${+functions[zshz]} )); then
    # Default to smart case matching so \`z kaku\` prefers \`Kaku\` over lowercase
    # path entries. Users can still override this in their own shell config.
    : "\${ZSHZ_CASE:=smart}"
    export ZSHZ_CASE
    source "\$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh"
fi

# cd + Tab falls back to zsh-z frecency history when filesystem completion
# has no match. Delegate ranking to zshz --complete so behavior stays aligned
# with the plugin (frecency ordering, smart-case, future plugin changes).
if (( \${+functions[zshz]} )); then
    _kaku_cd_history_complete() {
        emulate -L zsh
        setopt extended_glob no_sh_word_split

        _cd
        local ret=\$?
        local nmatches="\${compstate[nmatches]:-0}"
        if (( nmatches > 0 )); then
            return \$ret
        fi

        local token="\${PREFIX:-}"
        [[ -n "\$token" ]] || return \$ret
        [[ "\$token" != -* ]] || return \$ret

        (( \${+functions[zshz]} )) || return \$ret

        local -a matches
        local match
        while IFS= read -r match; do
            [[ -n "\$match" ]] || continue
            matches+=("\$match")
        done < <(zshz --complete -- "\$token" 2>/dev/null)

        (( \${#matches[@]} )) || return \$ret

        compadd -Q -U -X "zsh-z history dirs" -- "\${matches[@]}"
        return 0
    }

    if (( \${+functions[compdef]} )); then
        compdef _kaku_cd_history_complete cd
    fi
fi

# Detect if any autosuggest system is already active (e.g., Kiro CLI, Fig, etc.)
# These systems wrap zle widgets with names containing "autosuggest", which would
# conflict with zsh-autosuggestions and cause FUNCNEST recursion errors.
_kaku_has_autosuggest_system() {
    local w
    for w in \${(k)widgets}; do
        case "\${w:l}" in
            autosuggest-*) continue ;;  # zsh-autosuggestions' own widgets
            *autosuggest*) return 0 ;;  # third-party (Kiro CLI, Fig, etc.)
        esac
    done
    return 1
}

# Load zsh-autosuggestions only if:
# 1. User config has not loaded it yet (_zsh_autosuggest_start not defined)
# 2. No other autosuggest system is active (to avoid widget wrapping conflicts)
if ! (( \${+functions[_zsh_autosuggest_start]} )) && ! _kaku_has_autosuggest_system && [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh" ]]; then
    source "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh"
fi
unset -f _kaku_has_autosuggest_system 2>/dev/null

# Smart Tab behavior:
# - Use completion while typing arguments/path-like tokens
# - Accept inline suggestion first only for the first command token
# - Only claim Tab inside Kaku sessions unless explicitly disabled
if [[ -z "\${KAKU_SMART_TAB_DISABLE:-}" ]] && [[ "\${TERM_PROGRAM:-}" == "Kaku" ]]; then
    _kaku_tab_widget() {
        emulate -L zsh

        local has_suggestion=0
        if (( \${+widgets[autosuggest-accept]} )) && [[ -n "\${POSTDISPLAY:-}" ]]; then
            has_suggestion=1
        fi

        # Use completion while typing arguments (e.g. 'vim READ<Tab>')
        # and for path-like command tokens ('./scr<Tab>').
        local lbuf="\${LBUFFER}"
        local trimmed="\${lbuf#\${lbuf%%[![:space:]]*}}"
        local current_token="\${lbuf##*[[:space:]]}"

        if [[ -z "\$trimmed" || "\$trimmed" == *[[:space:]]* || "\$current_token" == */* ]]; then
            zle expand-or-complete
            return
        fi

        if (( has_suggestion )); then
            zle autosuggest-accept
        else
            zle expand-or-complete
        fi
    }
    zle -N _kaku_tab_widget
    bindkey '^I' _kaku_tab_widget
fi

# Defer fast-syntax-highlighting to first prompt (~40ms saved at startup)
# This plugin must be loaded LAST, and we delay it for faster shell startup.
# If user config already loaded it, skip to avoid overriding user settings.
if ! (( \${+functions[_zsh_highlight]} )) && [[ -f "\$KAKU_ZSH_DIR/plugins/fast-syntax-highlighting/fast-syntax-highlighting.plugin.zsh" ]]; then
    # Defer loading until first prompt display
    fast_syntax_highlighting_defer() {
        source "\$KAKU_ZSH_DIR/plugins/fast-syntax-highlighting/fast-syntax-highlighting.plugin.zsh"

        # Override comment color: fsh default (fg=8) is invisible on dark backgrounds.
        typeset -gA FAST_HIGHLIGHT_STYLES
        FAST_HIGHLIGHT_STYLES[comment]='fg=244'

        # Remove this hook after first run
        precmd_functions=("\${precmd_functions[@]:#fast_syntax_highlighting_defer}")
    }

    # Hook into precmd (runs before prompt is displayed)
    precmd_functions+=(fast_syntax_highlighting_defer)
fi

# Kaku AI fix hooks (error-only):
# - preexec captures the command text
# - precmd captures the previous command exit code
# Lua listens to these user vars and only suggests fixes when exit code != 0.
_kaku_set_user_var() {
    local name="\$1"
    local value="\$2"

    # Kaku defaults TERM to xterm-256color for SSH compatibility.
    # Use WEZTERM_PANE presence to detect Kaku/WezTerm panes reliably.
    if [[ "\$TERM" != "kaku" && -z "\${WEZTERM_PANE:-}" ]]; then
        return
    fi

    if [[ "\${WEZTERM_SHELL_SKIP_USER_VARS:-}" == "1" ]]; then
        return
    fi

    local encoded=""
    if command -v base64 >/dev/null 2>&1; then
        encoded="\$(printf '%s' "\$value" | base64 | tr -d '\r\n')"
    else
        return
    fi

    if [[ -n "\${TMUX:-}" ]]; then
        printf "\033Ptmux;\033\033]1337;SetUserVar=%s=%s\007\033\\\\" "\$name" "\$encoded"
    else
        printf "\033]1337;SetUserVar=%s=%s\007" "\$name" "\$encoded"
    fi
}

# Only emit exit code when a real command was executed.
# Empty Enter should not re-trigger AI suggestions for the previous failure.
typeset -g _kaku_ai_cmd_pending=0

_kaku_ai_preexec() {
    if [[ -n "\${KAKU_AUTO_DISABLE:-}" ]]; then
        return
    fi
    _kaku_ai_cmd_pending=1
    _kaku_set_user_var "kaku_last_cmd" "\$1"
}

_kaku_ai_precmd() {
    local last_exit_code="\$?"
    if [[ -n "\${KAKU_AUTO_DISABLE:-}" ]]; then
        _kaku_ai_cmd_pending=0
        return 0
    fi
    if [[ "\${_kaku_ai_cmd_pending:-0}" != "1" ]]; then
        return 0
    fi
    _kaku_set_user_var "kaku_last_exit_code" "\$last_exit_code"
    _kaku_ai_cmd_pending=0
}

if [[ \${preexec_functions[(Ie)_kaku_ai_preexec]} -eq 0 ]]; then
    preexec_functions+=(_kaku_ai_preexec)
fi
if [[ \${precmd_functions[(Ie)_kaku_ai_precmd]} -eq 0 ]]; then
    precmd_functions=(_kaku_ai_precmd "\${precmd_functions[@]}")
fi

# Cancel AI suggestions when user starts typing (before pressing Enter).
# This prevents AI notices from appearing after the user has already begun
# entering a new command, avoiding interruption.
typeset -g _kaku_ai_cancel_sent=0

_kaku_cancel_ai_on_typing() {
    if [[ "\$_kaku_ai_cancel_sent" == "0" && -n "\$BUFFER" ]]; then
        _kaku_set_user_var "kaku_user_typing" "1"
        _kaku_ai_cancel_sent=1
    fi
}

_kaku_reset_ai_cancel_flag() {
    _kaku_ai_cancel_sent=0
}

autoload -Uz add-zle-hook-widget 2>/dev/null
if (( \$+functions[add-zle-hook-widget] )); then
    add-zle-hook-widget line-pre-redraw _kaku_cancel_ai_on_typing
    add-zle-hook-widget line-init _kaku_reset_ai_cancel_flag
fi

# AI generate: intercept Enter on "# query" lines via accept-line widget.
# preexec does not fire for comment-only lines (zsh strips them before execution),
# so we wrap accept-line instead. Registration is deferred to first prompt so it
# runs after zsh-autosuggestions finishes binding its own widgets.
_kaku_ai_waiting=0
_kaku_ai_waiting_ts=0
_kaku_ai_reset_waiting() { _kaku_ai_waiting=0; }
add-zsh-hook precmd _kaku_ai_reset_waiting

_kaku_ai_query_accept_line() {
    # Block repeat Enter only while buffer still shows the # query.
    # Auto-reset after 30 seconds to prevent permanent blocking if Lua side fails.
    if (( _kaku_ai_waiting )); then
        if [[ "\${BUFFER[1]}" == '#' ]]; then
            local now=\$EPOCHSECONDS
            if (( now - _kaku_ai_waiting_ts > 30 )); then
                _kaku_ai_waiting=0
            else
                return
            fi
        else
            _kaku_ai_waiting=0
        fi
    fi
    # Only intercept a single-line comment (no newlines in buffer)
    if [[ -z "\${KAKU_AUTO_DISABLE:-}" && -n "\$BUFFER" && "\${BUFFER[1]}" == '#' && "\$BUFFER" != *\$'\\n'* ]]; then
        local query="\${BUFFER:1}"
        query="\${query# }"
        if [[ -n "\$query" ]]; then
            print -s -- "\${BUFFER}"
            _kaku_set_user_var "kaku_ai_query" "\$query"
            _kaku_ai_waiting=1
            _kaku_ai_waiting_ts=\$EPOCHSECONDS
            # Keep # query visible; Lua sends \x15 to clear it when result arrives
            zle reset-prompt
            return
        fi
    fi
    zle .accept-line
}

_kaku_ai_query_register_widget() {
    zle -N accept-line _kaku_ai_query_accept_line
    precmd_functions=("\${precmd_functions[@]:#_kaku_ai_query_register_widget}")
}
precmd_functions+=(_kaku_ai_query_register_widget)

# Auto-set TERM to xterm-256color for SSH connections when running under kaku,
# since remote hosts typically lack the kaku terminfo entry.
# Also auto-detect 1Password SSH agent and add IdentitiesOnly=yes to prevent
# "Too many authentication failures" caused by 1Password offering all stored keys.
# Set KAKU_SSH_SKIP_1PASSWORD_FIX=1 to disable the 1Password behavior.
# Guard: only define if no existing ssh function is present, so user-defined
# wrappers (e.g. from fzf-ssh, autossh plugins) are not silently replaced.
_kaku_wrapped_ssh() {
    local -a extra_opts=()

    # 1Password SSH agent fix: auto-add IdentitiesOnly=yes to prevent
    # "Too many authentication failures" when 1Password offers all stored keys.
    # Set KAKU_SSH_SKIP_1PASSWORD_FIX=1 to disable.
    if [[ -z "\${KAKU_SSH_SKIP_1PASSWORD_FIX-}" ]]; then
        local sock="\${SSH_AUTH_SOCK:-}"
        if [[ "\$sock" == *1password* || "\$sock" == *2BUA8C4S2C* ]]; then
            local has_identitiesonly=false prev=""
            for arg in "\$@"; do
                [[ "\$prev" == "-o" && "\$arg" == IdentitiesOnly=* ]] && has_identitiesonly=true
                [[ "\$arg" == -oIdentitiesOnly=* ]] && has_identitiesonly=true
                prev="\$arg"
            done
            \$has_identitiesonly || extra_opts+=(-o "IdentitiesOnly=yes")
        fi
    fi

    if [[ "\$TERM" == "kaku" ]]; then
        TERM=xterm-256color command ssh "\${extra_opts[@]}" "\$@"
    else
        command ssh "\${extra_opts[@]}" "\$@"
    fi
}
if (( \$+aliases[ssh] )); then
    typeset _kaku_existing_ssh_alias="\${aliases[ssh]}"
    function ssh {
        local -a extra_opts=()
        local -a _kaku_alias_words

        if [[ -z "\${KAKU_SSH_SKIP_1PASSWORD_FIX-}" ]]; then
            local sock="\${SSH_AUTH_SOCK:-}"
            if [[ "\$sock" == *1password* || "\$sock" == *2BUA8C4S2C* ]]; then
                local has_identitiesonly=false prev=""
                for arg in "\$@"; do
                    [[ "\$prev" == "-o" && "\$arg" == IdentitiesOnly=* ]] && has_identitiesonly=true
                    [[ "\$arg" == -oIdentitiesOnly=* ]] && has_identitiesonly=true
                    prev="\$arg"
                done
                \$has_identitiesonly || extra_opts+=(-o "IdentitiesOnly=yes")
            fi
        fi

        _kaku_alias_words=(\${(z)_kaku_existing_ssh_alias})
        if [[ "\${_kaku_alias_words[1]-}" == "ssh" ]]; then
            _kaku_wrapped_ssh "\${(@)_kaku_alias_words[2,-1]}" "\$@"
        elif [[ "\${_kaku_alias_words[1]-}" == "command" && "\${_kaku_alias_words[2]-}" == "ssh" ]]; then
            _kaku_wrapped_ssh "\${(@)_kaku_alias_words[3,-1]}" "\$@"
        elif [[ "\$TERM" == "kaku" ]]; then
            TERM=xterm-256color "\${_kaku_alias_words[@]}" "\${extra_opts[@]}" "\$@"
        else
            "\${_kaku_alias_words[@]}" "\${extra_opts[@]}" "\$@"
        fi
    }
    unalias ssh
elif ! typeset -f ssh > /dev/null 2>&1; then
function ssh {
    _kaku_wrapped_ssh "\$@"
}
fi

# Auto-set TERM to xterm-256color for sudo commands when running under kaku.
# sudo usually resets TERMINFO_DIRS, so root processes (e.g. nano) can fail
# with "unknown terminal type 'kaku'" even though Kaku set TERMINFO_DIRS for the
# user shell. Set KAKU_SUDO_SKIP_TERM_FIX=1 to disable this behavior.
# Guard: only define if no existing sudo function is present.
# If sudo is an alias, zsh expands it during function-definition parsing and
# raises a syntax error ("defining function based on alias"). Unalias first.
if ! typeset -f sudo > /dev/null 2>&1; then
unalias sudo 2>/dev/null || true
function sudo {
    if [[ -z "\${KAKU_SUDO_SKIP_TERM_FIX-}" && "\$TERM" == "kaku" ]]; then
        TERM=xterm-256color command sudo "\$@"
    else
        command sudo "\$@"
    fi
}
fi
EOF

echo -e "  ${GREEN}✓${NC} ${BOLD}Script${NC}      Generated kaku.zsh init script"

# 4. Configure tmux (Optional)
TMUX_SOURCE_LINE='source-file "$HOME/.config/kaku/tmux/kaku.tmux.conf" # Kaku tmux Integration'

write_kaku_tmux_file() {
	mkdir -p "$KAKU_TMUX_DIR"
	cat <<'EOF' >"$KAKU_TMUX_FILE"
# Kaku tmux Integration - DO NOT EDIT MANUALLY
# This file is managed by Kaku.app. Any changes may be overwritten.

set -g mouse on
bind-key -n S-WheelUpPane if-shell -F '#{pane_in_mode}' 'send-keys -X -N 5 scroll-up' 'copy-mode -e -u'
bind-key -n S-WheelDownPane if-shell -F '#{pane_in_mode}' 'send-keys -X -N 5 scroll-down' ''
EOF
	echo -e "  ${GREEN}✓${NC} ${BOLD}Script${NC}      Generated managed tmux integration"
}

normalize_kaku_tmux_source_line() {
	if [[ ! -f "$TMUXRC" ]]; then
		return
	fi

	local tmp_file
	tmp_file="$(mktemp "${TMPDIR:-/tmp}/kaku-tmuxrc.XXXXXX")"

	if awk -v source_line="$TMUX_SOURCE_LINE" '
BEGIN { replaced = 0; extra = 0 }
{
	if ($0 ~ /^[[:space:]]*#/ ) {
		print
		next
	}

	if ($0 ~ /^[[:space:]]*source-file[[:space:]]+/ &&
	    $0 ~ /kaku\/tmux\/kaku\.tmux\.conf/) {
		if (!replaced) {
			print source_line
			replaced = 1
		} else {
			extra++
		}
		next
	}

	print
}
END {
	if (replaced && extra > 0) {
		exit 2
	} else if (replaced) {
		exit 0
	}
	exit 3
}
' "$TMUXRC" >"$tmp_file"; then
		if ! cmp -s "$TMUXRC" "$tmp_file"; then
			backup_tmuxrc_once
			mv "$tmp_file" "$TMUXRC"
			echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Updated Kaku source line in .tmux.conf"
		else
			rm -f "$tmp_file"
		fi
	else
		local awk_status="$?"
		if [[ "$awk_status" == "2" ]]; then
			if ! cmp -s "$TMUXRC" "$tmp_file"; then
				backup_tmuxrc_once
				mv "$tmp_file" "$TMUXRC"
				echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Removed duplicate Kaku source line(s) from .tmux.conf"
			else
				rm -f "$tmp_file"
			fi
		else
			rm -f "$tmp_file"
			if [[ "$awk_status" != "3" ]]; then
				echo -e "${YELLOW}Warning: failed to normalize Kaku source line in .tmux.conf; leaving it unchanged.${NC}"
			fi
		fi
	fi
}

has_kaku_tmux_source_line() {
	if [[ ! -f "$TMUXRC" ]]; then
		return 1
	fi

	if grep -Fqx "$TMUX_SOURCE_LINE" "$TMUXRC"; then
		return 0
	fi

	grep -Eq '^[[:space:]]*source-file[[:space:]].*kaku/tmux/kaku\.tmux\.conf([[:space:]]|$)' "$TMUXRC"
}

ensure_kaku_tmux_integration() {
	# GUI-launched shells inherit a minimal PATH (no Homebrew). Probe common
	# install locations so tmux is found even when PATH is stripped down.
	# GUI-launched shells inherit a minimal PATH (no Homebrew/MacPorts). Probe
	# common install locations so tmux is found even when PATH is stripped down.
	local tmux_cmd=""
	if command -v tmux >/dev/null 2>&1; then
		tmux_cmd="tmux"
	elif [[ -x /opt/homebrew/bin/tmux ]]; then
		tmux_cmd=/opt/homebrew/bin/tmux
	elif [[ -x /usr/local/bin/tmux ]]; then
		tmux_cmd=/usr/local/bin/tmux
	elif [[ -x /opt/local/bin/tmux ]]; then
		tmux_cmd=/opt/local/bin/tmux
	fi

	if [[ -z "$tmux_cmd" ]]; then
		echo -e "  ${BLUE}•${NC} ${BOLD}Integrate${NC}   Skipped tmux integration ${NC}(tmux not found)${NC}"
		return
	fi

	write_kaku_tmux_file
	normalize_kaku_tmux_source_line

	if has_kaku_tmux_source_line; then
		echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Already linked in .tmux.conf"
	else
		backup_tmuxrc_once
		if [[ -f "$TMUXRC" && -s "$TMUXRC" ]]; then
			echo "" >>"$TMUXRC"
		fi
		echo "$TMUX_SOURCE_LINE" >>"$TMUXRC"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Successfully patched .tmux.conf"
	fi
}

ensure_kaku_tmux_integration

# 5. Configure .zshrc
PATH_LINE='[[ ":$PATH:" != *":$HOME/.config/kaku/zsh/bin:"* ]] && export PATH="$HOME/.config/kaku/zsh/bin:$PATH" # Kaku PATH Integration'
SOURCE_LINE='[[ -f "$HOME/.config/kaku/zsh/kaku.zsh" ]] && source "$HOME/.config/kaku/zsh/kaku.zsh" # Kaku Shell Integration'
LEGACY_INLINE_BLOCK_PRESERVED=0

# SYNC: the heredoc below must stay in sync with KAKU_LEGACY_INLINE_KNOWN_LINES
# in kaku/src/reset.rs. When adding or removing lines, update both places.
legacy_inline_block_has_only_kaku_managed_lines() {
	local line

	for line in "$@"; do
		if [[ -z "${line//[[:space:]]/}" ]]; then
			continue
		fi

		if ! grep -Fqx -- "$line" <<'EOF'
# Kaku Zsh Integration - DO NOT EDIT MANUALLY
# This file is managed by Kaku.app. Any changes may be overwritten.
export KAKU_ZSH_DIR="$HOME/.config/kaku/zsh"
# Add bundled binaries to PATH
export PATH="$KAKU_ZSH_DIR/bin:$PATH"
# Initialize Starship (Cross-shell prompt)
# Check file existence to avoid "no such file" errors in some zsh configurations
if [[ -x "$KAKU_ZSH_DIR/bin/starship" ]]; then
    eval "$("$KAKU_ZSH_DIR/bin/starship" init zsh)"
elif command -v starship &> /dev/null; then
    # Fallback to system starship if available
    eval "$(starship init zsh)"
fi
# Enable color output for ls
export CLICOLOR=1
export LSCOLORS="Gxfxcxdxbxegedabagacad"
# Smart History Configuration
HISTSIZE=50000
SAVEHIST=50000
HISTFILE="$HOME/.zsh_history"
setopt HIST_IGNORE_DUPS          # Do not record an event that was just recorded again
setopt HIST_IGNORE_SPACE         # Do not record an event starting with a space
setopt HIST_FIND_NO_DUPS         # Do not display a line previously found
setopt SHARE_HISTORY             # Share history between all sessions
setopt APPEND_HISTORY            # Append history to the history file (no overwriting)
# Set default Zsh options
setopt interactive_comments
bindkey -e
# Directory Navigation Options
setopt auto_cd
setopt auto_pushd
setopt pushd_ignore_dups
setopt pushdminus
# Common Aliases (Intuitive defaults)
alias ll='ls -lhF'   # Detailed list (human-readable sizes, no hidden files)
alias la='ls -lAhF'  # List all (including hidden, except . and ..)
alias l='ls -CF'     # Compact list
# Directory Navigation
alias ...='../..'
alias ....='../../..'
alias .....='../../../..'
alias ......='../../../../..'
alias md='mkdir -p'
alias rd=rmdir
# Grep Colors
alias grep='grep --color=auto'
alias egrep='grep -E --color=auto'
alias fgrep='grep -F --color=auto'
# Common Git Aliases (The Essentials)
alias g='git'
alias ga='git add'
alias gaa='git add --all'
alias gb='git branch'
alias gbd='git branch -d'
alias gc='git commit -v'
alias gcmsg='git commit -m'
alias gco='git checkout'
alias gcb='git checkout -b'
alias gd='git diff'
alias gds='git diff --staged'
alias gf='git fetch'
alias gl='git pull'
alias gp='git push'
alias gst='git status'
alias gss='git status -s'
alias glo='git log --oneline --decorate'
alias glg='git log --stat'
alias glgp='git log --stat -p'
# Load Plugins (Performance Optimized)
# Load zsh-completions into fpath before compinit
if [[ -d "$KAKU_ZSH_DIR/plugins/zsh-completions/src" ]]; then
    fpath=("$KAKU_ZSH_DIR/plugins/zsh-completions/src" $fpath)
fi
# Optimized compinit: Use cache and only rebuild when needed (~30ms saved)
autoload -Uz compinit
if [[ -n "${ZDOTDIR:-$HOME}/.zcompdump"(#qN.mh+24) ]]; then
    # Rebuild completion cache if older than 24 hours
    compinit
else
    # Load from cache (much faster)
    compinit -C
fi
# Load zsh-z (smart directory jumping) - Fast, no delay needed
if [[ -f "$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh" ]]; then
    # Default to smart case matching so `z kaku` prefers `Kaku` over lowercase
    # path entries. Users can still override this in their own shell config.
    : "${ZSHZ_CASE:=smart}"
    export ZSHZ_CASE
    source "$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh"
fi
# Load zsh-autosuggestions - Async, minimal impact
if [[ -f "$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh" ]]; then
    source "$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh"
    # Smart Tab: accept inline autosuggestion if present, otherwise run completion.
    # Avoids running completion immediately after accepting a suggestion, which can
    # introduce unexpected spacing for some command completers.
    # Keep this widget out of autosuggestions rebinding, otherwise POSTDISPLAY is
    # cleared before our condition check and Tab always falls back to completion.
    typeset -ga ZSH_AUTOSUGGEST_IGNORE_WIDGETS
    ZSH_AUTOSUGGEST_IGNORE_WIDGETS+=(kaku_tab_accept_or_complete)
    kaku_tab_accept_or_complete() {
        if [[ -n "$POSTDISPLAY" ]]; then
            zle autosuggest-accept
        else
            zle expand-or-complete
        fi
    }
    zle -N kaku_tab_accept_or_complete
    bindkey -M emacs '^I' kaku_tab_accept_or_complete
    bindkey -M main '^I' kaku_tab_accept_or_complete
    bindkey -M viins '^I' kaku_tab_accept_or_complete
fi
# Defer zsh-syntax-highlighting to first prompt (~40ms saved at startup)
# This plugin must be loaded LAST, and we delay it for faster shell startup
source "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"
if [[ -f "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" ]]; then
    # Simplified highlighters for better performance (removed brackets, pattern, cursor)
    export ZSH_HIGHLIGHT_HIGHLIGHTERS=(main)
    # Defer loading until first prompt display
    zsh_syntax_highlighting_defer() {
        source "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"
        # Remove this hook after first run
        precmd_functions=("${precmd_functions[@]:#zsh_syntax_highlighting_defer}")
    }
    # Hook into precmd (runs before prompt is displayed)
    precmd_functions+=(zsh_syntax_highlighting_defer)
fi
EOF
		then
			return 1
		fi
	done

	return 0
}

# Migrate legacy inline block from older versions to the single source-line model.
cleanup_legacy_inline_block() {
	if [[ ! -f "$ZSHRC" ]]; then
		return
	fi

	if ! grep -q "^# Kaku Shell Integration$" "$ZSHRC"; then
		return
	fi

	if ! grep -q "zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" "$ZSHRC"; then
		return
	fi

	if ! grep -q "KAKU_ZSH_DIR" "$ZSHRC"; then
		return
	fi

	local tmp_file
	local line
	local -a block_lines=()
	local in_block=0
	local saw_kaku_var=0
	local saw_syntax=0
	local removed_block=0
	local preserved_block=0
	local skip_blank_after_removed=0
	tmp_file="$(mktemp "${TMPDIR:-/tmp}/kaku-zshrc.XXXXXX")"

	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ "$in_block" == "0" ]]; then
			if [[ "$skip_blank_after_removed" == "1" && -z "${line//[[:space:]]/}" ]]; then
				continue
			fi
			skip_blank_after_removed=0

			if [[ "$line" == "# Kaku Shell Integration" ]]; then
				in_block=1
				saw_kaku_var=0
				saw_syntax=0
				block_lines=()
				continue
			fi

			printf '%s\n' "$line" >>"$tmp_file"
			continue
		fi

		block_lines+=("$line")
		[[ "$line" == *KAKU_ZSH_DIR* ]] && saw_kaku_var=1
		[[ "$line" == *zsh-syntax-highlighting/zsh-syntax-highlighting.zsh* ]] && saw_syntax=1

		if [[ "$saw_kaku_var" == "1" && "$saw_syntax" == "1" && "$line" =~ ^[[:space:]]*fi[[:space:]]*$ ]]; then
			if legacy_inline_block_has_only_kaku_managed_lines "${block_lines[@]}"; then
				removed_block=1
				skip_blank_after_removed=1
			else
				preserved_block=1
				printf '%s\n' "# Kaku Shell Integration" >>"$tmp_file"
				local block_line
				for block_line in "${block_lines[@]}"; do
					printf '%s\n' "$block_line" >>"$tmp_file"
				done
			fi

			in_block=0
			saw_kaku_var=0
			saw_syntax=0
			block_lines=()
		fi
	done <"$ZSHRC"

	if [[ "$in_block" == "1" ]]; then
		rm -f "$tmp_file"
		LEGACY_INLINE_BLOCK_PRESERVED=1
		echo -e "${YELLOW}Warning: found unterminated legacy Kaku block; leaving .zshrc unchanged.${NC}"
		return
	fi

	if ! cmp -s "$ZSHRC" "$tmp_file"; then
		backup_zshrc_once
		mv "$tmp_file" "$ZSHRC"
		if [[ "$removed_block" == "1" ]]; then
			echo -e "  ${GREEN}✓${NC} ${BOLD}Migrate${NC}     Removed legacy inline Kaku block from .zshrc"
		fi
		if [[ "$preserved_block" == "1" ]]; then
			LEGACY_INLINE_BLOCK_PRESERVED=1
			echo -e "${YELLOW}Warning: kept legacy Kaku block with custom lines to avoid deleting user shell config.${NC}"
		fi
	else
		rm -f "$tmp_file"
		if [[ "$preserved_block" == "1" ]]; then
			LEGACY_INLINE_BLOCK_PRESERVED=1
			echo -e "${YELLOW}Warning: kept legacy Kaku block with custom lines to avoid deleting user shell config.${NC}"
		fi
	fi
}

cleanup_legacy_inline_block

normalize_kaku_path_line() {
	if [[ ! -f "$ZSHRC" ]]; then
		return
	fi

	local tmp_file
	tmp_file="$(mktemp "${TMPDIR:-/tmp}/kaku-zshrc.XXXXXX")"

	# Exit codes: 0 = replaced exactly 1 line, 2 = collapsed duplicates, 3 = no match.
	# Only normalize Kaku's single-line PATH guard variants; leave user-managed
	# multi-line or custom PATH logic untouched.
	if awk -v path_line="$PATH_LINE" '
BEGIN { replaced = 0; extra = 0 }
{
	if ($0 ~ /^[[:space:]]*#/ ) {
		print
		next
	}

	if ($0 ~ /^[[:space:]]*\[\[/ &&
	    $0 ~ /kaku\/zsh\/bin/ &&
	    $0 ~ /&&[[:space:]]*export[[:space:]]+PATH=/) {
		if (!replaced) {
			print path_line
			replaced = 1
		} else {
			extra++
		}
		next
	}

	print
}
END {
	if (replaced && extra > 0) {
		exit 2
	} else if (replaced) {
		exit 0
	}
	exit 3
}
' "$ZSHRC" >"$tmp_file"; then
		if ! cmp -s "$ZSHRC" "$tmp_file"; then
			backup_zshrc_once
			mv "$tmp_file" "$ZSHRC"
			echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Updated Kaku PATH line in .zshrc"
		else
			rm -f "$tmp_file"
		fi
	else
		local awk_status="$?"
		if [[ "$awk_status" == "2" ]]; then
			if ! cmp -s "$ZSHRC" "$tmp_file"; then
				backup_zshrc_once
				mv "$tmp_file" "$ZSHRC"
				echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Removed duplicate Kaku PATH line(s) from .zshrc"
			else
				rm -f "$tmp_file"
			fi
		else
			rm -f "$tmp_file"
			if [[ "$awk_status" != "3" ]]; then
				echo -e "${YELLOW}Warning: failed to normalize Kaku PATH line in .zshrc; leaving it unchanged.${NC}"
			fi
		fi
	fi
}

normalize_kaku_path_line

normalize_kaku_source_line() {
	if [[ ! -f "$ZSHRC" ]]; then
		return
	fi

	local tmp_file
	tmp_file="$(mktemp "${TMPDIR:-/tmp}/kaku-zshrc.XXXXXX")"

	# Exit codes: 0 = replaced exactly 1 line, 2 = collapsed duplicates, 3 = no match.
	if awk -v source_line="$SOURCE_LINE" '
BEGIN { replaced = 0; extra = 0 }
{
	if ($0 ~ /^[[:space:]]*#/ ) {
		print
		next
	}

	if ($0 ~ /^[[:space:]]*\[\[/ &&
	    $0 ~ /kaku\/zsh\/kaku\.zsh/ &&
	    $0 ~ /&&[[:space:]]*source[[:space:]]/) {
		if (!replaced) {
			print source_line
			replaced = 1
		} else {
			extra++
		}
		next
	}

	print
}
END {
	if (replaced && extra > 0) {
		exit 2
	} else if (replaced) {
		exit 0
	}
	exit 3
}
' "$ZSHRC" >"$tmp_file"; then
		if ! cmp -s "$ZSHRC" "$tmp_file"; then
			backup_zshrc_once
			mv "$tmp_file" "$ZSHRC"
			echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Updated Kaku source line in .zshrc"
		else
			rm -f "$tmp_file"
		fi
	else
		# Capture $? before any `local` declaration; zsh resets $? to 0 on `local`.
		local awk_status="$?"
		if [[ "$awk_status" == "2" ]]; then
			if ! cmp -s "$ZSHRC" "$tmp_file"; then
				backup_zshrc_once
				mv "$tmp_file" "$ZSHRC"
				echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Removed duplicate Kaku source line(s) from .zshrc"
			else
				rm -f "$tmp_file"
			fi
		else
			rm -f "$tmp_file"
			if [[ "$awk_status" != "3" ]]; then
				echo -e "${YELLOW}Warning: failed to normalize Kaku source line in .zshrc; leaving it unchanged.${NC}"
			fi
		fi
	fi
}

normalize_kaku_source_line

has_kaku_path_line() {
	if [[ ! -f "$ZSHRC" ]]; then
		return 1
	fi

	if grep -Fqx "$PATH_LINE" "$ZSHRC"; then
		return 0
	fi

	grep -Eq '^[[:space:]]*\[\[.*kaku/zsh/bin.*\]\][[:space:]]*&&[[:space:]]*export[[:space:]]+PATH=.*kaku/zsh/bin' "$ZSHRC"
}

has_kaku_source_line() {
	if [[ ! -f "$ZSHRC" ]]; then
		return 1
	fi

	# Prefer exact match for the managed line to avoid false positives from comments.
	if grep -Fqx "$SOURCE_LINE" "$ZSHRC"; then
		return 0
	fi

	# Fallback: accept equivalent active source lines while avoiding comment-only matches.
	grep -Eq '^[[:space:]]*\[\[.*kaku/zsh/kaku\.zsh.*\]\][[:space:]]*&&[[:space:]]*source[[:space:]].*kaku/zsh/kaku\.zsh([[:space:]]|$)' "$ZSHRC"
}

# Check if the managed lines already exist
if has_kaku_path_line && has_kaku_source_line; then
	echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Already linked in .zshrc"
elif [[ "$LEGACY_INLINE_BLOCK_PRESERVED" == "1" ]]; then
	echo -e "  ${BLUE}•${NC} ${BOLD}Integrate${NC}   Preserved legacy inline Kaku block ${NC}(move custom lines outside it, then rerun kaku init)${NC}"
else
	# Backup existing .zshrc only if it doesn't have Kaku logic yet
	backup_zshrc_once

	if [[ -f "$ZSHRC" && -s "$ZSHRC" ]]; then
		echo "" >>"$ZSHRC"
	fi
	if ! has_kaku_path_line; then
		echo "$PATH_LINE" >>"$ZSHRC"
	fi
	if ! has_kaku_source_line; then
		echo "$SOURCE_LINE" >>"$ZSHRC"
	fi
	echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Successfully patched .zshrc"
fi

# 6. Configure TouchID for Sudo (Optional)
# Reference: logic from www/mole/bin/touchid.sh
configure_touchid() {
	PAM_SUDO_FILE="/etc/pam.d/sudo"
	PAM_SUDO_LOCAL_FILE="/etc/pam.d/sudo_local"
	PAM_TID_LINE="auth       sufficient     pam_tid.so"

	# 1. Check if already enabled
	if grep -q "pam_tid.so" "$PAM_SUDO_LOCAL_FILE" 2>/dev/null || grep -q "pam_tid.so" "$PAM_SUDO_FILE" 2>/dev/null; then
		return 0
	fi

	# 2. Check compatibility (Apple Silicon or Intel Macs with TouchID)
	if ! command -v bioutil &>/dev/null; then
		# Fallback check for arm64
		if [[ "$(uname -m)" != "arm64" ]]; then
			return 0
		fi
	fi

	echo -en "\n${BOLD}TouchID for sudo${NC}  Enable fingerprint authentication? (Y/n) "
	read -p "" -n 1 -r
	# Default to Yes (proceed if reply is empty or y/Y)
	if [[ -n "$REPLY" && ! $REPLY =~ ^[Yy]$ ]]; then
		echo "" # Clear the line after Skip
		return 0
	fi
	echo "" # Move to next line for result display

	# Try the modern sudo_local method (macOS Sonoma+)
	if grep -q "sudo_local" "$PAM_SUDO_FILE" 2>/dev/null; then
		echo "# sudo_local: local customizations for sudo" | sudo tee "$PAM_SUDO_LOCAL_FILE" >/dev/null
		echo "$PAM_TID_LINE" | sudo tee -a "$PAM_SUDO_LOCAL_FILE" >/dev/null
		sudo chmod 444 "$PAM_SUDO_LOCAL_FILE"
		sudo chown root:wheel "$PAM_SUDO_LOCAL_FILE"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Sudo${NC}        Enabled via sudo_local"
	else
		# Fallback to editing /etc/pam.d/sudo
		sudo awk -v line="$PAM_TID_LINE" 'NR==2{print line} 1' "$PAM_SUDO_FILE" >"${PAM_SUDO_FILE}.tmp" &&
			sudo mv "${PAM_SUDO_FILE}.tmp" "$PAM_SUDO_FILE"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Sudo${NC}        Enabled via /etc/pam.d/sudo"
	fi
}

if [[ "$UPDATE_ONLY" != "true" ]]; then
	configure_touchid
fi
