use crate::domain::DomainId;
use crate::pane::{
    CachePolicy, CloseReason, ForEachPaneLogicalLine, LogicalLine, Pane, PaneId, PaneReader,
    Pattern, SearchResult, WithPaneLines,
};
use crate::renderable::*;
use crate::tmux::{TmuxDomain, TmuxDomainState};
use crate::{Domain, Mux, MuxNotification};
use anyhow::Error;
use async_trait::async_trait;
use config::keyassignment::{PaneEncoding, ScrollbackEraseMode};
use config::{configuration, ExitBehavior, ExitBehaviorMessaging};
use fancy_regex::Regex;
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use procinfo::LocalProcessInfo;
use rangeset::RangeSet;
use smol::channel::{bounded, Receiver, TryRecvError};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::io::{Result as IoResult, Write};
use std::ops::Range;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::escape::csi::{Sgr, CSI};
use termwiz::escape::{Action, DeviceControlMode};
use termwiz::input::KeyboardEncoding;
use termwiz::surface::{Line, SequenceNo};
use url::Url;
use wezterm_dynamic::Value;
use wezterm_term::color::ColorPalette;
use wezterm_term::{
    Alert, AlertHandler, Clipboard, DownloadHandler, KeyCode, KeyModifiers, MouseEvent, Progress,
    SemanticZone, StableRowIndex, Terminal, TerminalConfiguration, TerminalSize,
};

const PROC_INFO_CACHE_TTL: Duration = Duration::from_millis(300);

#[derive(Debug)]
enum ProcessState {
    Running {
        child_waiter: Receiver<IoResult<ExitStatus>>,
        pid: Option<u32>,
        signaller: Box<dyn ChildKiller + Sync>,
        // Whether we've explicitly killed the child
        killed: bool,
    },
    DeadPendingClose {
        killed: bool,
    },
    Dead,
}

struct CachedProcInfo {
    root: LocalProcessInfo,
    updated: Instant,
    foreground: LocalProcessInfo,
}

/// Cache cwd lookups while the tty's foreground group and executable are
/// unchanged. A group switch, exec or leader exit must refresh immediately
/// so the next input uses the current foreground program.
#[cfg(unix)]
#[derive(Clone)]
struct CachedLeaderInfo {
    updated: Instant,
    fd: std::os::fd::RawFd,
    pid: u32,
    path: Option<std::path::PathBuf>,
    current_working_dir: Option<std::path::PathBuf>,
    updating: bool,
}

/// Cache of `with_root_pid(process_group_leader_pid)` to avoid repeating the
/// full system `proc_listallpids` + per-pid `proc_pidinfo` scan on every paint.
/// `forces_opaque_kaku_tui_background` (render hot path) and the tab bar both
/// query this each frame; without a cache, multiple panes streaming output from
/// TUI agents (claude code, codex) trigger N x ~1000 syscalls per frame.
#[cfg(unix)]
#[derive(Clone)]
struct CachedForegroundProc {
    updated: Instant,
    pid: u32,
    info: LocalProcessInfo,
}

#[cfg(unix)]
impl CachedLeaderInfo {
    fn new(fd: Option<std::os::fd::RawFd>) -> Self {
        let mut me = Self {
            updated: Instant::now(),
            fd: fd.unwrap_or(-1),
            pid: 0,
            path: None,
            current_working_dir: None,
            updating: false,
        };
        me.update();
        me
    }

    fn can_update(&self) -> bool {
        self.fd != -1 && !self.updating
    }

    fn update(&mut self) {
        self.pid = unsafe { libc::tcgetpgrp(self.fd) } as u32;
        if self.pid > 0 {
            self.path = LocalProcessInfo::executable_path(self.pid);
            self.current_working_dir = LocalProcessInfo::current_working_dir(self.pid);
        } else {
            self.path.take();
            self.current_working_dir.take();
        }
        self.updated = Instant::now();
        self.updating = false;
    }

