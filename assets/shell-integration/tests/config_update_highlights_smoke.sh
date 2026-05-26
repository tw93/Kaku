#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../state_common.sh
source "$SCRIPT_DIR/state_common.sh"

output="$(KAKU_CONFIG_UPDATE_LANGUAGE=en print_config_update_highlights "$SCRIPT_DIR" 12 15)"

[[ "$output" != *"  v12"* ]]
[[ "$output" != *"  v13"* ]]
[[ "$output" != *"  v14"* ]]
[[ "$output" == *"Shell integration compatibility is improved for SSH"* ]]
[[ "$output" == *"Starship prompt and AI shell hooks are more reliable"* ]]
[[ "$output" == *"regenerate the managed script correctly"* ]]
[[ "$output" == *"Yazi now follows Kaku dark and light themes automatically"* ]]

english_output="$(KAKU_CONFIG_UPDATE_LANGUAGE=en print_config_update_highlights "$SCRIPT_DIR" 20 21)"
[[ "$english_output" == *"Tab and pane close confirmation now support Never, Smart, and Always modes"* ]]
[[ "$english_output" == *"Kaku Dark now reports a dark terminal background to Hermes"* ]]
[[ "$english_output" != *"标签页和面板关闭确认"* ]]

chinese_output="$(KAKU_CONFIG_UPDATE_LANGUAGE=zh print_config_update_highlights "$SCRIPT_DIR" 20 21)"
[[ "$chinese_output" == *"标签页和面板关闭确认现在支持"* ]]
[[ "$chinese_output" == *"Kaku Dark 现在会向 Hermes 正确报告深色终端背景"* ]]
[[ "$chinese_output" != *"Tab and pane close confirmation now support"* ]]

echo "config_update_highlights smoke test passed"
