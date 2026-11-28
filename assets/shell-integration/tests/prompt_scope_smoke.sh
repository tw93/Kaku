#!/usr/bin/env bash
# Where the Starship prompt and Smart Tab start (#503):
#   inside Kaku                 -> both
#   tmux started from Kaku      -> both (KAKU_SESSION inherited)
#   another terminal            -> neither
#   another terminal + opt-in   -> prompt only
#   leaked marker, no tmux      -> neither, marker dropped
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
source "$SCRIPT_DIR/common.sh"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/kaku-prompt-scope.XXXXXX")"
cleanup() { rm -rf "$tmp_dir"; }
trap cleanup EXIT

fail() { echo "prompt_scope: $*" >&2; exit 1; }

# A starship stub whose `init zsh` output marks that it ran.
mkdir -p "$tmp_dir/bin"
cat >"$tmp_dir/bin/starship" <<'STUB'
#!/usr/bin/env bash
case "${1:-}" in
  init) echo 'typeset -g __KAKU_STARSHIP_RAN=1' ;;
  *) exit 1 ;;
esac
STUB
chmod +x "$tmp_dir/bin/starship"

HOME="$tmp_dir/home"
ZDOTDIR="$HOME"
mkdir -p "$HOME"
vendor_dir="$tmp_dir/vendor"
create_stub_vendor_dir "$vendor_dir"
mkdir -p "$vendor_dir/zsh-syntax-highlighting"

PATH="$tmp_dir/bin:$PATH" HOME="$HOME" ZDOTDIR="$ZDOTDIR" \
  KAKU_INIT_INTERNAL=1 KAKU_SKIP_TOOL_BOOTSTRAP=1 KAKU_SKIP_TERMINFO_BOOTSTRAP=1 \
  KAKU_VENDOR_DIR="$vendor_dir" \
  bash "$REPO_ROOT/assets/shell-integration/setup_zsh.sh" --update-only >/dev/null 2>&1 \
  || fail "setup_zsh.sh --update-only failed"
[[ -f "$HOME/.config/kaku/zsh/kaku.zsh" ]] || fail "kaku.zsh was not generated"

# probe <label> <expected "ran tab marker"> [ENV=VAL ...]
probe() {
  local label="$1" expected="$2"; shift 2
  local out
  out="$(env -i PATH="$tmp_dir/bin:/usr/bin:/bin" HOME="$HOME" ZDOTDIR="$ZDOTDIR" TERM=xterm-256color "$@" \
    zsh -f -c '
source "$HOME/.config/kaku/zsh/kaku.zsh"
print -r -- "${__KAKU_STARSHIP_RAN:-0} ${+functions[_kaku_tab_widget]} ${KAKU_SESSION:-_}"
' 2>&1 | tail -n 1)" || fail "$label: zsh exited non-zero: $out"
  [[ "$out" == "$expected" ]] || fail "$label: expected [$expected] got [$out]"
  echo "prompt_scope: $label ok ($out)" >&2
}

probe "inside Kaku"                 "1 1 1" TERM_PROGRAM=Kaku
probe "tmux started from Kaku"      "1 1 1" TERM_PROGRAM=tmux TMUX=/tmp/tmux-0/default,1,0 KAKU_SESSION=1
probe "tmux started elsewhere"      "0 0 _" TERM_PROGRAM=tmux TMUX=/tmp/tmux-0/default,1,0
probe "other terminal"              "0 0 _" TERM_PROGRAM=vscode
probe "other terminal, opt-in"      "1 0 _" TERM_PROGRAM=vscode KAKU_PROMPT_EVERYWHERE=1
probe "leaked marker, no tmux"      "0 0 _" TERM_PROGRAM=vscode KAKU_SESSION=1

# Powerlevel10k marks its own prompts inside Kaku, so a resize does not
# reflow the instant prompt; a value the user set is kept.
p10k() {
  local label="$1" expected="$2"; shift 2
  local out
  out="$(env -i PATH="$tmp_dir/bin:/usr/bin:/bin" HOME="$HOME" ZDOTDIR="$ZDOTDIR" TERM=xterm-256color "$@" \
    zsh -f -c '
[[ -n "${P10K_PRESET:-}" ]] && typeset -g POWERLEVEL9K_TERM_SHELL_INTEGRATION="$P10K_PRESET"
source "$HOME/.config/kaku/zsh/kaku.zsh"
print -r -- "${POWERLEVEL9K_TERM_SHELL_INTEGRATION:-unset}"
' 2>&1 | tail -n 1)" || fail "$label: zsh exited non-zero: $out"
  [[ "$out" == "$expected" ]] || fail "$label: expected [$expected] got [$out]"
  echo "prompt_scope: $label ok ($out)" >&2
}
p10k "p10k marks inside Kaku"       "true"  TERM_PROGRAM=Kaku
p10k "p10k user value kept"         "false" TERM_PROGRAM=Kaku P10K_PRESET=false
p10k "p10k untouched elsewhere"     "unset" TERM_PROGRAM=vscode

echo "prompt_scope: all cases passed" >&2