    fn expired(&self) -> bool {
        self.updated.elapsed() > PROC_INFO_CACHE_TTL
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LocalPaneConnectionState {
    Connecting,
    Connected,
}

pub struct LocalPane {
    pane_id: PaneId,
    terminal: Mutex<Terminal>,
    process: Mutex<ProcessState>,
    pty: Mutex<Box<dyn MasterPty>>,
    writer: Mutex<Box<dyn Write + Send>>,
    domain_id: DomainId,
    tmux_domain: Mutex<Option<Arc<TmuxDomainState>>>,
    proc_list: Mutex<Option<CachedProcInfo>>,
    #[cfg(unix)]
    leader: Arc<Mutex<Option<CachedLeaderInfo>>>,
    #[cfg(unix)]
    foreground_proc: Mutex<Option<CachedForegroundProc>>,
    encoding: Arc<AtomicU8>,
    command_description: String,
}

#[async_trait(?Send)]
impl Pane for LocalPane {
    fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    fn get_metadata(&self) -> Value {
        #[allow(unused_mut)]
        let mut map: BTreeMap<Value, Value> = BTreeMap::new();

        #[cfg(unix)]
        if let Some(tio) = self.pty.lock().get_termios() {
            use nix::sys::termios::LocalFlags;
            // Detect whether we might be in password input mode.
            // If local echo is disabled and canonical input mode
            // is enabled, then we assume that we're in some kind
            // of password-entry mode.
            let pw_input = !tio.local_flags.contains(LocalFlags::ECHO)
                && tio.local_flags.contains(LocalFlags::ICANON);
            map.insert(
                Value::String("password_input".to_string()),
                Value::Bool(pw_input),
            );
        }

        Value::Object(map.into())
    }

    fn get_cursor_position(&self) -> StableCursorPosition {
        let mut cursor = terminal_get_cursor_position(&mut self.terminal.lock());
        if self.tmux_domain.lock().is_some() {
            cursor.visibility = termwiz::surface::CursorVisibility::Hidden;
        }
        cursor
    }

    fn get_keyboard_encoding(&self) -> KeyboardEncoding {
        if self.tmux_domain.lock().is_some() {
            KeyboardEncoding::Xterm
        } else {
            self.terminal.lock().get_keyboard_encoding()
        }
    }

    fn get_encoding(&self) -> PaneEncoding {
        PaneEncoding::from_u8(self.encoding.load(Ordering::Relaxed))
    }

    fn set_encoding(&self, encoding: PaneEncoding) {
        self.encoding.store(encoding.to_u8(), Ordering::Relaxed);
    }

    fn get_current_seqno(&self) -> SequenceNo {
        self.terminal.lock().current_seqno()
    }

    fn get_visible_seqno(&self) -> Option<SequenceNo> {
        Some(self.terminal.lock().visible_seqno())
    }

    fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        terminal_get_dirty_lines(&mut self.terminal.lock(), lines, seqno)
    }

    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        terminal_for_each_logical_line_in_stable_range_mut(
            &mut self.terminal.lock(),
            lines,
            for_line,
        );
    }

    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines) {
        terminal_with_lines_mut(&mut self.terminal.lock(), lines, with_lines)
    }

    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        crate::pane::impl_get_lines_via_with_lines(self, lines)
    }

    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        crate::pane::impl_get_logical_lines_via_get_lines(self, lines)
    }

    fn get_dimensions(&self) -> RenderableDimensions {
        terminal_get_dimensions(&mut self.terminal.lock())
    }

    fn copy_user_vars(&self) -> HashMap<String, String> {
        self.terminal.lock().user_vars().clone()
    }

    fn exit_behavior(&self) -> Option<ExitBehavior> {
        // If we are ssh, and we've not yet fully connected,
        // then override exit_behavior so that we can show
        // connection issues
        let mut pty = self.pty.lock();
        let is_ssh_connecting = pty
            .downcast_mut::<crate::ssh::WrappedSshPty>()
            .map(|s| s.is_connecting())
            .unwrap_or(false);
        let is_failed_spawn = pty.is::<crate::domain::FailedSpawnPty>();

        if is_ssh_connecting || is_failed_spawn {
            Some(ExitBehavior::CloseOnCleanExit)
        } else {
            None
        }
    }

    fn kill(&self) {
        let mut proc = self.process.lock();
        log::debug!(
            "killing process in pane {}, state is {:?}",
            self.pane_id,
            proc
        );
        match &mut *proc {
            ProcessState::Running {
                signaller, killed, ..
            } => {
                let _ = signaller.kill();
                *killed = true;
            }
            ProcessState::DeadPendingClose { killed } => {
                *killed = true;
            }
            _ => {}
        }
    }

    fn is_dead(&self) -> bool {
        let mut proc = self.process.lock();

        const EXIT_BEHAVIOR: &str = "This message is shown because \
            \x1b]8;;https://github.com/tw93/Kaku/\
            config/lua/config/exit_behavior.html\
            \x1b\\exit_behavior\x1b]8;;\x1b\\";

        let mut terse = String::new();
        let mut brief = String::new();
        let mut trailer = String::new();
        let cmd = &self.command_description;

        match &mut *proc {
            ProcessState::Running {
                child_waiter,
                killed,
                ..
            } => {
                let status = match child_waiter.try_recv() {
                    Ok(Ok(s)) => Some(s),
                    Err(TryRecvError::Empty) => None,
                    _ => Some(ExitStatus::with_exit_code(1)),
                };

                if let Some(status) = status {
                    let success = match status.success() {
                        true => true,
                        false => configuration()
                            .clean_exit_codes
                            .contains(&status.exit_code()),
                    };

                    match (
                        self.exit_behavior()
                            .unwrap_or_else(|| configuration().exit_behavior),
                        success,
                        killed,
                    ) {
                        (ExitBehavior::Close, _, _) => *proc = ProcessState::Dead,
                        (ExitBehavior::CloseOnCleanExit, false, _) => {
                            brief = format!("Process {cmd} didn't exit cleanly");
                            terse = format!("{status}.");
                            trailer = format!("{EXIT_BEHAVIOR}=\"CloseOnCleanExit\"");

                            *proc = ProcessState::DeadPendingClose { killed: false }
                        }
                        (ExitBehavior::CloseOnCleanExit, ..) => *proc = ProcessState::Dead,
                        (ExitBehavior::Hold, success, false) => {
                            trailer = format!("{EXIT_BEHAVIOR}=\"Hold\"");

                            if success {
                                brief = format!("Process {cmd} completed.");
                                terse = "done".to_string();
                            } else {
                                brief = format!("Process {cmd} didn't exit cleanly");
                                terse = format!("{status}");
                            }
                            *proc = ProcessState::DeadPendingClose { killed: false }
                        }
                        (ExitBehavior::Hold, _, true) => *proc = ProcessState::Dead,
                    }
                    log::debug!("child terminated, new state is {:?}", proc);
                }
            }
            ProcessState::DeadPendingClose { killed } => {
                if *killed {
                    *proc = ProcessState::Dead;
                    log::debug!("child state -> {:?}", proc);
                }
            }
            ProcessState::Dead => {}
        }

        let mut notify = None;
        if !terse.is_empty() {
            match configuration().exit_behavior_messaging {
                ExitBehaviorMessaging::Verbose => {
                    if terse == "done" {
                        notify = Some(format!("\r\n{brief}\r\n{trailer}"));
                    } else {
                        notify = Some(format!("\r\n{brief}\r\n{terse}\r\n{trailer}"));
                    }
                }
                ExitBehaviorMessaging::Brief => {
                    if terse == "done" {
                        notify = Some(format!("\r\n{brief}"));
                    } else {
                        notify = Some(format!("\r\n{brief}\r\n{terse}"));
                    }
                }
                ExitBehaviorMessaging::Terse => {
                    notify = Some(format!("\r\n[{terse}]"));
                }
                ExitBehaviorMessaging::None => {}
            }
        }

        if let Some(notify) = notify {
            // Reset cursor visibility before printing exit message;
            // the child may have hidden the cursor (DECTCEM ESC[?25l)
            // and never restored it (e.g. SSH to a server that reboots).
            emit_output_for_pane(self.pane_id, &format!("\x1b[?25h{notify}"));
        } else if !terse.is_empty() {
            // ExitBehaviorMessaging::None — still reset cursor visibility
            emit_output_for_pane(self.pane_id, "\x1b[?25h");
        }

        match &*proc {
            ProcessState::Running { .. } => false,
            ProcessState::DeadPendingClose { .. } => false,
            ProcessState::Dead => true,
        }
    }

    fn set_clipboard(&self, clipboard: &Arc<dyn Clipboard>) {
        self.terminal.lock().set_clipboard(clipboard);
    }

    fn set_download_handler(&self, handler: &Arc<dyn DownloadHandler>) {
        self.terminal.lock().set_download_handler(handler);
    }

    fn set_config(&self, config: Arc<dyn TerminalConfiguration>) {
        self.terminal.lock().set_config(config);
    }

    fn config_palette_changed(&self) {
        self.terminal.lock().config_palette_changed();
    }

    fn get_config(&self) -> Option<Arc<dyn TerminalConfiguration>> {
        Some(self.terminal.lock().get_config())
    }

    fn perform_actions(&self, actions: Vec<termwiz::escape::Action>) {
        self.terminal.lock().perform_actions(actions)
    }

    fn mouse_event(&self, event: MouseEvent) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        self.terminal.lock().mouse_event(event)
    }

    fn key_down(&self, key: KeyCode, mods: KeyModifiers) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        if self.tmux_domain.lock().is_some() {
            log::trace!("key: {:?}", key);
            if key == KeyCode::Char('q') {
                self.terminal.lock().send_paste("detach\n")?;
            }
            return Ok(());
        } else {
            self.terminal.lock().key_down(key, mods)
        }
    }

    fn key_up(&self, key: KeyCode, mods: KeyModifiers) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        self.terminal.lock().key_up(key, mods)
    }

    fn resize(&self, size: TerminalSize) -> Result<(), Error> {
        self.pty.lock().resize(PtySize {
            rows: size.rows.try_into()?,
            cols: size.cols.try_into()?,
            pixel_width: size.pixel_width.try_into()?,
            pixel_height: size.pixel_height.try_into()?,
        })?;
        self.terminal.lock().resize(size);
        Ok(())
    }

    fn resize_visual(&self, size: TerminalSize) -> Result<(), Error> {
        self.terminal.lock().resize(size);
        Ok(())
    }

    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        Mux::get().record_input_for_current_identity();
        MutexGuard::map(self.writer.lock(), |writer| {
            let w: &mut dyn std::io::Write = writer;
            w
        })
    }

    fn reader(&self) -> anyhow::Result<Option<PaneReader>> {
        let pty = self.pty.lock();
        let reader = pty.try_clone_reader()?;
        #[cfg(unix)]
        let fd = pty.as_raw_fd();
        Ok(Some(PaneReader {
            reader,
            #[cfg(unix)]
            fd,
        }))
    }

    fn send_text(&self, text: &str) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        self.terminal.lock().send_text(text)
    }

    fn send_paste(&self, text: &str) -> Result<(), Error> {
        Mux::get().record_input_for_current_identity();
        if self.tmux_domain.lock().is_some() {
            Ok(())
        } else {
            self.terminal.lock().send_paste(text)
        }
    }

    fn get_title(&self) -> String {
        let title = self.terminal.lock().get_title().to_string();
        // If the title is the default pane title, then try to spice
        // things up a bit by returning the process basename instead
        if title == "wezterm" {
            if let Some(proc_name) = self.get_foreground_process_name(CachePolicy::AllowStale) {
                let proc_name = std::path::Path::new(&proc_name);
                if let Some(name) = proc_name.file_name() {
                    return name.to_string_lossy().to_string();
                }
            }
        }

        title
    }

    fn get_progress(&self) -> Progress {
        self.terminal.lock().get_progress()
    }

    fn get_program_status(&self) -> Option<String> {
        self.terminal
            .lock()
            .program_status()
            .map(|state| state.as_str().to_string())
    }

    fn palette(&self) -> ColorPalette {
        self.terminal.lock().palette()
    }

    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn erase_scrollback(&self, erase_mode: ScrollbackEraseMode) {
        match erase_mode {
            ScrollbackEraseMode::ScrollbackOnly => {
                self.terminal.lock().erase_scrollback();
            }
            ScrollbackEraseMode::ScrollbackAndViewport => {
                self.terminal.lock().erase_scrollback_and_viewport();
            }
        }
    }

    fn inject_scrollback(&self, lines: Vec<Line>) -> anyhow::Result<()> {
        self.terminal.lock().prepend_primary_scrollback(lines);
        Ok(())
    }

    fn focus_changed(&self, focused: bool) {
        self.terminal.lock().focus_changed(focused);
    }

    fn refresh_focus(&self, focused: bool) {
        self.terminal.lock().refresh_focus(focused);
    }

    fn has_unseen_output(&self) -> bool {
        self.terminal.lock().has_unseen_output()
    }

    fn is_mouse_grabbed(&self) -> bool {
        if self.tmux_domain.lock().is_some() {
            false
        } else {
            self.terminal.lock().is_mouse_grabbed()
        }
    }

    fn application_cursor_keys_enabled(&self) -> bool {
        self.terminal.lock().application_cursor_keys_enabled()
    }

    fn is_alt_screen_active(&self) -> bool {
        if self.tmux_domain.lock().is_some() {
            false
        } else {
            self.terminal.lock().is_alt_screen_active()
        }
    }

    fn is_primary_peek(&self) -> bool {
        self.terminal.lock().is_primary_peek()
    }

    fn set_primary_peek(&self, peek: bool) {
        self.terminal.lock().set_primary_peek(peek);
    }

    fn get_current_working_dir(&self, policy: CachePolicy) -> Option<Url> {
        self.terminal
            .lock()
            .get_current_dir()
            .cloned()
            .or_else(|| self.divine_current_working_dir(policy))
    }

    fn tty_name(&self) -> Option<String> {
        #[cfg(unix)]
        {
            let name = self.pty.lock().tty_name()?;
            Some(name.to_string_lossy().into_owned())
        }
    }

    fn get_foreground_process_info(&self, policy: CachePolicy) -> Option<LocalProcessInfo> {
        #[cfg(unix)]
        if let Some(pid) = self.pty.lock().process_group_leader() {
            let pid = pid as u32;
            if policy == CachePolicy::AllowStale {
                let cache = self.foreground_proc.lock();
                if let Some(cached) = cache.as_ref() {
                    if cached.pid == pid && cached.updated.elapsed() <= PROC_INFO_CACHE_TTL {
                        return Some(cached.info.clone());
                    }
                }
            }
            let info = LocalProcessInfo::with_root_pid(pid)?;
            *self.foreground_proc.lock() = Some(CachedForegroundProc {
                updated: Instant::now(),
                pid,
                info: info.clone(),
            });
            return Some(info);
        }

        self.divine_foreground_process(policy)
    }

    fn get_foreground_process_name(&self, policy: CachePolicy) -> Option<String> {
        #[cfg(unix)]
        {
            let leader = self.get_leader(policy);
            if let Some(path) = &leader.path {
                return Some(path.to_string_lossy().to_string());
            }
            // A pipeline's group leader can exit before its pager. Reuse the
            // cached shell tree, restricted to the group that still owns the tty.
            // A fresh tree from the previous group is still the wrong snapshot.
            let policy = if self
                .proc_list
                .lock()
                .as_ref()
                .map(|info| info.updated < leader.updated)
                .unwrap_or(false)
            {
                CachePolicy::FetchImmediate
            } else {
                policy
            };
            let info = self.divine_process_list(policy)?;
            return foreground_group_member(&info.root, leader.pid)
                .map(|process| process.executable.to_string_lossy().into_owned());
        }

        #[allow(unreachable_code)]
        None
    }

    fn can_close_without_prompting(&self, _reason: CloseReason) -> bool {
        // Consider only the tty's foreground process group so backgrounded
        // daemons spawned by the shell (e.g. gitstatusd) don't count as a
        // stateful process at an otherwise idle prompt.
        let fg_pgid = self.pty.lock().process_group_leader().map(|p| p as u32);

        // Lua hooks can query this pane again, including its process name.
        // Release the process cache lock before invoking user code.
        let process_tree = self
            .divine_process_list(CachePolicy::FetchImmediate)
            .map(|info| info.root.clone());
        if let Some(root) = process_tree {
            log::trace!(
                "can_close_without_prompting? fg_pgid={:?} procs in pane {:#?}",
                fg_pgid,
                root
            );

            let hook_result = config::run_immediate_with_lua_config(|lua| {
                let lua = match lua {
                    Some(lua) => lua,
                    None => return Ok(None),
                };
                let v = config::lua::emit_sync_callback(
                    &*lua,
                    ("mux-is-process-stateful".to_string(), (root.clone())),
                )?;
                match v {
                    mlua::Value::Nil => Ok(None),
                    mlua::Value::Boolean(v) => Ok(Some(v)),
                    _ => Ok(None),
                }
            });

            fn default_stateful_check(proc_list: &LocalProcessInfo, fg_pgid: Option<u32>) -> bool {
                // Restrict to the foreground process group when we know it, so
                // background jobs in the same process tree are ignored. When the
                // foreground group is unknown (no tty, e.g. ssh/serial), fall
                // back to the whole tree.
                let exe_names = match fg_pgid {
                    Some(pgid) => proc_list.flatten_to_exe_names_in_group(pgid),
                    None => proc_list.flatten_to_exe_names(),
                };

                // Fig uses `figterm` a pseudo terminal for a lot of functionality, it runs between
                // the shell and terminal. Unfortunately it is typically named `<shell> (figterm)`,
                // which prevents the statuful check from passing. This strips the suffix from the
                // process name to allow the check to pass.
                let names = exe_names
                    .into_iter()
                    .map(|s| match s.strip_suffix(" (figterm)") {
                        Some(s) => s.into(),
                        None => s,
                    })
                    .collect::<HashSet<_>>();

                let skip = configuration()
                    .skip_close_confirmation_for_processes_named
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>();

                if !names.is_subset(&skip) {
                    // There are other processes running than are listed,
                    // so we consider this to be stateful
                    return true;
                }
                false
            }

            let is_stateful = match hook_result {
                Ok(None) => default_stateful_check(&root, fg_pgid),
                Ok(Some(s)) => s,
                Err(err) => {
                    log::error!(
                        "Error while running mux-is-process-stateful \
                         hook: {:#}, falling back to default behavior",
                        err
                    );
                    default_stateful_check(&root, fg_pgid)
                }
            };

            !is_stateful
        } else {
            #[cfg(unix)]
            {
                // If the process is dead but exit_behavior is holding the
                // window, we don't need to prompt to confirm closing.
                // That is detectable as no longer having a process group leader.
                if self.pty.lock().process_group_leader().is_none() {
                    return true;
                }
            }

            false
        }
    }

    fn get_semantic_zones(&self) -> anyhow::Result<Vec<SemanticZone>> {
        let mut term = self.terminal.lock();
        term.get_semantic_zones()
    }

    fn get_last_command_status(&self) -> Option<i32> {
        let term = self.terminal.lock();
        term.get_last_command_status()
    }

    fn get_last_command_output_start(&self) -> Option<StableRowIndex> {
        let term = self.terminal.lock();
        term.get_last_command_output_start()
    }

    async fn search(
        &self,
        pattern: Pattern,
        range: Range<StableRowIndex>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let term = self.terminal.lock();
        let screen = term.screen();

        enum CompiledPattern {
            CaseSensitiveString(String),
            CaseInSensitiveString(String),
            Regex(Regex),
        }

        let pattern = match pattern {
            Pattern::CaseSensitiveString(s) => CompiledPattern::CaseSensitiveString(s),
            Pattern::CaseInSensitiveString(s) => {
                // normalize the case so we match everything lowercase
                CompiledPattern::CaseInSensitiveString(s.to_lowercase())
            }
            Pattern::CaseSmartString(s) => {
                if s.chars().any(|c| c.is_uppercase()) {
                    CompiledPattern::CaseSensitiveString(s)
                } else {
                    // normalize the case so we match everything lowercase
                    CompiledPattern::CaseInSensitiveString(s.to_lowercase())
                }
            }
            Pattern::Regex(r) => CompiledPattern::Regex(Regex::new(&r)?),
        };

        let mut results = vec![];
        let mut uniq_matches: HashMap<String, usize> = HashMap::new();

        screen.for_each_logical_line_in_stable_range(range, |sr, lines| {
            if let Some(limit) = limit {
                if results.len() == limit as usize {
                    // We've reach the limit, stop iteration.
                    return false;
                }
            }

            if lines.is_empty() {
                // Nothing to do on this iteration, carry on with the next.
                return true;
            }
            let haystack = if lines.len() == 1 {
                lines[0].as_str()
            } else {
                let mut s = String::new();
                for line in lines {
                    s.push_str(&line.as_str());
                }
                Cow::Owned(s)
            };
            let stable_idx = sr.start;

            if haystack.is_empty() {
                return true;
            }

            let haystack = match &pattern {
                CompiledPattern::CaseInSensitiveString(_) => Cow::Owned(haystack.to_lowercase()),
                _ => haystack,
            };
            let mut coords = None;

            match &pattern {
                CompiledPattern::CaseInSensitiveString(s)
                | CompiledPattern::CaseSensitiveString(s) => {
                    for (idx, s) in haystack.match_indices(s) {
                        found_match(
                            s,
                            idx,
                            lines,
                            stable_idx,
                            &mut uniq_matches,
                            &mut coords,
                            &mut results,
                        );
                    }
                }
                CompiledPattern::Regex(re) => {
                    // Allow for the regex to contain captures
                    for capture_res in re.captures_iter(&haystack) {
                        match capture_res {
                            Ok(c) => {
                                // Look for the captures in reverse order, as index==0 is
                                // the whole matched string.  We can't just call
                                // `c.iter().rev()` as the capture iterator isn't double-ended.
                                for idx in (0..c.len()).rev() {
                                    if let Some(m) = c.get(idx) {
                                        found_match(
                                            m.as_str(),
                                            m.start(),
                                            lines,
                                            stable_idx,
                                            &mut uniq_matches,
                                            &mut coords,
                                            &mut results,
                                        );
                                        break;
                                    }
                                }
                            }
                            Err(err) => {
                                // On errors like max backtracking limit reached, fancy_regex does
                                // NOT advance the iterator position, so silently ignoring Err
                                // would loop forever.
                                log::warn!("line {stable_idx} search error: {err}");
                                log::warn!("stopping collecting matches on line {stable_idx}");
                                break;
                            }
                        }
                    }
                }
            }

            // Keep iterating
            true
        });

        #[derive(Copy, Clone, Debug)]
        struct Coord {
            byte_idx: usize,
            grapheme_idx: usize,
            stable_row: StableRowIndex,
        }

        fn found_match(
            text: &str,
            byte_idx: usize,
            lines: &[&Line],
            stable_idx: StableRowIndex,
            uniq_matches: &mut HashMap<String, usize>,
            coords: &mut Option<Vec<Coord>>,
            results: &mut Vec<SearchResult>,
        ) {
            if coords.is_none() {
                coords.replace(make_coords(lines, stable_idx));
            }
            let coords = coords.as_ref().unwrap();

            let match_id = match uniq_matches.get(text).copied() {
                Some(id) => id,
                None => {
                    let id = uniq_matches.len();
                    uniq_matches.insert(text.to_owned(), id);
                    id
                }
            };
            let (start_x, start_y) = haystack_idx_to_coord(byte_idx, coords);
            let (end_x, end_y) = haystack_idx_to_coord(byte_idx + text.len(), coords);
            results.push(SearchResult {
                start_x,
                start_y,
                end_x,
                end_y,
                match_id,
            });
        }

        fn make_coords(lines: &[&Line], stable_row: StableRowIndex) -> Vec<Coord> {
            let mut byte_idx = 0;
            let mut coords = vec![];

            for (row_idx, line) in lines.iter().enumerate() {
                for cell in line.visible_cells() {
                    coords.push(Coord {
                        byte_idx,
                        grapheme_idx: cell.cell_index(),
                        stable_row: stable_row + row_idx as StableRowIndex,
                    });
                    byte_idx += cell.str().len();
                }
            }

            coords
        }

        fn haystack_idx_to_coord(idx: usize, coords: &[Coord]) -> (usize, StableRowIndex) {
            let c = coords
                .binary_search_by(|ele| ele.byte_idx.cmp(&idx))
                .or_else(|i| -> Result<usize, usize> { Ok(i) })
                .unwrap();
            let coord = coords.get(c).map(|c| *c).unwrap_or_else(|| {
                let last = coords.last().unwrap();
                Coord {
                    grapheme_idx: last.grapheme_idx + 1,
                    ..*last
                }
            });
            (coord.grapheme_idx, coord.stable_row)
        }

        Ok(results)
    }
}

