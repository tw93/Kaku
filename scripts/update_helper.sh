#!/bin/bash
set -euo pipefail

TARGET_APP="$1"
NEW_APP="$2"
WORK_DIR="$3"
LOG_FILE="$WORK_DIR/update.log"
# Kaku spawns the helper with stderr closed; keep its own errors in the log.
exec 2>>"$LOG_FILE"

strip_trailing_slashes() {
  local p="$1"
  while [[ "$p" == */ ]]; do
    p="${p%/}"
  done
  printf '%s\n' "$p"
}
TARGET_APP_NORM=$(strip_trailing_slashes "$TARGET_APP")
NEW_APP_NORM=$(strip_trailing_slashes "$NEW_APP")
# Use normalized paths consistently for all later path joins and file operations.
TARGET_APP="$TARGET_APP_NORM"
NEW_APP="$NEW_APP_NORM"
BACKUP_APP="${TARGET_APP}.backup.$(date +%s).$$"
TARGET_GUI="$TARGET_APP/Contents/MacOS/kaku-gui"
TARGET_CLI="$TARGET_APP/Contents/MacOS/kaku"

# Validate that paths end with Kaku.app for safety (allow trailing slashes).
# Final component match mirrors Rust Path::ends_with("Kaku.app") semantics.
# After stripping trailing slashes, the final component must be Kaku.app
if [[ ! "$TARGET_APP_NORM" == */Kaku.app && ! "$TARGET_APP_NORM" == Kaku.app ]]; then
    echo "Error: TARGET_APP must end with Kaku.app" >&2
    exit 1
fi
if [[ ! "$NEW_APP_NORM" == */Kaku.app && ! "$NEW_APP_NORM" == Kaku.app ]]; then
    echo "Error: NEW_APP must end with Kaku.app" >&2
    exit 1
fi

log() {
  printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$1" >>"$LOG_FILE"
}

# The new bundle is staged beside the installed one, so a rename never crosses
# a volume and the swap below is two back-to-back renames.
INCOMING_APP="${TARGET_APP}.incoming.$(date +%s).$$"

# Whatever ends this run (an error, a signal, the end of the script), leave an
# app at TARGET_APP: finish a half-done swap from the staged copy, else put the
# previous app back. SIGKILL is the one exit this cannot cover, which is why the
# window between the two renames is kept to two mv calls (#591).
ensure_app_present() {
  if [[ -d "$TARGET_APP" ]]; then
    return 0
  fi
  if [[ -d "$INCOMING_APP" ]] && /bin/mv "$INCOMING_APP" "$TARGET_APP"; then
    log "finished swap on exit"
    return 0
  fi
  if [[ -d "$BACKUP_APP" ]] && /bin/mv "$BACKUP_APP" "$TARGET_APP"; then
    log "restored previous app"
    return 0
  fi
  log "error: no app left at $TARGET_APP"
}

schedule_job_removal() {
  # A finished one-shot job stays loaded until logout; unload it once this
  # process is gone. AbandonProcessGroup keeps launchd from killing this child.
  if [[ -n "${KAKU_UPDATE_JOB_LABEL:-}" ]]; then
    /usr/bin/nohup /bin/bash -c '/bin/sleep 2; /bin/launchctl remove "$1"' _ "$KAKU_UPDATE_JOB_LABEL" \
      </dev/null >/dev/null 2>&1 &
  fi
}

KAKU_STOPPED=0
INTERRUPTED=0
# One helper at a time per installed app: a second Restart to Update (two GUI
# processes, or a retry while the first job still runs) would otherwise delete
# the first one's staged copy or swap into a half-restored bundle.
LOCK_DIR="${TARGET_APP}.updating"
HOLDS_LOCK=0

