#!/bin/bash
# Kaku Fish Setup Script
# Configures a "batteries-included" Fish environment using Kaku's bundled resources.
# It is designed to be safe: can be re-run at any time.

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

# Resolve resources directory
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
if [[ -n "${KAKU_VENDOR_DIR:-}" && -d "${KAKU_VENDOR_DIR}" ]]; then
	VENDOR_DIR="${KAKU_VENDOR_DIR}"
fi

TOOL_INSTALL_SCRIPT="$SCRIPT_DIR/install_cli_tools.sh"
if [[ ! -f "$TOOL_INSTALL_SCRIPT" ]]; then
	TOOL_INSTALL_SCRIPT="$RESOURCES_DIR/install_cli_tools.sh"
fi

USER_CONFIG_DIR="$HOME/.config/kaku/fish"
KAKU_INIT_FILE="$USER_CONFIG_DIR/kaku.fish"
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
FISH_CONF_D_DIR="$HOME/.config/fish/conf.d"
FISH_CONF_D_FILE="$FISH_CONF_D_DIR/kaku.fish"
TMUXRC="$HOME/.tmux.conf"
BACKUP_SUFFIX=".kaku-backup-$(date +%s)"
TMUXRC_BACKED_UP=0

if [[ -d "$SCRIPT_DIR/yazi-flavors" ]]; then
	KAKU_YAZI_FLAVOR_SOURCE_DIR="$SCRIPT_DIR/yazi-flavors"
else
	KAKU_YAZI_FLAVOR_SOURCE_DIR="$RESOURCES_DIR/yazi-flavors"
fi

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

	if grep -Fq '# Kaku-aligned theme for Yazi 26.x' "$YAZI_THEME_FILE"; then
		return 0
	fi
	local normalized expected
	normalized="$(sed -e 's/[[:space:]]*$//' -e '/^[[:space:]]*$/d' "$YAZI_THEME_FILE")"
	expected=$'[mgr]\nborder_symbol = "│"\nborder_style = { fg = "#555555" }\n[indicator]\npadding = { open = "", close = "" }'
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
	# The yazi wrapper for fish lives in fish/bin/ but its content is shell-agnostic bash.
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
	if [[ "${KAKU_SKIP_TERMINFO_BOOTSTRAP:-0}" == "1" ]]; then
		return
	fi

	if infocmp kaku >/dev/null 2>&1; then
		return
	fi

	local target_dir="$HOME/.terminfo"
	local compiled_entry="$RESOURCES_DIR/terminfo/6b/kaku"
	local source_entry=""

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

echo -e "${BOLD}Setting up Kaku Fish Shell Environment${NC}"

# 1. Prepare User Config Directory
mkdir -p "$USER_CONFIG_DIR"
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

# Copy Starship Config (if not exists)
if [[ ! -f "$STARSHIP_CONFIG" ]]; then
	if [[ -f "$VENDOR_DIR/starship.toml" ]]; then
		mkdir -p "$(dirname "$STARSHIP_CONFIG")"
		cp "$VENDOR_DIR/starship.toml" "$STARSHIP_CONFIG"
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized starship.toml ${NC}(~/.config/starship.toml)${NC}"
	fi
fi

# Initialize Yazi configs if not yet created
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

# 3. Configure tmux (Optional)
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

# 4. Generate Kaku Fish Init File
cat <<'EOF' >"$KAKU_INIT_FILE"
# Kaku Fish Integration - DO NOT EDIT MANUALLY
# This file is managed by Kaku.app. Any changes may be overwritten.

# === PATH ===
fish_add_path "$HOME/.config/kaku/fish/bin"

# === Starship prompt ===
if command -q starship
    starship init fish | source
end

# === Zoxide ===
if command -q zoxide
    zoxide init fish | source
end

# === SSH TERM fix ===
# Auto-set TERM to xterm-256color for SSH connections since remote hosts
# typically lack the kaku terminfo entry.
function ssh
    if test "$TERM" = kaku; and not set -q KAKU_SSH_SKIP_TERM_FIX
        TERM=xterm-256color command ssh $argv
    else
        command ssh $argv
    end
end

# === sudo TERM fix ===
# sudo resets TERMINFO_DIRS, so root processes may fail with "unknown terminal kaku".
function sudo
    if test "$TERM" = kaku; and not set -q KAKU_SUDO_SKIP_TERM_FIX
        TERM=xterm-256color command sudo $argv
    else
        command sudo $argv
    end
end

# === OSC 7: Working directory reporting ===
function __kaku_osc7 --on-event fish_prompt
    printf '\033]7;file://%s%s\033\\' (hostname) $PWD
end

# === OSC 133: Semantic prompt zones ===
function __kaku_semantic_preexec --on-event fish_preexec
    printf '\033]133;C;\007'
end
function __kaku_semantic_precmd --on-event fish_prompt
    printf '\033]133;A\007'
end

# === OSC 1337: User variables (AI fix hooks) ===
function __kaku_set_user_var
    # Only emit when inside a Kaku/WezTerm pane
    if test "$TERM" != kaku; and not set -q WEZTERM_PANE
        return
    end
    if set -q WEZTERM_SHELL_SKIP_USER_VARS; and test "$WEZTERM_SHELL_SKIP_USER_VARS" = 1
        return
    end
    if not command -q base64
        return
    end
    set -l encoded (printf '%s' $argv[2] | base64 | tr -d '\r\n')
    if set -q TMUX
        printf '\033Ptmux;\033\033]1337;SetUserVar=%s=%s\007\033\\' $argv[1] $encoded
    else
        printf '\033]1337;SetUserVar=%s=%s\007' $argv[1] $encoded
    end
