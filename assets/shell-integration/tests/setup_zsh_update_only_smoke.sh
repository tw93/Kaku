#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SHELL_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$SHELL_DIR/../.." && pwd)"

# A shell inside Kaku exports KAKU_CONFIG_FILE, which would point the theme
# lookup at the real config instead of the scratch HOME's kaku.lua.
unset KAKU_CONFIG_FILE XDG_CONFIG_HOME

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/kaku-setup-zsh-smoke.XXXXXX")"
trap 'chmod -R u+w "$tmp_dir" 2>/dev/null; rm -rf "$tmp_dir"' EXIT

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

# Bash expands backticks and $(...) in an unquoted heredoc while generating
# kaku.zsh, so an unescaped one runs as a command and leaves its error here
# (#450: `[comment]` in a comment). zsh -n below cannot see it, since the
# substitution only empties part of a comment.
if grep -Fq ": command not found" "$error_log"; then
  cat "$error_log" >&2
  fail "setup_zsh.sh ran a command while generating kaku.zsh"
fi

[[ -f "$tmp_home/.config/starship.toml" ]] || fail "starship.toml was not initialized"
python3 - "$tmp_home/.config/starship.toml" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as config_file:
    config = tomllib.load(config_file)
for section, fields in {
    "git_branch": ("symbol",),
    "git_status": ("conflicted", "staged", "modified", "stashed"),
}.items():
    for field in fields:
        value = config.get(section, {}).get(field)
        if not isinstance(value, str) or not value.strip():
            sys.exit(f"FAIL: starship.toml has a missing or blank {section}.{field}")
# The character module deliberately supplies only the prompt/input separator.
for field in ("success_symbol", "error_symbol"):
    if config.get("character", {}).get(field) != " ":
        sys.exit(f"FAIL: starship.toml changed character.{field}")
PY
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
# zsh. A corrupted heredoc or a stray top-level construct surfaces here even
# when setup_zsh.sh itself exited 0.
if command -v zsh >/dev/null 2>&1; then
  if ! zsh -n "$tmp_home/.config/kaku/zsh/kaku.zsh" 2>"$tmp_dir/zsh_parse.log"; then
    cat "$tmp_dir/zsh_parse.log" >&2
    fail "generated kaku.zsh failed 'zsh -n' parse check"
  fi

  repeated_queries="$(HOME="$tmp_home" TERM=xterm-256color TERM_PROGRAM=Kaku zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    typeset -g query_seen="" query_deliveries=0
    _kaku_set_ai_user_var() {
      [[ "$1" == "kaku_ai_query" ]] || return 1
      if [[ "$query_seen" != "$2" && -n "$2" ]]; then
        (( ++query_deliveries ))
      fi
      query_seen="$2"
      return 0
    }
    zle() { :; }
    for round in 1 2; do
      BUFFER="# list files"
      _kaku_ai_waiting=0
      _kaku_ai_query_accept_line
    done
    print -r -- "$query_deliveries"
  ')"
  [[ "$repeated_queries" == "2" ]] || fail "identical AI queries did not emit two changes"

  # Execute the generated prompt hook, not a source-text match (#551).
  mouse_reset="$(HOME="$tmp_home" TERM=xterm-256color TERM_PROGRAM=Kaku zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    (( ${precmd_functions[(Ie)_kaku_reset_mouse_tracking]} )) || exit 1
    (( ${preexec_functions[(Ie)_kaku_mouse_tracking_preexec]} )) || exit 1
    # Initial and empty prompts must not write during instant-prompt capture.
    [[ -z "$(_kaku_reset_mouse_tracking)" ]] || exit 1
    _kaku_mouse_tracking_preexec
    _kaku_reset_mouse_tracking
    # No new command: no redundant reset on another prompt.
    [[ -z "$(_kaku_reset_mouse_tracking)" ]] || exit 1
  ')" || fail "mouse recovery hook is not registered"
  [[ "$mouse_reset" == $'\e[?1000;1002;1003;1004;1005;1006;1007;1016l\e[?25h' ]] \
    || fail "prompt did not reset stale mouse/focus reporting and show the cursor"

  outside_reset="$(HOME="$tmp_home" TERM_PROGRAM=Apple_Terminal zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    unset TMUX
    export KAKU_SESSION=1
    _kaku_mouse_tracking_preexec
    _kaku_reset_mouse_tracking || exit 1
  ')" || fail "mouse hook must not stop later prompt hooks outside Kaku"
  [[ -z "$outside_reset" ]] || fail "mouse hook changed a different terminal"

  # OSC 133 prompt marks: session restore cuts history at the marked prompt.
  semantic_marks="$(HOME="$tmp_home" TERM=xterm-256color TERM_PROGRAM=Kaku zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    (( ${precmd_functions[(Ie)_kaku_semantic_precmd]} )) || exit 1
    (( ${preexec_functions[(Ie)_kaku_semantic_preexec]} )) || exit 1
    # Powerlevel10k still captures stdout at its first precmd (#561).
    __p9k_instant_prompt_active=1
    [[ -z "$(_kaku_semantic_precmd)" ]] || exit 1
    unset __p9k_instant_prompt_active
    _kaku_semantic_precmd
    _kaku_semantic_preexec
  ')" || fail "semantic prompt hooks are not registered or wrote during instant prompt"
  [[ "$semantic_marks" == $'\e]133;P;k=i\a\e]133;C\a' ]] \
    || fail "prompt and command did not send OSC 133 marks"

  outside_marks="$(HOME="$tmp_home" TERM_PROGRAM=Apple_Terminal zsh -dfc '
    add-zsh-hook() { :; }
    source "$HOME/.config/kaku/zsh/kaku.zsh" >/dev/null
    unset TMUX
    _kaku_semantic_precmd || exit 1
    _kaku_semantic_preexec || exit 1
  ')" || fail "semantic prompt hooks must not stop later hooks outside Kaku"
  [[ -z "$outside_marks" ]] || fail "semantic prompt marks reached a different terminal"

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

