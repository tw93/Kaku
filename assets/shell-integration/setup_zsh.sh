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
USER_CONFIG_DIR="$HOME/.config/kaku/zsh"
KAKU_INIT_FILE="$USER_CONFIG_DIR/kaku.zsh"
STARSHIP_CONFIG="$HOME/.config/starship.toml"
ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"
BACKUP_SUFFIX=".kaku-backup-$(date +%s)"
ZSHRC_BACKED_UP=0

backup_zshrc_once() {
	if [[ -f "$ZSHRC" ]] && [[ "$ZSHRC_BACKED_UP" -eq 0 ]]; then
		cp "$ZSHRC" "$ZSHRC$BACKUP_SUFFIX"
		ZSHRC_BACKED_UP=1
	fi
}

# Ensure vendor resources exist
if [[ ! -d "$VENDOR_DIR" ]]; then
	echo -e "${YELLOW}Error: Vendor resources not found in $VENDOR_DIR${NC}"
	exit 1
fi

echo -e "${BOLD}Setting up Kaku Shell Environment${NC}"

# 1. Prepare User Config Directory
mkdir -p "$USER_CONFIG_DIR"
mkdir -p "$USER_CONFIG_DIR/plugins"
mkdir -p "$USER_CONFIG_DIR/bin"

# 2. Copy Resources to User Directory (persistence)
# Copy Starship binary
if [[ -f "$VENDOR_DIR/starship" ]]; then
	cp "$VENDOR_DIR/starship" "$USER_CONFIG_DIR/bin/"
	chmod +x "$USER_CONFIG_DIR/bin/starship"
else
	echo -e "${YELLOW}Warning: Starship binary not found in $VENDOR_DIR${NC}"
	echo -e "${YELLOW}         Prompt will not be available until you reinstall Kaku.${NC}"
fi

# Validate required plugin directories up front.
# setup_zsh.sh may be run standalone, so provide a clear dependency hint.
for plugin in zsh-z zsh-autosuggestions zsh-syntax-highlighting zsh-completions; do
	if [[ ! -d "$VENDOR_DIR/$plugin" ]]; then
		echo -e "${YELLOW}Error: Missing plugin vendor directory: $VENDOR_DIR/$plugin${NC}"
		echo -e "${YELLOW}Hint: Run scripts/download_vendor.sh before setup_zsh.sh.${NC}"
		exit 1
	fi
done

# Copy Plugins
cp -R "$VENDOR_DIR/zsh-z" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-autosuggestions" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-syntax-highlighting" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-completions" "$USER_CONFIG_DIR/plugins/"
echo -e "  ${GREEN}✓${NC} ${BOLD}Tools${NC}       Installed Starship & Zsh plugins ${NC}(~/.config/kaku/zsh)${NC}"

# Copy Starship Config (if not exists)
STARSHIP_CONFIG_CREATED=false
if [[ ! -f "$STARSHIP_CONFIG" ]]; then
	if [[ -f "$VENDOR_DIR/starship.toml" ]]; then
		mkdir -p "$(dirname "$STARSHIP_CONFIG")"
		cp "$VENDOR_DIR/starship.toml" "$STARSHIP_CONFIG"
		STARSHIP_CONFIG_CREATED=true
		echo -e "  ${GREEN}✓${NC} ${BOLD}Config${NC}      Initialized starship.toml ${NC}(~/.config/starship.toml)${NC}"
	fi
fi

# Keep default prompt clean for local development.
# User-owned starship.toml should not be modified after creation.
if [[ "$STARSHIP_CONFIG_CREATED" == "true" ]] && [[ -f "$STARSHIP_CONFIG" ]]; then
	starship_cloud_patched=false

	ensure_starship_cloud_module_disabled() {
		local module="$1"
		if ! grep -Eq "^[[:space:]]*\\[$module\\]" "$STARSHIP_CONFIG"; then
			cat <<EOF >>"$STARSHIP_CONFIG"

[$module]
disabled = true
EOF
			starship_cloud_patched=true
		fi
	}

	for module in aws gcloud azure kubernetes openstack docker_context terraform; do
		ensure_starship_cloud_module_disabled "$module"
	done

	if [[ "$starship_cloud_patched" == "true" ]]; then
		echo -e "  ${GREEN}✓${NC} ${BOLD}Prompt${NC}      Initialized cloud context defaults in starship.toml"
	fi
fi

# 3. Create/Update Kaku Init File (managed by Kaku)
cat <<EOF >"$KAKU_INIT_FILE"
# Kaku Zsh Integration - DO NOT EDIT MANUALLY
# This file is managed by Kaku.app. Any changes may be overwritten.

export KAKU_ZSH_DIR="\$HOME/.config/kaku/zsh"

# Add bundled binaries to PATH
export PATH="\$KAKU_ZSH_DIR/bin:\$PATH"

# Initialize Starship (Cross-shell prompt)
# Check file existence to avoid "no such file" errors in some zsh configurations
if [[ -x "\$KAKU_ZSH_DIR/bin/starship" ]]; then
    eval "\$("\$KAKU_ZSH_DIR/bin/starship" init zsh)"
