#!/bin/bash
# Kaku Zsh Setup Script
# This script configures a "batteries-included" Zsh environment using Kaku's bundled resources.
# It is designed to be safe: it backs up existing configurations and can be re-run.

set -euo pipefail

# Configuration
KAKU_APP_DIR="/Applications/Kaku.app"
# If running from inside the app bundle (typical case), detect location
if [[ -d "../../../Contents/Resources" ]]; then
	RESOURCES_DIR="$(cd ../../../Contents/Resources && pwd)"
elif [[ -d "$KAKU_APP_DIR/Contents/Resources" ]]; then
	RESOURCES_DIR="$KAKU_APP_DIR/Contents/Resources"
else
	echo "Error: Could not locate Kaku resources."
	exit 1
fi

VENDOR_DIR="$RESOURCES_DIR/vendor"
USER_CONFIG_DIR="$HOME/.config/kaku/zsh"
STARSHIP_CONFIG="$HOME/.config/starship.toml"
ZSHRC="$HOME/.zshrc"
BACKUP_SUFFIX=".kaku-backup-$(date +%s)"

# Ensure vendor resources exist
if [[ ! -d "$VENDOR_DIR" ]]; then
	echo "Error: Vendor resources not found in $VENDOR_DIR"
	exit 1
fi

echo "✨ Setting up Kaku Shell Environment..."

# 1. Prepare User Config Directory
mkdir -p "$USER_CONFIG_DIR"
mkdir -p "$USER_CONFIG_DIR/plugins"
mkdir -p "$USER_CONFIG_DIR/bin"

# 2. Copy Resources to User Directory (persistence)
echo "📦 Installing shell tools..."

# Copy Starship binary
if [[ -f "$VENDOR_DIR/starship" ]]; then
	cp "$VENDOR_DIR/starship" "$USER_CONFIG_DIR/bin/"
	chmod +x "$USER_CONFIG_DIR/bin/starship"
fi

# Copy Zoxide binary
if [[ -f "$VENDOR_DIR/zoxide" ]]; then
	cp "$VENDOR_DIR/zoxide" "$USER_CONFIG_DIR/bin/"
	chmod +x "$USER_CONFIG_DIR/bin/zoxide"
fi

# Copy Plugins
cp -R "$VENDOR_DIR/zsh-autosuggestions" "$USER_CONFIG_DIR/plugins/"
cp -R "$VENDOR_DIR/zsh-syntax-highlighting" "$USER_CONFIG_DIR/plugins/"

# Copy Starship Config (if not exists)
if [[ ! -f "$STARSHIP_CONFIG" ]]; then
	if [[ -f "$VENDOR_DIR/starship.toml" ]]; then
		mkdir -p "$(dirname "$STARSHIP_CONFIG")"
		cp "$VENDOR_DIR/starship.toml" "$STARSHIP_CONFIG"
		echo "   -> Installed default starship.toml"
	fi
else
	echo "   -> Existing starship.toml found, skipping."
fi

# 3. Configure .zshrc
echo "📝 Configuring .zshrc..."

# Check if Kaku block already exists
if grep -q "# Kaku Shell Integration" "$ZSHRC" 2>/dev/null; then
	echo "   -> Kaku configuration already exists in .zshrc."
	echo "   -> Updating plugins only."
else
	# Backup existing .zshrc
	if [[ -f "$ZSHRC" ]]; then
		cp "$ZSHRC" "$ZSHRC$BACKUP_SUFFIX"
		echo "   -> Backed up existing .zshrc to $ZSHRC$BACKUP_SUFFIX"
	fi

	# Append Kaku configuration
	# We use single quotes for EOF to prevent expansion, but we want to expand SOME variables.
	# So we use standard EOF and escape the $ signs we want to keep literal.
	cat <<EOF >>"$ZSHRC"

# Kaku Shell Integration
# Added by Kaku.app Setup
export KAKU_ZSH_DIR="\$HOME/.config/kaku/zsh"

# Add bundled binaries to PATH
export PATH="\$KAKU_ZSH_DIR/bin:\$PATH"

# Enable color output for ls
export CLICOLOR=1
export LSCOLORS=ExFxBxDxCxegedabagacad

# Initialize Starship (Cross-shell prompt)
if command -v starship &> /dev/null; then
    eval "\$(starship init zsh)"
fi

# Initialize Zoxide (Smarter cd)
if command -v zoxide &> /dev/null; then
    eval "\$(zoxide init zsh)"
    alias cd="z"
fi

# Load Plugins
source "\$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh"
source "\$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"

EOF
	echo "   -> Added configuration to .zshrc"
fi

echo "✅ Setup complete! Restart your terminal or run 'source ~/.zshrc' to see changes."
