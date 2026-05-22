#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SHELL_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$SHELL_DIR/../.." && pwd)"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/kaku-setup-zsh-smoke.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

tmp_home="$tmp_dir/home"
tmp_vendor="$tmp_dir/vendor"
mkdir -p "$tmp_home" "$tmp_vendor"

for plugin in fast-syntax-highlighting zsh-autosuggestions zsh-completions zsh-z; do
  mkdir -p "$tmp_vendor/$plugin"
done

if [[ -f "$REPO_ROOT/assets/vendor/starship.toml" ]]; then
  cp "$REPO_ROOT/assets/vendor/starship.toml" "$tmp_vendor/starship.toml"
else
  printf '# test starship config\n' >"$tmp_vendor/starship.toml"
fi

output_log="$tmp_dir/output.log"
error_log="$tmp_dir/error.log"

if ! HOME="$tmp_home" \
  KAKU_INIT_INTERNAL=1 \
  KAKU_SKIP_TOOL_BOOTSTRAP=1 \
  KAKU_SKIP_TERMINFO_BOOTSTRAP=1 \
  KAKU_VENDOR_DIR="$tmp_vendor" \
  bash "$SHELL_DIR/setup_zsh.sh" --update-only >"$output_log" 2>"$error_log"; then
  cat "$output_log" >&2
  cat "$error_log" >&2
  fail "setup_zsh.sh --update-only failed"
fi

if grep -Fq "local: can only be used in a function" "$output_log" "$error_log"; then
  cat "$output_log" >&2
  cat "$error_log" >&2
  fail "setup_zsh.sh used local outside a function"
fi

[[ -f "$tmp_home/.config/starship.toml" ]] || fail "starship.toml was not initialized"
[[ -f "$tmp_home/.config/kaku/zsh/kaku.zsh" ]] || fail "kaku.zsh was not generated"
[[ -f "$tmp_home/.zshrc" ]] || fail ".zshrc was not patched"

echo "setup_zsh update-only smoke test passed"