struct LocalPaneDCSHandler {
    pane_id: PaneId,
    tmux_domain: Option<Arc<TmuxDomainState>>,
}

pub(crate) fn emit_output_for_pane(pane_id: PaneId, message: &str) {
    let mut parser = termwiz::escape::parser::Parser::new();
    let mut actions = vec![Action::CSI(CSI::Sgr(Sgr::Reset))];
    parser.parse(message.as_bytes(), |action| actions.push(action));

    promise::spawn::spawn_into_main_thread(async move {
        let mux = Mux::get();
        if let Some(pane) = mux.get_pane(pane_id) {
            pane.perform_actions(actions);
            mux.notify(MuxNotification::PaneOutput(pane_id));
        }
    })
    .detach();
}

impl wezterm_term::DeviceControlHandler for LocalPaneDCSHandler {
    fn handle_device_control(&mut self, control: termwiz::escape::DeviceControlMode) {
        match control {
            DeviceControlMode::Enter(mode) => {
                if !mode.ignored_extra_intermediates
                    && mode.params.len() == 1
                    && mode.params[0] == 1000
                    && mode.intermediates.is_empty()
                {
                    log::info!("tmux -CC mode requested");

                    // Create a new domain to host these tmux tabs
                    let domain = TmuxDomain::new(self.pane_id);
                    let tmux_domain = Arc::clone(&domain.inner);

                    let domain: Arc<dyn Domain> = Arc::new(domain);
                    let mux = Mux::get();
                    mux.add_domain(&domain);

                    if let Some(pane) = mux.get_pane(self.pane_id) {
                        let pane = pane.downcast_ref::<LocalPane>().unwrap();
                        pane.tmux_domain.lock().replace(Arc::clone(&tmux_domain));

                        emit_output_for_pane(
                            self.pane_id,
                            "\r\n[This pane is running tmux control mode. Press q to detach]",
                        );
                    }

                    self.tmux_domain.replace(tmux_domain);

                // TODO: do we need to proactively list available tabs here?
                // if so we should arrange to call domain.attach() and make
                // it do the right thing.
                } else if configuration().log_unknown_escape_sequences {
                    log::warn!("unknown DeviceControlMode::Enter {:?}", mode,);
                }
            }
            DeviceControlMode::Exit => {
                if let Some(tmux) = self.tmux_domain.take() {
                    let mux = Mux::get();
                    if let Some(pane) = mux.get_pane(self.pane_id) {
                        let pane = pane.downcast_ref::<LocalPane>().unwrap();
                        pane.tmux_domain.lock().take();
                    }
                    mux.domain_was_detached(tmux.domain_id);
                }
            }
            DeviceControlMode::Data(c) => {
                if configuration().log_unknown_escape_sequences {
                    log::warn!(
                        "unhandled DeviceControlMode::Data {:x} {}",
                        c,
                        (c as char).escape_debug()
                    );
                }
            }
            DeviceControlMode::TmuxEvents(events) => {
                if let Some(tmux) = self.tmux_domain.as_ref() {
                    tmux.advance(events);
                } else {
                    log::warn!("unhandled DeviceControlMode::TmuxEvents {:?}", &events);
                }
            }
            _ => {
                if configuration().log_unknown_escape_sequences {
                    log::warn!("unhandled: {:?}", control);
                }
            }
        }
    }
}