# #573: an untouched V0.22.0 starship.toml (icons stripped) is Kaku's own
# copy and gets the restored icons; an edited one is the user's and stays.
run_update_only() {
  HOME="$1" \
    KAKU_INIT_INTERNAL=1 \
    KAKU_SKIP_TOOL_BOOTSTRAP=1 \
    KAKU_SKIP_TERMINFO_BOOTSTRAP=1 \
    KAKU_VENDOR_DIR="$tmp_vendor" \
    bash "$SHELL_DIR/setup_zsh.sh" --update-only >"$output_log" 2>"$error_log" \
    || { cat "$output_log" "$error_log" >&2; fail "setup_zsh.sh --update-only failed"; }
}
stale_fixture="$REPO_ROOT/tests/fixtures/starship-v0.22.0.toml"
for edited in false true; do
  repair_home="$tmp_dir/repair-$edited"
  mkdir -p "$repair_home/.config"
  cp "$stale_fixture" "$repair_home/.config/starship.toml"
  [[ "$edited" == true ]] && printf '# mine\n' >>"$repair_home/.config/starship.toml"
  before="$(shasum -a 256 <"$repair_home/.config/starship.toml")"
  run_update_only "$repair_home"
  if [[ "$edited" == true ]]; then
    [[ "$(shasum -a 256 <"$repair_home/.config/starship.toml")" == "$before" ]] \
      || fail "an edited starship.toml was replaced"
  else
    cmp -s "$tmp_vendor/starship.toml" "$repair_home/.config/starship.toml" \
      || fail "the V0.22.0 starship.toml with blank icons was not restored"
  fi
done

# Dotfile managers link ~/.zshrc, ~/.tmux.conf and the yazi theme into a repo;
# rewriting them must write through the link, not replace it.
link_failures=0
expect() {
  local message="$1"
  shift
  if ! "$@"; then
    echo "FAIL: $message" >&2
    link_failures=$((link_failures + 1))
  fi
}
file_mode() { stat -f '%Lp' "$1"; }