elif command -v starship &> /dev/null; then
    # Fallback to system starship if available
    eval "\$(starship init zsh)"
fi

# Enable color output for ls
export CLICOLOR=1
export LSCOLORS="Gxfxcxdxbxegedabagacad"

# Smart History Configuration
HISTSIZE=50000
SAVEHIST=50000
HISTFILE="\$HOME/.zsh_history"
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
if [[ -d "\$KAKU_ZSH_DIR/plugins/zsh-completions/src" ]]; then
    fpath=("\$KAKU_ZSH_DIR/plugins/zsh-completions/src" \$fpath)
fi

# Optimized compinit: Use cache and only rebuild when needed (~30ms saved)
autoload -Uz compinit
if [[ -n "\${ZDOTDIR:-\$HOME}/.zcompdump"(#qN.mh+24) ]]; then
    # Rebuild completion cache if older than 24 hours
    compinit
else
    # Load from cache (much faster)
    compinit -C
fi

# Load zsh-z (smart directory jumping) - Fast, no delay needed
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh" ]]; then
    # Default to smart case matching so `z kaku` prefers `Kaku` over lowercase
    # path entries. Users can still override this in their own shell config.
    : "\${ZSHZ_CASE:=smart}"
    export ZSHZ_CASE
    source "\$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh"
fi

# Load zsh-autosuggestions - Async, minimal impact
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh" ]]; then
    source "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh"

    # Smart Tab: accept inline autosuggestion if present, otherwise run completion.
    # Avoids running completion immediately after accepting a suggestion, which can
    # introduce unexpected spacing for some command completers.
    # Keep this widget out of autosuggestions rebinding, otherwise POSTDISPLAY is
    # cleared before our condition check and Tab always falls back to completion.
    typeset -ga ZSH_AUTOSUGGEST_IGNORE_WIDGETS
    ZSH_AUTOSUGGEST_IGNORE_WIDGETS+=(kaku_tab_accept_or_complete)
    kaku_tab_accept_or_complete() {
        if [[ -n "\$POSTDISPLAY" ]]; then
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
if [[ -f "\$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" ]]; then
    # Simplified highlighters for better performance (removed brackets, pattern, cursor)
    export ZSH_HIGHLIGHT_HIGHLIGHTERS=(main)

    # Defer loading until first prompt display
    zsh_syntax_highlighting_defer() {
        source "\$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"

        # Remove this hook after first run
        precmd_functions=("\${precmd_functions[@]:#zsh_syntax_highlighting_defer}")
    }

    # Hook into precmd (runs before prompt is displayed)
    precmd_functions+=(zsh_syntax_highlighting_defer)
fi
EOF

echo -e "  ${GREEN}✓${NC} ${BOLD}Script${NC}      Generated kaku.zsh init script"

# 4. Configure .zshrc
SOURCE_LINE="[[ -f \"\$HOME/.config/kaku/zsh/kaku.zsh\" ]] && source \"\$HOME/.config/kaku/zsh/kaku.zsh\" # Kaku Shell Integration"

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
	tmp_file="$(mktemp "${TMPDIR:-/tmp}/kaku-zshrc.XXXXXX")"

	if awk '
BEGIN { in_block = 0; saw_syntax = 0; saw_kaku_var = 0 }
{
	if (!in_block && $0 == "# Kaku Shell Integration") {
		in_block = 1
		saw_syntax = 0
		saw_kaku_var = 0
		next
	}

	if (in_block) {
		if ($0 ~ /KAKU_ZSH_DIR/) {
			saw_kaku_var = 1
			next
		}

		if ($0 ~ /zsh-syntax-highlighting\/zsh-syntax-highlighting\.zsh/) {
			saw_syntax = 1
			next
		}

		if (saw_kaku_var && saw_syntax && $0 ~ /^[[:space:]]*fi[[:space:]]*$/) {
			in_block = 0
			saw_syntax = 0
			saw_kaku_var = 0
			next
		}

		next
	}

	print
}
END {
	if (in_block) {
		exit 42
	}
}
' "$ZSHRC" >"$tmp_file"; then
		if ! cmp -s "$ZSHRC" "$tmp_file"; then
			backup_zshrc_once
			mv "$tmp_file" "$ZSHRC"
			echo -e "  ${GREEN}✓${NC} ${BOLD}Migrate${NC}     Removed legacy inline Kaku block from .zshrc"
		else
			rm -f "$tmp_file"
		fi
	else
		rm -f "$tmp_file"
		echo -e "${YELLOW}Warning: found legacy Kaku block but failed to migrate it safely; leaving .zshrc unchanged.${NC}"
	fi
}

cleanup_legacy_inline_block

# Check if the source line already exists
if grep -q "kaku/zsh/kaku.zsh" "$ZSHRC" 2>/dev/null; then
	echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Already linked in .zshrc"
else
	# Backup existing .zshrc only if it doesn't have Kaku logic yet
	backup_zshrc_once

	# Append the single source line
	echo -e "\n$SOURCE_LINE" >>"$ZSHRC"
	echo -e "  ${GREEN}✓${NC} ${BOLD}Integrate${NC}   Successfully patched .zshrc"
fi

# 5. Configure TouchID for Sudo (Optional)
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