struct LocalPaneNotifHandler {
    pane_id: PaneId,
}

impl AlertHandler for LocalPaneNotifHandler {
    fn alert(&mut self, alert: Alert) {
        let pane_id = self.pane_id;
        promise::spawn::spawn_into_main_thread(async move {
            let mux = Mux::get();
            match &alert {
                Alert::WindowTitleChanged(title) => {
                    if let Some((_domain, window_id, _tab_id)) = mux.resolve_pane_id(pane_id) {
                        if let Some(mut window) = mux.get_window_mut(window_id) {
                            window.set_title(title);
                        }
                    }
                }
                Alert::TabTitleChanged(title) => {
                    if let Some((_domain, _window_id, tab_id)) = mux.resolve_pane_id(pane_id) {
                        if let Some(tab) = mux.get_tab(tab_id) {
                            tab.set_title(title.as_deref().unwrap_or(""));
                        }
                    }
                }
                _ => {}
            }

            mux.notify(MuxNotification::Alert { pane_id, alert });
        })
        .detach();
    }
}

/// This is a little gross; on some systems, our pipe reader will continue
/// to be blocked in read even after the child process has died.
/// We need to wake up and notice that the child terminated in order
/// for our state to wind down.
/// This block schedules a background thread to wait for the child
/// to terminate, and then nudge the muxer to check for dead processes.
/// Without this, typing `exit` in `cmd.exe` would keep the pane around
/// until something else triggered the mux to prune dead processes.
fn split_child(
    mut process: Box<dyn Child>,
) -> (
    Receiver<IoResult<ExitStatus>>,
    Box<dyn ChildKiller + Sync>,
    Option<u32>,
) {
    let pid = process.process_id();
    let signaller = process.clone_killer();

    let (tx, rx) = bounded(1);

    std::thread::spawn(move || {
        let status = process.wait();
        tx.try_send(status).ok();
        promise::spawn::spawn_into_main_thread(async move {
            let mux = Mux::get();
            mux.prune_dead_windows();
        })
        .detach();
    });

    (rx, signaller, pid)
}