tmux_stub_dir="$tmp_dir/tmux-bin"
mkdir -p "$tmux_stub_dir"
printf '#!/usr/bin/env bash\nexit 0\n' >"$tmux_stub_dir/tmux"
chmod +x "$tmux_stub_dir/tmux"

run_linked_update_only() {
  HOME="$1" \
    ZDOTDIR="$1" \
    PATH="$tmux_stub_dir:$PATH" \
    KAKU_INIT_INTERNAL=1 \
    KAKU_SKIP_TOOL_BOOTSTRAP=1 \
    KAKU_SKIP_TERMINFO_BOOTSTRAP=1 \
    KAKU_VENDOR_DIR="$tmp_vendor" \
    bash "$SHELL_DIR/setup_zsh.sh" --update-only </dev/null >"$output_log" 2>"$error_log"
}

kaku_source_line='[[ -f "$HOME/.config/kaku/zsh/kaku.zsh" ]] && source "$HOME/.config/kaku/zsh/kaku.zsh" # Kaku Shell Integration'
kaku_tmux_line='source-file "$HOME/.config/kaku/tmux/kaku.tmux.conf" # Kaku tmux Integration'
stale_zshrc='export FOO=1
[[ ":$PATH:" != *":$HOME/.config/kaku/zsh/bin:"* ]] && export PATH="$HOME/.config/kaku/zsh/bin:$PATH"
[[ -f "$HOME/.config/kaku/zsh/kaku.zsh" ]] && source "$HOME/.config/kaku/zsh/kaku.zsh"
'

linked_home="$tmp_dir/linked-home"
dotfiles="$tmp_dir/dotfiles"
mkdir -p "$linked_home/.config/yazi" "$linked_home/.config/kaku" "$dotfiles"
printf "config.color_scheme = 'Kaku Light'\n" >"$linked_home/.config/kaku/kaku.lua"
printf '%s' "$stale_zshrc" >"$dotfiles/zshrc"
printf 'set -g mouse on\nsource-file ~/.config/kaku/tmux/kaku.tmux.conf\n' >"$dotfiles/tmux.conf"
cat >"$dotfiles/theme.toml" <<'EOF'
[mgr]
border_symbol = "x"

# ===== Kaku Yazi Flavor (managed) =====
[flavor]
dark = "old-flavor"
light = "old-flavor"
# ===== End Kaku Yazi Flavor (managed) =====
EOF
chmod 0644 "$dotfiles/zshrc" "$dotfiles/tmux.conf" "$dotfiles/theme.toml"
ln -s "$dotfiles/zshrc" "$linked_home/.zshrc"
ln -s "$dotfiles/tmux.conf" "$linked_home/.tmux.conf"
ln -s "$dotfiles/theme.toml" "$linked_home/.config/yazi/theme.toml"

run_linked_update_only "$linked_home" \
  || { cat "$output_log" "$error_log" >&2; fail "setup_zsh.sh --update-only failed with linked dotfiles"; }

# each rewritten dotfile is still a symlink
expect ".zshrc is no longer a symlink" test -L "$linked_home/.zshrc"
expect ".tmux.conf is no longer a symlink" test -L "$linked_home/.tmux.conf"
expect "yazi theme.toml is no longer a symlink" test -L "$linked_home/.config/yazi/theme.toml"