acquire_lock() {
  local owner
  if /bin/mkdir "$LOCK_DIR" 2>/dev/null; then
    printf '%s\n' "$$" >"$LOCK_DIR/pid"
    HOLDS_LOCK=1
    return 0
  fi
  owner=$(/bin/cat "$LOCK_DIR/pid" 2>/dev/null || true)
  if [[ -n "$owner" ]] && kill -0 "$owner" 2>/dev/null; then
    return 1
  fi
  # The previous holder died without releasing it.
  /bin/rm -rf "$LOCK_DIR"
  if /bin/mkdir "$LOCK_DIR" 2>/dev/null; then
    printf '%s\n' "$$" >"$LOCK_DIR/pid"
    HOLDS_LOCK=1
    return 0
  fi
  return 1
}

on_exit() {
  local rc=$?
  set +e
  ensure_app_present
  if [[ -d "$INCOMING_APP" ]]; then
    /bin/rm -rf "$INCOMING_APP"
  fi
  # A failure after Kaku was stopped must not leave the user without Kaku
  # running; reopen whichever app is installed now. Not after a signal: that
  # is usually logout or shutdown, and reopening would fight it.
  if [[ "$rc" != 0 && "$KAKU_STOPPED" == 1 && "$INTERRUPTED" == 0 && -d "$TARGET_APP" ]]; then
    log "update failed, reopening $TARGET_APP"
    /usr/bin/open "$TARGET_APP" || true
  fi
  if [[ "$HOLDS_LOCK" == 1 ]]; then
    /bin/rm -rf "$LOCK_DIR"
  fi
  schedule_job_removal
  exit "$rc"
}

read_persisted_managed_shell() {
  local config_base state_file managed_shell
  config_base="${XDG_CONFIG_HOME:-}"
  if [[ -z "$config_base" ]]; then
    if [[ -z "${HOME:-}" ]]; then
      return
    fi
    config_base="$HOME/.config"
  fi
  state_file="$config_base/kaku/state.json"
  if [[ ! -f "$state_file" ]]; then
    return
  fi
  managed_shell="$(/usr/bin/plutil -extract managed_shell raw -expect string -o - -- "$state_file" 2>/dev/null || true)"
  case "$managed_shell" in
    zsh|fish) printf '%s\n' "$managed_shell" ;;
  esac
}

install_kaku_wrapper_fallback() {
  local home_dir shell_candidate wrapper_shell wrapper_path wrapper_dir
  home_dir="${HOME:-}"
  if [[ -z "$home_dir" ]]; then
    return 1
  fi

  shell_candidate="${KAKU_TARGET_SHELL:-}"
  if [[ -z "$shell_candidate" ]]; then
    shell_candidate="$(read_persisted_managed_shell || true)"
  fi
  if [[ -z "$shell_candidate" ]]; then
    shell_candidate="${SHELL:-/bin/zsh}"
  fi
  case "$shell_candidate" in
    *fish|fish)
      wrapper_shell="fish"
      ;;
    *)
      wrapper_shell="zsh"
      ;;
  esac

  wrapper_path="$home_dir/.config/kaku/$wrapper_shell/bin/kaku"
  wrapper_dir="${wrapper_path%/*}"
  /bin/mkdir -p "$wrapper_dir"

  /bin/cat >"$wrapper_path" <<EOF
#!/bin/bash
set -euo pipefail

if [[ -n "\${KAKU_BIN:-}" && -x "\${KAKU_BIN}" ]]; then
  exec "\${KAKU_BIN}" "\$@"
fi

for candidate in \
  "$TARGET_CLI" \
  "/Applications/Kaku.app/Contents/MacOS/kaku" \
  "\${HOME:-}/Applications/Kaku.app/Contents/MacOS/kaku"; do
  if [[ -n "\$candidate" && -x "\$candidate" ]]; then
    exec "\$candidate" "\$@"
  fi
done

  echo "kaku: Kaku.app not found. Expected /Applications/Kaku.app." >&2
  exit 127
EOF

  /bin/chmod 755 "$wrapper_path"
  printf '%s\n' "$wrapper_path"
}