impl LocalPane {
    pub fn new(
        pane_id: PaneId,
        mut terminal: Terminal,
        process: Box<dyn Child + Send>,
        pty: Box<dyn MasterPty>,
        writer: Box<dyn Write + Send>,
        domain_id: DomainId,
        encoding: Arc<AtomicU8>,
        command_description: String,
    ) -> Self {
        let (process, signaller, pid) = split_child(process);

        terminal.set_device_control_handler(Box::new(LocalPaneDCSHandler {
            pane_id,
            tmux_domain: None,
        }));
        terminal.set_notification_handler(Box::new(LocalPaneNotifHandler { pane_id }));

        Self {
            pane_id,
            terminal: Mutex::new(terminal),
            process: Mutex::new(ProcessState::Running {
                child_waiter: process,
                pid,
                signaller,
                killed: false,
            }),
            pty: Mutex::new(pty),
            writer: Mutex::new(writer),
            domain_id,
            tmux_domain: Mutex::new(None),
            proc_list: Mutex::new(None),
            #[cfg(unix)]
            leader: Arc::new(Mutex::new(None)),
            #[cfg(unix)]
            foreground_proc: Mutex::new(None),
            encoding,
            command_description,
        }
    }

    #[cfg(unix)]
    fn get_leader(&self, policy: CachePolicy) -> CachedLeaderInfo {
        let mut leader = self.leader.lock();

        if policy == CachePolicy::FetchImmediate {
            leader.replace(CachedLeaderInfo::new(self.pty.lock().as_raw_fd()));
        } else if let Some(info) = leader.as_mut() {
            let pid = unsafe { libc::tcgetpgrp(info.fd) } as u32;
            let path = LocalProcessInfo::executable_path(pid);
            if pid != info.pid || path != info.path {
                info.update();
            } else if info.expired() && info.can_update() {
                // The same program may still change cwd. Refresh it
                // asynchronously as before.
                info.updating = true;
                let leader_ref = Arc::clone(&self.leader);
                std::thread::spawn(move || {
                    let mut leader = leader_ref.lock();
                    if let Some(leader) = leader.as_mut() {
                        leader.update();
                    }
                });
            }
        } else {
            leader.replace(CachedLeaderInfo::new(self.pty.lock().as_raw_fd()));
        }

        (*leader).clone().unwrap()
    }