# each link target holds the updated content with its mode unchanged
expect "linked .zshrc target lacks the current source line" grep -Fqx "$kaku_source_line" "$dotfiles/zshrc"
expect "linked .zshrc target lost the user's own line" grep -Fqx 'export FOO=1' "$dotfiles/zshrc"
expect "linked .tmux.conf target lacks the current source line" grep -Fqx "$kaku_tmux_line" "$dotfiles/tmux.conf"
expect "linked .tmux.conf target lost the user's own line" grep -Fqx 'set -g mouse on' "$dotfiles/tmux.conf"
expect "linked theme.toml target lacks the current flavor" grep -Fqx 'dark = "kaku-light"' "$dotfiles/theme.toml"
expect "linked theme.toml target still holds the old flavor" bash -c '! grep -Fq old-flavor "$1"' _ "$dotfiles/theme.toml"
expect "linked theme.toml target lost the user's own section" grep -Fqx 'border_symbol = "x"' "$dotfiles/theme.toml"
for target in zshrc tmux.conf theme.toml; do
  expect "linked $target target mode changed from 644 to $(file_mode "$dotfiles/$target")" \
    test "$(file_mode "$dotfiles/$target")" = 644
done

# a .zshrc linked to a read-only file is left alone and the run completes
readonly_home="$tmp_dir/readonly-home"
readonly_dotfiles="$tmp_dir/readonly-dotfiles"
mkdir -p "$readonly_home" "$readonly_dotfiles"
printf '%s' "$stale_zshrc" >"$readonly_dotfiles/zshrc"
chmod 0444 "$readonly_dotfiles/zshrc"
chmod 0555 "$readonly_dotfiles"
ln -s "$readonly_dotfiles/zshrc" "$readonly_home/.zshrc"
readonly_before="$(shasum -a 256 <"$readonly_dotfiles/zshrc")"
readonly_status=0
run_linked_update_only "$readonly_home" || readonly_status=$?
expect "setup_zsh.sh --update-only exited $readonly_status with a read-only linked .zshrc" \
  test "$readonly_status" -eq 0
expect "read-only linked .zshrc is no longer a symlink" test -L "$readonly_home/.zshrc"
expect "read-only linked .zshrc points somewhere else" \
  test "$(readlink "$readonly_home/.zshrc" 2>/dev/null)" = "$readonly_dotfiles/zshrc"
expect "read-only .zshrc target content changed" \
  test "$(shasum -a 256 <"$readonly_dotfiles/zshrc")" = "$readonly_before"
expect "read-only .zshrc target mode changed" test "$(file_mode "$readonly_dotfiles/zshrc")" = 444

# A regular .zshrc is replaced by rename, so a reader never sees it half written.
regular_home="$tmp_dir/regular-home"
mkdir -p "$regular_home"
printf '%s' "$stale_zshrc" >"$regular_home/.zshrc"
regular_inode_before="$(stat -f %i "$regular_home/.zshrc")"
run_linked_update_only "$regular_home" \
  || { cat "$output_log" "$error_log" >&2; fail "setup_zsh.sh --update-only failed with a regular .zshrc"; }
# a regular .zshrc is replaced by rename: still a regular file, new inode
expect "regular .zshrc became a symlink" test ! -L "$regular_home/.zshrc"
expect "regular .zshrc is no longer a regular file" test -f "$regular_home/.zshrc"
regular_inode_after="$(stat -f %i "$regular_home/.zshrc" 2>/dev/null || echo missing)"
expect "regular .zshrc was truncated in place (inode $regular_inode_before kept)" \
  test "$regular_inode_after" != "$regular_inode_before"
expect "regular .zshrc lacks the current source line" grep -Fqx "$kaku_source_line" "$regular_home/.zshrc"
expect "regular .zshrc lost the user's own line" grep -Fqx 'export FOO=1' "$regular_home/.zshrc"

# The installers and the yazi wrappers they generate share one write_through.
write_through_body() {
  awk -v nth="$2" '
    /^write_through\(\) \{$/ { seen++; if (seen == nth) grab = 1 }
    grab { print }
    grab && /^}$/ { exit }
  ' "$1"
}
reference_body="$(write_through_body "$SHELL_DIR/setup_zsh.sh" 1)"
expect "setup_zsh.sh has no write_through definition" test -n "$reference_body"
for site in "setup_zsh.sh 2" "setup_fish.sh 1" "setup_fish.sh 2"; do
  read -r site_file site_nth <<<"$site"
  expect "write_through #$site_nth in $site_file differs from setup_zsh.sh's" \
    test "$(write_through_body "$SHELL_DIR/$site_file" "$site_nth")" = "$reference_body"
