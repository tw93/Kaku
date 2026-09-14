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

if ! grep -Fq "fg=249" "$tmp_home/.config/kaku/zsh/kaku.zsh"; then
  fail "generated kaku.zsh did not set readable comment color fg=249"
fi
if grep -Fq "fg=244" "$tmp_home/.config/kaku/zsh/kaku.zsh"; then
  fail "generated kaku.zsh still contains old comment color fg=244"
fi
grep -Fq 'if { (( _kaku_in_kaku )) || [[ -n "${KAKU_PROMPT_EVERYWHERE:-}" ]]; } && command -v starship' \
  "$tmp_home/.config/kaku/zsh/kaku.zsh" \
  || fail "generated kaku.zsh did not preserve the runtime Kaku session guard"
grep -Fq 'local capability_file="$HOME/.config/kaku/ai_inline_capability"' \
  "$tmp_home/.config/kaku/zsh/kaku.zsh" \
  || fail "generated kaku.zsh did not read the inline AI capability"
grep -Fq '_kaku_set_ai_user_var "kaku_ai_query" "[mode:${mode}] ${body}"' \
  "$tmp_home/.config/kaku/zsh/kaku.zsh" \
  || fail "generated kaku.zsh did not authenticate inline AI queries"

# The generated file is sourced by the user's real zsh, so it must parse under
# zsh. A corrupted heredoc (e.g. an unescaped backtick that bash expanded at
# generation time, #450) or a stray top-level construct surfaces here even when
# setup_zsh.sh itself exited 0.
if command -v zsh >/dev/null 2>&1; then
  if ! zsh -n "$tmp_home/.config/kaku/zsh/kaku.zsh" 2>"$tmp_dir/zsh_parse.log"; then
    cat "$tmp_dir/zsh_parse.log" >&2
    fail "generated kaku.zsh failed 'zsh -n' parse check"
  fi

  # Execute the generated prompt hook, not a source-text match (#551).
  HOME="$tmp_home" TERM=xterm-256color TERM_PROGRAM=Kaku zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    _kaku_set_ai_user_var() { sent_name="$1"; sent_value="$2"; }
    zle() { zle_action="$1"; }
    _kaku_copy_preexec "dbt run --select example"
    () { return 7; }
    _kaku_ai_precmd
    [[ $_kaku_copy_status == 7 && -n $_kaku_copy_start && -n $_kaku_copy_end ]] || exit 1
    BUFFER=/copy
    _kaku_ai_query_accept_line
    [[ $sent_name == kaku_copy_command && $sent_value == *"dbt run --select example" ]] || exit 2
    [[ -z $BUFFER && $zle_action == redisplay ]] || exit 3
    previous="$sent_value"
    BUFFER=/copy
    _kaku_ai_query_accept_line
    [[ $sent_value != "$previous" && ${sent_value#*$'\''\n'\''} == ${previous#*$'\''\n'\''} ]] || exit 4
    BUFFER="/copy extra"
    _kaku_ai_query_accept_line
    [[ $zle_action == .accept-line ]] || exit 5
  ' || fail "shell /copy metadata or accept-line behavior failed"

  mouse_reset="$(HOME="$tmp_home" TERM=xterm-256color TERM_PROGRAM=Kaku zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    (( ${precmd_functions[(Ie)_kaku_reset_mouse_tracking]} )) || exit 1
    _kaku_reset_mouse_tracking
  ')" || fail "mouse recovery hook is not registered"
  [[ "$mouse_reset" == $'\e[?1000;1002;1003;1004;1005;1006;1007;1016l' ]] \
    || fail "prompt did not reset stale mouse/focus reporting"

  outside_reset="$(HOME="$tmp_home" TERM_PROGRAM=Apple_Terminal zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    unset TMUX
    export KAKU_SESSION=1
    _kaku_reset_mouse_tracking || exit 1
  ')" || fail "mouse hook must not stop later prompt hooks outside Kaku"
  [[ -z "$outside_reset" ]] || fail "mouse hook changed a different terminal"

  starship_stub_dir="$tmp_dir/starship-bin"
  starship_marker="$tmp_dir/starship-initialized"
  mkdir -p "$starship_stub_dir"
  cp /dev/null "$starship_marker"
  cat >"$starship_stub_dir/starship" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == "init" ]]; then
  printf 'print -r -- initialized >> "$KAKU_STARSHIP_TEST_MARKER"\n'
fi
EOF
  chmod +x "$starship_stub_dir/starship"

  HOME="$tmp_home" \
    TERM_PROGRAM="Apple_Terminal" \
    KAKU_STARSHIP_TEST_MARKER="$starship_marker" \
    PATH="$starship_stub_dir:$PATH" \
    zsh -dfc 'add-zsh-hook() { :; }; source "$HOME/.config/kaku/zsh/kaku.zsh"'
  [[ ! -s "$starship_marker" ]] \
    || fail "generated kaku.zsh initialized Starship outside Kaku"

  HOME="$tmp_home" \
    TERM_PROGRAM="Kaku" \
    KAKU_STARSHIP_TEST_MARKER="$starship_marker" \
    PATH="$starship_stub_dir:$PATH" \
    zsh -dfc 'add-zsh-hook() { :; }; source "$HOME/.config/kaku/zsh/kaku.zsh"'
  [[ "$(cat "$starship_marker")" == "initialized" ]] \
    || fail "generated kaku.zsh did not initialize Starship inside Kaku"
else
  echo "warning: zsh not found; skipping kaku.zsh parse check" >&2
fi

echo "setup_zsh update-only smoke test passed"