# Run as a one-shot launchd job rather than as Kaku's child. On macOS 27 the
# helper was still killed about a second after Kaku quit, while it was halfway
# through replacing the app, though it already led its own process group: a new
# group does not leave the app's coalition, and the system may reap an app's
# subordinate processes (#591). A launchd job belongs to no app. When the gui
# domain refuses the job (an ssh session, say), keep running inline.
detach_to_launchd() {
  if [[ -n "${KAKU_UPDATE_JOB_LABEL:-}" || "${KAKU_UPDATE_INLINE:-}" == 1 ]]; then
    return 1
  fi
  local label plist name
  label="fun.tw93.kaku.update.$(date +%s).$$"
  plist="$WORK_DIR/$label.plist"
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' \
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
    '<plist version="1.0"><dict><key>ProgramArguments</key><array/><key>EnvironmentVariables</key><dict/></dict></plist>' \
    >"$plist" || return 1
  if ! {
    /usr/bin/plutil -insert Label -string "$label" "$plist" &&
      /usr/bin/plutil -insert RunAtLoad -bool true "$plist" &&
      /usr/bin/plutil -insert LaunchOnlyOnce -bool true "$plist" &&
      /usr/bin/plutil -insert AbandonProcessGroup -bool true "$plist" &&
      /usr/bin/plutil -insert ProgramArguments.0 -string /bin/bash "$plist" &&
      /usr/bin/plutil -insert ProgramArguments.1 -string "$0" "$plist" &&
      /usr/bin/plutil -insert ProgramArguments.2 -string "$TARGET_APP" "$plist" &&
      /usr/bin/plutil -insert ProgramArguments.3 -string "$NEW_APP" "$plist" &&
      /usr/bin/plutil -insert ProgramArguments.4 -string "$WORK_DIR" "$plist" &&
      /usr/bin/plutil -insert EnvironmentVariables.KAKU_UPDATE_JOB_LABEL -string "$label" "$plist"
  } >/dev/null 2>&1; then
    /bin/rm -f "$plist"
    log "could not write the launchd job, applying inline"
    return 1
  fi
  # The job gets launchd's bare environment otherwise; keep what the helper
  # and `kaku init` read from the caller.
  for name in $(compgen -e); do
    case "$name" in
      HOME | USER | LOGNAME | SHELL | ZDOTDIR | PATH | TMPDIR | LANG | LC_* | XDG_* | KAKU_*)
        [[ "$name" == KAKU_UPDATE_JOB_LABEL ]] && continue
        /usr/bin/plutil -insert "EnvironmentVariables.$name" -string "${!name}" "$plist" >/dev/null 2>&1 || true
        ;;
    esac
  done
  if /bin/launchctl bootstrap "gui/$(/usr/bin/id -u)" "$plist" 2>>"$LOG_FILE"; then
    /bin/rm -f "$plist"
    log "handed off to launchd job $label"
    return 0
  fi
  /bin/rm -f "$plist"
  log "launchd job refused, applying inline"
  return 1
}

if detach_to_launchd; then
  exit 0
fi

trap on_exit EXIT
trap 'INTERRUPTED=1; log "interrupted by signal"; exit 1' TERM HUP INT

log "start apply update"
if ! acquire_lock; then
  log "another update is already running, leaving it to finish"
  exit 0
fi

# Stage the new bundle while Kaku still runs: a failed copy leaves Kaku running
# and untouched, and the installed app is only missing between two renames
# instead of for a whole copy (#591).
for stale in "${TARGET_APP}".incoming.*; do
  if [[ -d "$stale" && "$stale" != "$INCOMING_APP" ]]; then
    /bin/rm -rf "$stale"
  fi
done
log "stage new app"
if ! /usr/bin/ditto "$NEW_APP" "$INCOMING_APP"; then
  log "error: failed to stage new app"
  exit 1
fi
/usr/bin/xattr -cr "$INCOMING_APP" >/dev/null 2>&1 || true