done

# Exercise all four real helpers, including the generated wrapper definitions.
# A file-size limit makes the OS reject a partial write without filling the disk.
python3 - "$SHELL_DIR" <<'PYTEST'
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import unittest

shell_dir = Path(sys.argv.pop())

class WriteThroughTests(unittest.TestCase):
    pass

def probe(helper, shape):
    with tempfile.TemporaryDirectory(prefix="kaku-write-through-") as folder:
        root = Path(folder)
        target_dir = root / "dotfiles with spaces"
        target_dir.mkdir()
        target = target_dir / "config"
        original = b"keep original user config\n"
        target.write_bytes(original)
        target.chmod(0o640)
        original_inode = target.stat().st_ino
        original_owner = (target.stat().st_uid, target.stat().st_gid)
        source = root / "replacement"
        replacement = b"new config\n" if shape != "write_failure" else b"x" * 65536
        source.write_bytes(replacement)
        link = root / "config-link"
        if shape == "relative":
            link.symlink_to("dotfiles with spaces/config")
        elif shape == "chained":
            intermediate = root / "middle"
            intermediate.symlink_to("dotfiles with spaces/config")
            link.symlink_to("middle")
        elif shape == "dangling":
            target.unlink()
            link.symlink_to(target)
        elif shape == "cycle":
            link.symlink_to("middle")
            (root / "middle").symlink_to("config-link")
        else:
            link.symlink_to(target)
        link_text = os.readlink(link)
        command = "set -euo pipefail\n" + helper + "\n"
        if shape == "write_failure":
            command += "ulimit -f 1\n"
        command += 'write_through "$1" "$2"\n'
        result = subprocess.run(
            ["/bin/bash", "-c", command, "probe", str(source), str(link)],
            capture_output=True, timeout=5,
        )
        assert link.is_symlink() and os.readlink(link) == link_text, "symlink changed"
        if shape in ("write_failure", "cycle"):
            assert result.returncode != 0, "failed write reported success"
            assert target.read_bytes() == original, "failed write changed original content"
            assert target.stat().st_ino == original_inode, "failed write replaced original file"
            assert source.read_bytes() == replacement, "failed write discarded replacement"
        else:
            assert result.returncode == 0, result.stderr.decode()
            assert target.read_bytes() == replacement, "target was not updated"
            assert not source.exists(), "successful write left its source behind"
            if shape != "dangling":
                assert target.stat().st_ino != original_inode, "target was truncated in place"
        if shape != "dangling":
            assert stat.S_IMODE(target.stat().st_mode) == 0o640, "target mode changed"
            assert (target.stat().st_uid, target.stat().st_gid) == original_owner, "target owner changed"
        assert sorted(p.name for p in target_dir.iterdir()) == ["config"], "temporary file leaked"

for script in ("setup_zsh.sh", "setup_fish.sh"):
    helpers = re.findall(r"^write_through\(\) \{\n.*?^\}", (shell_dir / script).read_text(), re.M | re.S)
    assert len(helpers) == 2, f"expected two helper definitions in {script}"
    for index, helper in enumerate(helpers):
        for shape in ("absolute", "relative", "chained", "dangling", "cycle", "write_failure"):
            def test(self, helper=helper, shape=shape):
                probe(helper, shape)
            setattr(WriteThroughTests, f"test_{script.replace('.', '_')}_{index}_{shape}", test)

unittest.main()
PYTEST

if [[ "$link_failures" != 0 ]]; then
  cat "$output_log" "$error_log" >&2
  fail "$link_failures linked dotfile check(s) failed"
fi

echo "setup_zsh update-only smoke test passed"