    fn divine_current_working_dir(&self, policy: CachePolicy) -> Option<Url> {
        #[cfg(unix)]
        {
            let leader = self.get_leader(policy);
            if let Some(path) = &leader.current_working_dir {
                return Url::from_directory_path(path).ok();
            }
            return None;
        }

        #[allow(unreachable_code)]
        None
    }

    fn divine_process_list(
        &self,
        policy: CachePolicy,
    ) -> Option<MappedMutexGuard<'_, CachedProcInfo>> {
        if let ProcessState::Running { pid: Some(pid), .. } = &*self.process.lock() {
            let mut proc_list = self.proc_list.lock();

            let expired = policy == CachePolicy::FetchImmediate
                || proc_list
                    .as_ref()
                    .map(|info| info.updated.elapsed() > PROC_INFO_CACHE_TTL)
                    .unwrap_or(true);

            if expired {
                log::trace!("CachedProcInfo expired, refresh");
                let root = LocalProcessInfo::with_root_pid(*pid)?;

                // Windows doesn't have any job control or session concept,
                // so we infer that the equivalent to the process group
                // leader is the most recently spawned program running
                // in the console
                let mut youngest = &root;

                fn find_youngest<'a>(
                    proc: &'a LocalProcessInfo,
                    youngest: &mut &'a LocalProcessInfo,
                ) {
                    if proc.start_time >= youngest.start_time {
                        *youngest = proc;
                    }

                    for child in proc.children.values() {
                        find_youngest(child, youngest);
                    }
                }

                find_youngest(&root, &mut youngest);
                let mut foreground = youngest.clone();
                foreground.children.clear();

                proc_list.replace(CachedProcInfo {
                    root,
                    foreground,
                    updated: Instant::now(),
                });
                log::trace!("CachedProcInfo updated");
            }