# pgrep/pkill -f treats the pattern as a regex, but TARGET_GUI/TARGET_CLI may contain
# regex metacharacters. Match against the full command line via ps and shell pattern
# literals instead. Use ps -axww so long command lines are not truncated.
collect_kaku_pids() {
  ps -axww -o pid= -o args= | while read -r pid args; do
    [[ -z "$pid" ]] && continue
    [[ "$pid" == "$$" ]] && continue
    case "$args" in
      *"$TARGET_GUI"* | *"$TARGET_CLI"* ) printf '%s\n' "$pid" ;;
    esac
  done | sort -u
}

KAKU_STOPPED=1
KAKU_PIDS=""
for _ in $(seq 1 20); do
  KAKU_PIDS=$(collect_kaku_pids | tr '\n' ' ')
  if [[ -z "$KAKU_PIDS" ]]; then
    break
  fi
  for pid in $KAKU_PIDS; do
    if ! kill -TERM "$pid" 2>/dev/null; then
      log "failed to send TERM to pid $pid"
    fi
  done
  sleep 1
done
# Final force-kill if any remain
KAKU_PIDS=$(collect_kaku_pids | tr '\n' ' ')
if [[ -n "$KAKU_PIDS" ]]; then
  for pid in $KAKU_PIDS; do
    if ! kill -KILL "$pid" 2>/dev/null; then
      log "failed to send KILL to pid $pid"
    fi
  done
fi

log "swap in new app"
if [[ -d "$TARGET_APP" ]]; then
  /bin/mv "$TARGET_APP" "$BACKUP_APP"
fi
/bin/mv "$INCOMING_APP" "$TARGET_APP"

if [[ -d "$BACKUP_APP" ]]; then
  /bin/rm -rf "$BACKUP_APP" || true
fi

log "refresh shell integration"
if "$TARGET_CLI" init --update-only >>"$LOG_FILE" 2>&1; then
  log "shell integration refreshed"
else
  log "warning: failed to refresh shell integration via kaku init"
  if fallback_wrapper_path="$(install_kaku_wrapper_fallback)"; then
    log "installed fallback kaku wrapper at ${fallback_wrapper_path:-~/.config/kaku/<unknown>/bin/kaku}"
  else
    log "warning: failed to install fallback kaku wrapper"
  fi
fi

# Write update completed marker with new version
NEW_VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$TARGET_APP/Contents/Info.plist" 2>/dev/null || echo "")
if [[ -n "$NEW_VERSION" ]]; then
  DATA_DIR="${XDG_DATA_HOME:-$HOME/Library/Application Support}/kaku"
  /bin/mkdir -p "$DATA_DIR" 2>/dev/null
  printf '%s\n' "$NEW_VERSION" > "$DATA_DIR/update_completed"
  log "wrote update_completed marker: $NEW_VERSION"
fi

log "relaunch app"
sleep 1

# Verify the new app exists before attempting to open
if [[ ! -d "$TARGET_APP" ]]; then
  log "error: TARGET_APP does not exist after copy: $TARGET_APP"
  exit 1
fi

# Try multiple methods to relaunch the app
log "attempting to relaunch: $TARGET_APP"

# Method 1: open command with path (most reliable)
if /usr/bin/open "$TARGET_APP" 2>>"$LOG_FILE"; then
  log "relaunch via open path succeeded"
else
  log "open path failed (exit code: $?), trying open -a"
  sleep 1
  # Method 2: open by app name
  if /usr/bin/open -a Kaku 2>>"$LOG_FILE"; then
    log "relaunch via open -a succeeded"
  else
    log "open -a failed (exit code: $?), trying osascript"
    sleep 1
    # Method 3: AppleScript as last resort
    /usr/bin/osascript -e 'tell application "Kaku" to activate' 2>>"$LOG_FILE" || log "osascript also failed"
  fi
fi

log "done"
/bin/rm -f "$0" >/dev/null 2>&1 || true
/bin/rm -rf "$WORK_DIR" >/dev/null 2>&1 || true
