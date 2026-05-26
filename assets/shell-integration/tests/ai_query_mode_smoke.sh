#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

fail() {
  echo "ai_query_mode: $*" >&2
  exit 1
}

zsh_script="$REPO_ROOT/assets/shell-integration/setup_zsh.sh"
fish_script="$REPO_ROOT/assets/shell-integration/setup_fish.sh"

zsh_body="$(sed -n '/_kaku_ai_query_accept_line()/,/^}/p' "$zsh_script")"
fish_body="$(sed -n '/function __kaku_ai_query_execute/,/^end/p' "$fish_script")"

[[ "$zsh_body" == *'mode="explain"'* ]] || fail "zsh explain mode missing"
[[ "$zsh_body" == *'mode="candidates"'* ]] || fail "zsh candidates mode missing"
[[ "$zsh_body" == *'_kaku_set_user_var "kaku_ai_query" "[mode:\${mode}] \${body}"'* ]] \
  || fail "zsh mode-tagged user var missing"

[[ "$fish_body" == *'set mode explain'* ]] || fail "fish explain mode missing"
[[ "$fish_body" == *'set mode candidates'* ]] || fail "fish candidates mode missing"
[[ "$fish_body" == *'__kaku_set_user_var kaku_ai_query "[mode:$mode] $query"'* ]] \
  || fail "fish mode-tagged user var missing"

echo "ai_query_mode smoke test passed"
