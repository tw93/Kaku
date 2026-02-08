#!/bin/bash
# Kaku First Run Experience
# This script is launched automatically on the first run of Kaku.

set -euo pipefail

# Resources directory resolution
if [[ -d "../../../Contents/Resources" ]]; then
	RESOURCES_DIR="$(cd ../../../Contents/Resources && pwd)"
elif [[ -d "/Applications/Kaku.app/Contents/Resources" ]]; then
	RESOURCES_DIR="/Applications/Kaku.app/Contents/Resources"
else
	# Fallback for dev environment
	DIR="$(dirname "$0")"
	RESOURCES_DIR="$DIR"
fi

SETUP_SCRIPT="$RESOURCES_DIR/setup_zsh.sh"

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
echo ""
echo "Welcome to Kaku! 🚀"
echo "The terminal built for the AI coding era."
echo ""
echo "--------------------------------------------------------"
echo "Would you like to install Kaku's enhanced shell features?"
echo "This includes:"
echo "  ✨ Starship Prompt (Beautiful & Fast)"
echo "  ⚡️ Zsh Syntax Highlighting"
echo "  🤖 Zsh Autosuggestions"
echo "--------------------------------------------------------"
echo ""

# Interactive Prompt
read -p "Install enhanced shell features? [Y/n] " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
	echo ""
	if [[ -f "$SETUP_SCRIPT" ]]; then
		"$SETUP_SCRIPT"
	else
		echo "Error: setup_zsh.sh not found at $SETUP_SCRIPT"
	fi
	mkdir -p ~/.config/kaku
	touch ~/.config/kaku/.first_run_completed
else
	echo ""
	echo "Skipping shell setup. You can run it manually later:"
	echo "$SETUP_SCRIPT"
	mkdir -p ~/.config/kaku
	touch ~/.config/kaku/.first_run_completed
fi

echo ""
echo "All set! Enjoy coding with Kaku."
echo ""
echo "Press any key to start your shell..."
read -n 1 -s -r

# Replace current process with zsh to enter the shell
exec /bin/zsh -l