            return Some(MutexGuard::map(proc_list, |info| info.as_mut().unwrap()));
        }
        None
    }

    fn divine_foreground_process(&self, policy: CachePolicy) -> Option<LocalProcessInfo> {
        self.divine_process_list(policy)
            .map(|info| info.foreground.clone())
    }
}

fn foreground_group_member(root: &LocalProcessInfo, pgid: u32) -> Option<&LocalProcessInfo> {
    let member = (root.pgid == pgid
        && !root.executable.as_os_str().is_empty()
        && !matches!(
            root.status,
            procinfo::LocalProcessStatus::Zombie | procinfo::LocalProcessStatus::Dead
        ))
    .then_some(root);
    root.children
        .values()
        .filter_map(|child| foreground_group_member(child, pgid))
        .chain(member)
        .max_by_key(|process| (process.start_time, process.pid))
}

impl Drop for LocalPane {
    fn drop(&mut self) {
        // Avoid lingering zombies if we can, but don't block forever.
        // <https://github.com/wezterm/wezterm/issues/558>
        if let ProcessState::Running { signaller, .. } = &mut *self.process.lock() {
            let _ = signaller.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use procinfo::LocalProcessStatus;

    /// The exit notice, the password mask and the default confirmation are
    /// interface text: no emoji in them (the WezTerm-era 👍, ⚠️, 🔑 and 🛑).
    #[test]
    fn interface_text_carries_no_emoji() {
        let sources = [
            include_str!("localpane.rs"),
            include_str!("connui.rs"),
            include_str!("ssh.rs"),
            include_str!("../../config/src/keyassignment.rs"),
        ];
        let is_emoji = |c: char| matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F);
        for source in sources {
            for line in source.lines() {
                let ui = line.contains("brief = format!(")
                    || line.contains("let placeholder =")
                    || line.contains("Really continue");
                if ui {
                    assert!(
                        !line.chars().any(is_emoji),
                        "emoji in interface text: {}",
                        line
                    );
                }
            }
        }
    }

    fn process(pid: u32, pgid: u32, executable: &str) -> LocalProcessInfo {
        LocalProcessInfo {
            pid,
            ppid: 1,
            pgid,
            name: String::new(),
            executable: executable.into(),
            argv: vec![],
            cwd: std::path::PathBuf::new(),
            status: LocalProcessStatus::Run,
            start_time: pid as u64,
            children: HashMap::new(),
        }
    }

    #[test]
    fn foreground_name_survives_an_exited_pipeline_leader() {
        let mut shell = process(100, 100, "/bin/zsh");
        // The producer at pid 200 has already exited. Its group still owns
        // the terminal, and a newer background task must not replace less.
        shell.children.insert(201, process(201, 200, "/bin/cat"));
        shell
            .children
            .insert(202, process(202, 200, "/usr/bin/less"));
        shell
            .children
            .insert(300, process(300, 300, "/usr/bin/sleep"));
        let found = foreground_group_member(&shell, 200).expect("the pager is still running");
        assert_eq!(found.executable, std::path::Path::new("/usr/bin/less"));
        assert_eq!(foreground_group_member(&shell, 100).unwrap().pid, 100);
        assert!(foreground_group_member(&shell, 400).is_none());
    }

    #[test]
    fn foreground_name_ignores_empty_paths_and_exited_members() {
        let mut shell = process(100, 100, "/bin/zsh");
        shell.children.insert(201, process(201, 200, ""));
        let mut exited = process(202, 200, "/usr/bin/seq");
        exited.status = LocalProcessStatus::Zombie;
        shell.children.insert(202, exited);
        assert!(foreground_group_member(&shell, 200).is_none());
    }

    fn test_pane(
        command: portable_pty::CommandBuilder,
    ) -> (LocalPane, Box<dyn Child + Send + Sync>) {
        let pair = portable_pty::native_pty_system()
            .openpty(PtySize::default())
            .unwrap();
        let child = pair.slave.spawn_command(command).unwrap();
        let pid = child.process_id().unwrap();
        let terminal = Terminal::new(
            TerminalSize::default(),
            Arc::new(config::TermConfig::new()),
            "test",
            "test",
            Box::new(std::io::sink()),
        );
        // Keep the child owned by the test rather than starting a mux waiter.
        let pane = LocalPane {
            pane_id: PaneId::new(0),
            terminal: Mutex::new(terminal),
            process: Mutex::new(ProcessState::Running {
                child_waiter: bounded(1).1,
                pid: Some(pid),
                signaller: child.clone_killer(),
                killed: false,
            }),
            pty: Mutex::new(pair.master),
            writer: Mutex::new(Box::new(std::io::sink())),
            domain_id: DomainId::new(0),
            tmux_domain: Mutex::new(None),
            proc_list: Mutex::new(None),
            leader: Arc::new(Mutex::new(None)),
            foreground_proc: Mutex::new(None),
            encoding: Arc::new(AtomicU8::new(PaneEncoding::Utf8.to_u8())),
            command_description: String::new(),
        };
        (pane, child)
    }

    #[test]
    fn foreground_name_invalidates_changed_identity_but_reuses_same_program() {
        let (pane, mut child) = test_pane(portable_pty::CommandBuilder::new("/bin/cat"));
        let pid = child.process_id().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let actual_path = std::path::PathBuf::from("/bin/cat");
        while LocalProcessInfo::executable_path(pid).as_ref() != Some(&actual_path)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut leader = CachedLeaderInfo::new(pane.pty.lock().as_raw_fd());
        leader.pid = pid + 1;
        leader.path = Some("/stale/shell".into());
        pane.leader.lock().replace(leader);
        let switched = pane.get_foreground_process_name(CachePolicy::AllowStale);

        pane.leader.lock().as_mut().unwrap().path = Some("/stale/before-exec".into());
        let same_group_changed = pane.get_foreground_process_name(CachePolicy::AllowStale);
        let updated = {
            let mut cache = pane.leader.lock();
            let leader = cache.as_mut().unwrap();
            leader.pid = pid;
            leader.path = Some(actual_path.clone());
            leader.updated
        };
        let reused = pane.get_foreground_process_name(CachePolicy::AllowStale);
        let same_updated = pane.leader.lock().as_ref().unwrap().updated;
        let fast_path_skipped_tree = pane.proc_list.lock().is_none();

        *pane.process.lock() = ProcessState::Dead;
        child.kill().unwrap();
        child.wait().unwrap();

        let expected = Some(actual_path.to_string_lossy().into_owned());
        assert_eq!(switched, expected);
        assert_eq!(same_group_changed, expected);
        assert_eq!(reused, expected);
        assert_eq!(same_updated, updated);
        assert!(fast_path_skipped_tree);
    }

    #[test]
    fn foreground_name_drops_an_exited_leader_before_the_cache_ttl() {
        let mut command = portable_pty::CommandBuilder::new("/bin/zsh");
        command.args(["-f", "-i", "-c", "seq 1 3000 | less -X -M"]);
        command.env("TERM", "xterm-256color");
        let (pane, mut child) = test_pane(command);
        let pid = child.process_id().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut pager_group = None;
        while Instant::now() < deadline {
            if let Some(group) = pane.pty.lock().process_group_leader() {
                let group = group as u32;
                if group != pid && LocalProcessInfo::executable_path(group).is_none() {
                    if let Some(tree) = LocalProcessInfo::with_root_pid(pid) {
                        if foreground_group_member(&tree, group)
                            .and_then(|process| process.executable.file_name())
                            == Some(std::ffi::OsStr::new("less"))
                        {
                            pager_group = Some(group);
                            break;
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut leader = CachedLeaderInfo::new(pane.pty.lock().as_raw_fd());
        // This is the real observed race: the same foreground group still
        // owns the tty, but its briefly cached producer has already exited.
        leader.path = Some("/usr/bin/seq".into());
        let updated = leader.updated;
        pane.leader.lock().replace(leader);

        // The shell tree was sampled before the foreground group changed,
        // even though its normal TTL has not expired yet.
        pane.proc_list.lock().replace(CachedProcInfo {
            root: process(pid + 1, pid + 1, "/stale/shell"),
            foreground: process(pid + 1, pid + 1, "/stale/shell"),
            updated: updated - Duration::from_millis(1),
        });
        let refreshed_tree = pane.get_foreground_process_name(CachePolicy::AllowStale);
        let tree_updated = pane.proc_list.lock().as_ref().unwrap().updated;
        let reused_tree = pane.get_foreground_process_name(CachePolicy::AllowStale);
        let same_tree_updated = pane.proc_list.lock().as_ref().unwrap().updated;
        *pane.process.lock() = ProcessState::Dead;
        if let Some(group) = pager_group {
            // Only the proven descendant pipeline belongs to this test.
            unsafe { libc::kill(-(group as i32), libc::SIGTERM) };
        }
        child.kill().ok();
        child.wait().unwrap();

        assert!(
            pager_group.is_some(),
            "the producer exited and less owns the tty"
        );
        assert_eq!(refreshed_tree.as_deref(), Some("/usr/bin/less"));
        assert_eq!(reused_tree, refreshed_tree);
        assert_eq!(same_tree_updated, tree_updated);
    }
}