end

# Capture last command for AI suggestion (preexec fires before command runs)
function __kaku_ai_preexec --on-event fish_preexec
    if set -q KAKU_AUTO_DISABLE
        return
    end
    __kaku_set_user_var kaku_last_cmd $argv[1]
    set -g _kaku_ai_cmd_pending 1
end

# Capture exit code for AI suggestion (fish_prompt fires after command finishes)
function __kaku_ai_precmd --on-event fish_prompt
    set -l last_exit $status
    if set -q KAKU_AUTO_DISABLE
        set -g _kaku_ai_cmd_pending 0
        return
    end
    if not set -q _kaku_ai_cmd_pending; or test "$_kaku_ai_cmd_pending" != 1
        return
    end
    __kaku_set_user_var kaku_last_exit_code $last_exit
    set -g _kaku_ai_cmd_pending 0
end

# AI generate: intercept Enter on "# query" lines.
# fish_preexec does not fire for comment-only lines, so we bind \r to
# catch the commandline buffer before fish discards the comment.
set -g __kaku_ai_waiting 0
set -g __kaku_ai_waiting_ts 0

function __kaku_ai_query_execute
    if not set -q KAKU_AUTO_DISABLE
        set -l cmd (commandline)
        # Only intercept a single-line comment (no newline in buffer)
        if string match -qr '^#[^\n]*$' -- $cmd
            # Block repeat Enter while waiting, auto-reset after 30 seconds
            if test "$__kaku_ai_waiting" = 1
                set -l now (date +%s)
                if test (math "$now - $__kaku_ai_waiting_ts") -gt 30
                    set -g __kaku_ai_waiting 0
                else
                    return
                end
            end
            set -l query (string replace -r '^#\s*' '' -- $cmd)
            if test -n "$query"
                builtin history append -- $cmd
                set -g __kaku_ai_waiting 1
                set -g __kaku_ai_waiting_ts (date +%s)
                __kaku_set_user_var kaku_ai_query $query
                # Keep # query visible; Lua sends \x15 to clear it when result arrives
                commandline -f repaint
                return
            end
        end
    end
    set -g __kaku_ai_waiting 0
    commandline -f execute
end
bind \r __kaku_ai_query_execute
bind \n __kaku_ai_query_execute

# === Common abbreviations ===
abbr -a ll 'ls -lhF'
abbr -a la 'ls -lAhF'
abbr -a l 'ls -CF'
abbr -a grep 'grep --color=auto'
abbr -a egrep 'grep -E --color=auto'
abbr -a fgrep 'grep -F --color=auto'

# Git abbreviations
abbr -a g git
abbr -a ga 'git add'
abbr -a gaa 'git add --all'
abbr -a gb 'git branch'
abbr -a gbd 'git branch -d'
abbr -a gc 'git commit -v'
abbr -a gcmsg 'git commit -m'
abbr -a gco 'git checkout'
abbr -a gcb 'git checkout -b'
abbr -a gd 'git diff'
abbr -a gds 'git diff --staged'
abbr -a gf 'git fetch'
abbr -a gl 'git pull'
abbr -a gp 'git push'
abbr -a gst 'git status'
abbr -a gss 'git status -s'
abbr -a glo 'git log --oneline --decorate'
abbr -a glg 'git log --stat'

# Directory navigation
abbr -a md 'mkdir -p'
abbr -a ... 'cd ../..'
abbr -a .... 'cd ../../..'

# yazi launcher — cd into the directory yazi is in when you exit.
function y
    set -l yazi_cmd "$HOME/.config/kaku/fish/bin/yazi"
    if not test -x "$yazi_cmd"
        set yazi_cmd (command -v yazi 2>/dev/null; or true)
    end
    if test -z "$yazi_cmd"
        echo "yazi not found. Install it with: brew install yazi"
        return 127
    end
    set -l tmp (mktemp -t 'yazi-cwd.XXXXXX')
    $yazi_cmd $argv --cwd-file=$tmp
    if set -l cwd (command cat -- $tmp); and test -n "$cwd"; and test "$cwd" != "$PWD"
        builtin cd -- $cwd
    end
    rm -f -- $tmp
end
EOF

echo -e "  ${GREEN}✓${NC} ${BOLD}Script${NC}      Generated kaku.fish init script"

# 5. Install fish conf.d entry point
mkdir -p "$FISH_CONF_D_DIR"
cat <<EOF >"$FISH_CONF_D_FILE"
# Kaku shell integration -- managed. Remove with: kaku reset
set -l _kaku_fish_init "\$HOME/.config/kaku/fish/kaku.fish"
if test -f \$_kaku_fish_init
    source \$_kaku_fish_init
end
EOF

echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Installed ${NC}~/.config/fish/conf.d/kaku.fish${NC}"

echo ""
echo -e "${GREEN}${BOLD}Kaku Fish setup complete!${NC}"
echo ""
echo "Restart fish or run: source ~/.config/fish/conf.d/kaku.fish"
echo "Roll back anytime with: kaku reset"
