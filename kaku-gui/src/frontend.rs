use crate::scripting::guiwin::GuiWin;
use crate::spawn::SpawnWhere;
use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use ::window::*;
use anyhow::{Context, Error};
use config::keyassignment::{KeyAssignment, SpawnCommand, SpawnTabDomain};
use config::{ConfigSubscription, NotificationHandling};
use mux::client::ClientId;
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use promise::{Future, Promise};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wezterm_term::{Alert, ClipboardSelection};
use wezterm_toast_notification::*;

pub const SET_DEFAULT_TERMINAL_EVENT: &str = "set-default-terminal";

pub struct GuiFrontEnd {
    connection: Rc<Connection>,
    switching_workspaces: RefCell<bool>,
    spawned_mux_window: RefCell<HashSet<MuxWindowId>>,
    known_windows: RefCell<BTreeMap<Window, MuxWindowId>>,
    client_id: Arc<ClientId>,
    config_subscription: RefCell<Option<ConfigSubscription>>,
    /// Global count of unread bell events across all windows
    unread_bell_count: RefCell<usize>,
}

impl Drop for GuiFrontEnd {
    fn drop(&mut self) {
        ::window::shutdown();
    }
}

lazy_static::lazy_static! {
    static ref FAST_CONFIG_SNAPSHOT: Mutex<Option<config::ConfigHandle>> = Mutex::new(None);
}

fn fast_config_snapshot() -> config::ConfigHandle {
    if let Some(cfg) = FAST_CONFIG_SNAPSHOT.lock().unwrap().as_ref().cloned() {
        return cfg;
    }
    let cfg = config::configuration();
    FAST_CONFIG_SNAPSHOT.lock().unwrap().replace(cfg.clone());
    cfg
}

pub(crate) fn refresh_fast_config_snapshot() {
    let cfg = config::configuration();
    FAST_CONFIG_SNAPSHOT.lock().unwrap().replace(cfg);
}

fn resolve_bundled_kaku_bin() -> anyhow::Result<PathBuf> {
    fn add_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
        if !candidates.iter().any(|p| p == &path) {
            candidates.push(path);
        }
    }

    let mut candidates = Vec::new();

    if let Some(path) = std::env::var_os("KAKU_BIN") {
        add_candidate(&mut candidates, PathBuf::from(path));
    }

    let current_exe = std::env::current_exe().context("resolve executable path")?;
    if let Some(parent) = current_exe.parent() {
        add_candidate(&mut candidates, parent.join("kaku"));
    }

    if let Ok(resolved_exe) = std::fs::canonicalize(&current_exe) {
        if let Some(parent) = resolved_exe.parent() {
            add_candidate(&mut candidates, parent.join("kaku"));
        }
    }

    add_candidate(
        &mut candidates,
        config::HOME_DIR
            .join(".config")
            .join("kaku")
            .join("zsh")
            .join("bin")
            .join("kaku"),
    );

    #[cfg(target_os = "macos")]
    {
        add_candidate(
            &mut candidates,
            PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku"),
        );
        add_candidate(
            &mut candidates,
            config::HOME_DIR
                .join("Applications")
                .join("Kaku.app")
                .join("Contents")
                .join("MacOS")
                .join("kaku"),
        );
    }

    if let Some(path) = candidates.iter().find(|path| path.exists()) {
        return Ok(path.clone());
    }

    anyhow::bail!(
        "could not find kaku binary; checked: {}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(crate) fn kaku_cli_program_for_spawn() -> String {
    match resolve_bundled_kaku_bin() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(err) => {
            // Finder-launched apps can have a minimal PATH; fall back only when
            // we cannot resolve the bundled companion binary.
            log::warn!("Falling back to PATH lookup for `kaku`: {err:#}");
            "kaku".to_string()
        }
    }
}

pub fn open_kaku_config() {
    let kaku_bin = kaku_cli_program_for_spawn();

    promise::spawn::spawn_into_main_thread(async move {
        let config = fast_config_snapshot();
        let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi());
        let size = config.initial_size(dpi as u32, None);
        let term_config = Arc::new(config::TermConfig::with_config(config));
        crate::spawn::spawn_command_impl(
            &SpawnCommand {
                domain: SpawnTabDomain::DomainName("local".to_string()),
                args: Some(vec![kaku_bin, "config".to_string()]),
                ..Default::default()
            },
            SpawnWhere::NewTab,
            size,
            None,
            term_config,
        );
    })
    .detach();
}

pub fn run_kaku_update_from_menu() {
    static UPDATE_RUNNING: AtomicBool = AtomicBool::new(false);
    if UPDATE_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("run_kaku_update_from_menu: update already running, ignoring");
        return;
    }
    run_kaku_subcommand_in_new_tab("update", Some(&UPDATE_RUNNING));
}

pub fn run_kaku_doctor_in_new_tab() {
    run_kaku_subcommand_in_new_tab("doctor", None);
}

fn run_kaku_subcommand_in_new_tab(subcommand: &str, running_flag: Option<&'static AtomicBool>) {
    let subcommand = subcommand.to_string();
    let kaku_bin = kaku_cli_program_for_spawn();
    let fallback_bin = shlex::try_quote(&kaku_bin)
        .map(|q| q.into_owned())
        .unwrap_or_else(|_| kaku_bin.clone());

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let shell_name = Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    // Use -ic (interactive) so the shell sources rc files (where proxy env vars
    // like http_proxy/ALL_PROXY are typically set), but skip -l (login) which
    // loads the full profile and is much slower.
    let command_str = if shell_name == "fish" {
        format!(
            "{fallback_bin} {subcommand}; printf '\\nPress Enter to close...\\n'; read -l dummy"
        )
    } else {
        format!("{fallback_bin} {subcommand}; printf '\\nPress Enter to close...\\n'; read dummy")
    };

    let flag = running_flag.map(|f| f as *const AtomicBool as usize);

    promise::spawn::spawn_into_main_thread(async move {
        use crate::spawn::SpawnWhere;
        use config::keyassignment::{SpawnCommand, SpawnTabDomain};
        use std::sync::Arc;

        let config = fast_config_snapshot();
        let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi());
        let size = config.initial_size(dpi as u32, None);
        let term_config = Arc::new(config::TermConfig::with_config(config));

        // Find an existing window to add the tab to, so we don't spawn
        // a separate window that lacks the user's proxy environment.
        let src_window_id =
            try_front_end().and_then(|fe| fe.gui_windows().first().map(|w| w.mux_window_id));

        let spawn_cmd = SpawnCommand {
            domain: SpawnTabDomain::DomainName("local".to_string()),
            args: Some(vec![shell, "-ic".to_string(), command_str]),
            ..Default::default()
        };

        crate::spawn::spawn_command_impl(
            &spawn_cmd,
            SpawnWhere::NewTab,
            size,
            src_window_id,
            term_config,
        );

        // Clear the running flag once the tab spawn has been issued.
        // This does not wait for the subcommand to finish — it only prevents
        // rapid duplicate menu clicks from opening a second tab before the
        // first one has been created. spawn_command_impl has no completion
        // callback, so finer-grained tracking would require hooking tab close.
        if let Some(flag_addr) = flag {
            // SAFETY: flag_addr was cast from a `&'static AtomicBool`, so the
            // pointed-to value is guaranteed to live for the duration of the
            // process. The cast round-trip through usize is needed because
            // async closures require `'static` captures, and raw pointers are
            // `Send` while `&'static AtomicBool` is not capture-friendly across
            // the spawn boundary without an explicit `move`.
            let flag_ref = unsafe { &*(flag_addr as *const AtomicBool) };
            flag_ref.store(false, Ordering::SeqCst);
        }
    })
    .detach();
}

pub fn set_default_terminal_with_feedback() {
    fn show_window_toast(message: &str) -> bool {
        let windows = front_end().gui_windows();
        if windows.is_empty() {
            return false;
        }

        for gui in windows {
            let text = message.to_string();
            gui.window
                .notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.show_toast(text);
                })));
        }

        true
    }

    match Connection::get() {
        Some(conn) => match conn.set_default_terminal() {
            Ok(()) => {
                let message = "Kaku is now the default terminal";
                if !show_window_toast(message) {
                    conn.alert("Default Terminal", message);
                }
            }
            Err(err) => {
                let message = format!("Failed to set Kaku as default terminal: {err:#}");
                log::error!("{message}");
                if !show_window_toast("Failed to set default terminal") {
                    conn.alert("Default Terminal", &message);
                }
            }
        },
        None => {
            log::error!("Cannot set default terminal because no GUI connection is available");
        }
    }
}

impl GuiFrontEnd {
    pub fn try_new() -> anyhow::Result<Rc<GuiFrontEnd>> {
        let connection = Connection::init()?;
        connection.set_event_handler(Self::app_event_handler);
        connection.flush_pending_service_events();
        ::window::connection::mark_app_event_handler_ready();
        connection.flush_pending_service_events();

        let mux = Mux::get();
        let client_id = mux.active_identity().expect("to have set my own id");

        let front_end = Rc::new(GuiFrontEnd {
            connection,
            switching_workspaces: RefCell::new(false),
            spawned_mux_window: RefCell::new(HashSet::new()),
            known_windows: RefCell::new(BTreeMap::new()),
            client_id: client_id.clone(),
            config_subscription: RefCell::new(None),
            unread_bell_count: RefCell::new(0),
        });

        mux.subscribe(move |n| {
            match n {
                MuxNotification::WorkspaceRenamed {
                    old_workspace,
                    new_workspace,
                } => {
                    let mux = Mux::get();
                    let active = mux.active_workspace();
                    if active == old_workspace || active == new_workspace {
                        let switcher = WorkspaceSwitcher::new(&new_workspace);
                        promise::spawn::spawn_into_main_thread(async move {
                            drop(switcher);
                        })
                        .detach();
                    }
                }
                MuxNotification::WindowWorkspaceChanged(_)
                | MuxNotification::ActiveWorkspaceChanged(_)
                | MuxNotification::WindowCreated(_)
                | MuxNotification::WindowRemoved(_) => {
                    promise::spawn::spawn_into_main_thread(async move {
                        let fe = crate::frontend::front_end();
                        if !fe.is_switching_workspace() {
                            fe.reconcile_workspace();
                        }
                    })
                    .detach();
                }
                MuxNotification::PaneFocused(pane_id) => {
                    promise::spawn::spawn_into_main_thread(async move {
                        let mux = Mux::get();
                        if let Err(err) = mux.focus_pane_and_containing_tab(pane_id) {
                            log::error!("Error reconciling PaneFocused notification: {err:#}");
                        }
                    })
                    .detach();
                }
                MuxNotification::TabTitleChanged { .. } => {}
                MuxNotification::WindowTitleChanged { .. } => {}
                MuxNotification::TabResized(tab_id) => {
                    let mux = Mux::get();
                    if let Some(window_id) = mux.window_containing_tab(tab_id) {
                        crate::session_restore::request_save_window_snapshot(window_id);
                    }
                }
                MuxNotification::TabAddedToWindow { window_id, .. } => {
                    crate::session_restore::request_save_window_snapshot(window_id);
                }
                MuxNotification::PaneRemoved(_) => {}
                MuxNotification::WindowInvalidated(_) => {}
                MuxNotification::PaneOutput(_) => {}
                MuxNotification::PaneAdded(pane_id) => {
                    let mux = Mux::get();
                    if let Some((_, window_id, _)) = mux.resolve_pane_id(pane_id) {
                        crate::session_restore::request_save_window_snapshot(window_id);
                    }
                }
                MuxNotification::Alert {
                    pane_id,
                    alert:
                        Alert::ToastNotification {
                            title,
                            body,
                            focus: _,
                        },
                } => {
                    let mux = Mux::get();

                    if let Some((_domain, window_id, tab_id)) = mux.resolve_pane_id(pane_id) {
                        let config = config::configuration();

                        if let Some((_fdomain, f_window, f_tab, f_pane)) =
                            mux.resolve_focused_pane(&client_id)
                        {
                            let show = match config.notification_handling {
                                NotificationHandling::NeverShow => false,
                                NotificationHandling::AlwaysShow => true,
                                NotificationHandling::SuppressFromFocusedPane => f_pane != pane_id,
                                NotificationHandling::SuppressFromFocusedTab => f_tab != tab_id,
                                NotificationHandling::SuppressFromFocusedWindow => {
                                    f_window != window_id
                                }
                            };

                            if show {
                                let message = if title.is_none() { "" } else { &body };
                                let title = title.as_ref().unwrap_or(&body);
                                // FIXME: if notification.focus is true, we should do
                                // something here to arrange to focus pane_id when the
                                // notification is clicked
                                persistent_toast_notification(title, message);
                            }
                        }
                    }
                }
                MuxNotification::Alert {
                    pane_id: _,
                    alert: Alert::Bell | Alert::Progress(_),
                } => {
                    // Handled via TermWindowNotif; NOP it here.
                }
                MuxNotification::Alert {
                    pane_id: _,
                    alert:
                        Alert::OutputSinceFocusLost
                        | Alert::PaletteChanged
                        | Alert::CurrentWorkingDirectoryChanged
                        | Alert::WindowTitleChanged(_)
                        | Alert::TabTitleChanged(_)
                        | Alert::IconTitleChanged(_)
                        | Alert::SetUserVar { .. },
                } => {}
                MuxNotification::Empty => {
                    #[cfg(target_os = "macos")]
                    {
                        // Keep the app process alive on macOS when the last
                        // window closes, so Dock reopen is instant and consistent.
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        if config::configuration().quit_when_all_windows_are_closed {
                            promise::spawn::spawn_into_main_thread(async move {
                                if mux::activity::Activity::count() == 0 {
                                    log::trace!("Mux is now empty, terminate gui");
                                    if let Some(conn) = Connection::get() {
                                        conn.terminate_message_loop();
                                    } else {
                                        log::warn!(
                                            "Cannot terminate message loop: GUI connection is not initialized"
                                        );
                                    }
                                }
                            })
                            .detach();
                        }
                    }
                }
                MuxNotification::SaveToDownloads { name, data } => {
                    if !config::configuration().allow_download_protocols {
                        log::error!(
                            "Ignoring download request for {:?}, \
                                 as allow_download_protocols=false",
                            name
                        );
                    } else if let Err(err) = crate::download::save_to_downloads(name, &*data) {
                        log::error!("save_to_downloads: {:#}", err);
                    }
                }
                MuxNotification::AssignClipboard {
                    pane_id,
                    selection,
                    clipboard,
                } => {
                    promise::spawn::spawn_into_main_thread(async move {
                        let fe = crate::frontend::front_end();
                        log::trace!(
                            "set clipboard in pane {} {:?} {:?}",
                            pane_id,
                            selection,
                            clipboard
                        );
                        if let Some(window) = fe.known_windows.borrow().keys().next() {
                            window.set_clipboard(
                                match selection {
                                    ClipboardSelection::Clipboard => Clipboard::Clipboard,
                                    ClipboardSelection::PrimarySelection => {
                                        Clipboard::PrimarySelection
                                    }
                                },
                                clipboard.map(|s| s.to_string()).unwrap_or_default(),
                            );
                        } else {
                            log::error!("Cannot assign clipboard as there are no windows");
                        };
                    })
                    .detach();
                }
            }
            true
        });
        // Re-evaluate config only if it queried `wezterm.gui.get_appearance()`
        // before the GUI connection was ready during initial config load.
        if window_funcs::take_appearance_queried_before_gui_ready() {
            config::reload();
        }
        refresh_fast_config_snapshot();

        // Build the initial menubar synchronously so AppKit has selectors
        // registered before users hit menu actions or key equivalents.
        crate::commands::CommandDef::recreate_menubar(&config::configuration());
        front_end.connection.sync_global_hotkey();

        Ok(front_end)
    }

    fn spawn_open_command_script(file_name: String, prefer_existing_window: bool) {
        let is_directory = Path::new(&file_name).is_dir();
        let quoted_file_name = if is_directory {
            None
        } else {
            match shlex::try_quote(&file_name) {
                Ok(name) => Some(name.into_owned()),
                Err(_) => {
                    log::error!(
                        "OpenCommandScript: {file_name} has embedded NUL bytes and
                         cannot be launched via the shell"
                    );
                    return;
                }
            }
        };

        promise::spawn::spawn(async move {
            use config::keyassignment::SpawnTabDomain;
            use wezterm_term::TerminalSize;

            // We send the script to execute to the shell on stdin, rather than ask the
            // shell to execute it directly, so that we start the shell and read in the
            // user's rc files before running the script.  Without this, wezterm on macOS
            // is launched with a default and very anemic path, and that is frustrating for
            // users.

            let mux = Mux::get();
            let workspace = mux.active_workspace();
            let window_id = if prefer_existing_window {
                let mut windows = mux.iter_windows_in_workspace(&workspace);
                windows.pop()
            } else {
                None
            };
            let pane_id = None;
            let cmd = None;
            let cwd = if is_directory {
                Some(file_name.clone())
            } else {
                None
            };

            match mux
                .spawn_tab_or_window(
                    window_id,
                    SpawnTabDomain::DomainName("local".to_string()),
                    cmd,
                    cwd,
                    None,
                    TerminalSize::default(),
                    pane_id,
                    workspace,
                    None, // optional position
                )
                .await
            {
                Ok((_tab, pane, _window_id)) => {
                    if let Some(quoted_file_name) = quoted_file_name {
                        log::trace!("Spawned {file_name} as pane_id {}", pane.pane_id());
                        let mut writer = pane.writer();
                        if let Err(err) = write!(writer, "{quoted_file_name} ; exit\n") {
                            log::warn!("failed to send spawned command to pane: {err:#}");
                        }
                    } else {
                        log::trace!("Spawned pane_id {} with cwd={file_name}", pane.pane_id());
                    }
                }
                Err(err) => {
                    log::error!("Failed to spawn {file_name}: {err:#?}");
                }
            };
        })
        .detach();
    }
    fn activate_tab_for_tty(tty_name: String) {
        let tty_name = tty_name.trim().to_string();
        if tty_name.is_empty() {
            log::warn!("ActivatePaneForTty called with empty tty");
            return;
        }

        let mut tty_candidates = vec![tty_name.clone()];
        if let Some(stripped) = tty_name.strip_prefix("/dev/") {
            tty_candidates.push(stripped.to_string());
        } else {
            tty_candidates.push(format!("/dev/{tty_name}"));
        }
        tty_candidates.sort();
        tty_candidates.dedup();

        let target_basename = Path::new(&tty_name)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string());

        let mux = Mux::get();
        let pane_id = mux.iter_panes().into_iter().find_map(|pane| {
            let pane_tty = pane.tty_name()?;
            if tty_candidates
                .iter()
                .any(|candidate| candidate == &pane_tty)
            {
                return Some(pane.pane_id());
            }

            if let Some(target_basename) = target_basename.as_deref() {
                let pane_basename = Path::new(&pane_tty)
                    .file_name()
                    .and_then(|name| name.to_str());
                if pane_basename == Some(target_basename) {
                    return Some(pane.pane_id());
                }
            }

            None
        });

        let Some(pane_id) = pane_id else {
            log::warn!("No pane found for tty={tty_name}");
            return;
        };

        if let Err(err) = mux.focus_pane_and_containing_tab(pane_id) {
            log::error!("Failed to focus pane {pane_id} for tty={tty_name}: {err:#}");
            return;
        }

        if let Some((_domain, window_id, _tab_id)) = mux.resolve_pane_id(pane_id) {
            if let Some(fe) = try_front_end() {
                if let Some(gui_window) = fe.gui_window_for_mux_window(window_id) {
                    gui_window.window.focus();
                }
            }
        }
    }

    fn app_event_handler(event: ApplicationEvent) {
        match event {
            ApplicationEvent::OpenCommandScript(file_name) => {
                Self::spawn_open_command_script(file_name, false);
            }
            ApplicationEvent::OpenCommandScriptInTab(file_name) => {
                Self::spawn_open_command_script(file_name, true);
            }
            ApplicationEvent::ActivatePaneForTty(tty_name) => {
                Self::activate_tab_for_tty(tty_name);
            }
            ApplicationEvent::PerformKeyAssignment(action) => {
                // We should only get here when there are no windows open
                // and the user picks an action from the menubar.
                // This is not currently possible, but could be in the
                // future.

                fn spawn_command(spawn: &SpawnCommand, spawn_where: SpawnWhere) {
                    let config = fast_config_snapshot();
                    let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi());
                    // Keep this path cheap when no GUI window exists yet:
                    // avoid font metric resolution here and let the window layer
                    // apply final geometry/pixel sizing.
                    let size = config.initial_size(dpi as u32, None);
                    let term_config = Arc::new(config::TermConfig::with_config(config));

                    crate::spawn::spawn_command_impl(spawn, spawn_where, size, None, term_config);
                }

                match action {
                    KeyAssignment::EmitEvent(event)
                        if event == "update-kaku" || event == "run-kaku-update" =>
                    {
                        run_kaku_update_from_menu();
                    }
                    KeyAssignment::EmitEvent(event) if event == "run-kaku-cli" => {
                        let kaku_cli = kaku_cli_program_for_spawn();
                        spawn_command(
                            &SpawnCommand {
                                args: Some(vec![kaku_cli]),
                                ..Default::default()
                            },
                            SpawnWhere::NewWindow,
                        );
                    }
                    KeyAssignment::EmitEvent(event) if event == "open-kaku-config" => {
                        open_kaku_config();
                    }
                    KeyAssignment::EmitEvent(event) if event == SET_DEFAULT_TERMINAL_EVENT => {
                        set_default_terminal_with_feedback();
                    }
                    KeyAssignment::RestorePreviousWindow => {
                        crate::session_restore::restore_previous_window_from_menu();
                    }
                    KeyAssignment::ReloadConfiguration => {
                        // Manual reload is intentionally disabled.
                    }
                    KeyAssignment::QuitApplication => {
                        // If we get here, there are no windows that could have received
                        // the QuitApplication command, therefore it must be ok to quit
                        // immediately
                        #[cfg(target_os = "macos")]
                        {
                            ::window::request_terminate(
                                ::window::QuitOrigin::AppScopeQuitApplication,
                            );
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            if let Some(conn) = Connection::get() {
                                conn.terminate_message_loop();
                            } else {
                                log::warn!(
                                    "Cannot terminate message loop for QuitApplication: GUI connection is not initialized"
                                );
                            }
                        }
                    }
                    KeyAssignment::SpawnWindow => {
                        spawn_command(&SpawnCommand::default(), SpawnWhere::NewWindow);
                    }
                    KeyAssignment::SpawnTab(spawn_where) => {
                        spawn_command(
                            &SpawnCommand {
                                domain: spawn_where,
                                ..Default::default()
                            },
                            SpawnWhere::NewWindow,
                        );
                    }
                    KeyAssignment::SpawnCommandInNewTab(spawn) => {
                        spawn_command(&spawn, SpawnWhere::NewTab);
                    }
                    KeyAssignment::SpawnCommandInNewWindow(spawn) => {
                        spawn_command(&spawn, SpawnWhere::NewWindow);
                    }
                    _ => {
                        // Try to forward window-scoped actions (like
                        // ShowDebugOverlay) to the first available GUI
                        // window so they open in-place instead of being
                        // silently dropped.
                        if let Some(fe) = try_front_end() {
                            if let Some(gui) = fe.gui_windows().first() {
                                gui.window
                                    .notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                        if let Some(pane) = tw.get_active_pane_or_overlay() {
                                            if let Err(e) =
                                                tw.perform_key_assignment(&pane, &action)
                                            {
                                                log::error!(
                                                    "forwarded perform_key_assignment failed: {e:#}"
                                                );
                                            }
                                        }
                                    })));
                                return;
                            }
                        }
                        log::warn!("unhandled perform: {action:?}");
                    }
                }
            }
        }
    }

    pub fn run_forever(&self) -> anyhow::Result<()> {
        self.connection
            .run_message_loop()
            .context("running message loop")
    }

    pub fn gui_windows(&self) -> Vec<GuiWin> {
        let windows = self.known_windows.borrow();
        let mut windows: Vec<GuiWin> = windows
            .iter()
            .map(|(window, &mux_window_id)| GuiWin {
                mux_window_id,
                window: window.clone(),
            })
            .collect();
        windows.sort_by(|a, b| a.window.cmp(&b.window));
        windows
    }

    pub fn reconcile_workspace(&self) -> Future<()> {
        let mut promise = Promise::new();
        let mux = Mux::get();
        let workspace = mux.active_workspace_for_client(&self.client_id);

        if mux.is_workspace_empty(&workspace) {
            // We don't want to silently kill off things that might
            // be running in other workspaces, so let's pick one
            // and activate it
            if self.is_switching_workspace() {
                promise.ok(());
                return promise.get_future().unwrap();
            }
            for workspace in mux.iter_workspaces() {
                if !mux.is_workspace_empty(&workspace) {
                    mux.set_active_workspace_for_client(&self.client_id, &workspace);
                    log::debug!("using {} instead, as it is not empty", workspace);
                    break;
                }
            }
        }

        let workspace = mux.active_workspace_for_client(&self.client_id);
        log::debug!("workspace is {}, fixup windows", workspace);

        let mut mux_windows = mux.iter_windows_in_workspace(&workspace);

        // First, repurpose existing windows.
        // Note that both iter_windows_in_workspace and self.known_windows have a
        // deterministic iteration order, so switching back and forth should result
        // in a consistent mux <-> gui window mapping.
        let known_windows = std::mem::take(&mut *self.known_windows.borrow_mut());
        let mut windows = BTreeMap::new();
        let mut unused = BTreeMap::new();

        for (window, window_id) in known_windows.into_iter() {
            if let Some(idx) = mux_windows.iter().position(|&id| id == window_id) {
                // it already points to the desired mux window
                windows.insert(window, window_id);
                mux_windows.remove(idx);
            } else {
                unused.insert(window, window_id);
            }
        }

        let mut mux_windows = mux_windows.into_iter();

        for (window, old_id) in unused.into_iter() {
            if let Some(mux_window_id) = mux_windows.next() {
                window.notify(TermWindowNotif::SwitchToMuxWindow(mux_window_id));
                windows.insert(window, mux_window_id);
            } else {
                // We have more windows than are in the new workspace;
                // we no longer need this one!
                window.close();
                front_end().spawned_mux_window.borrow_mut().remove(&old_id);
            }
        }

        log::trace!("reconcile: windows -> {:?}", windows);
        *self.known_windows.borrow_mut() = windows;

        let future = promise.get_future().unwrap();

        // then spawn any new windows that are needed
        promise::spawn::spawn(async move {
            while let Some(mux_window_id) = mux_windows.next() {
                if front_end().has_mux_window(mux_window_id)
                    || front_end()
                        .spawned_mux_window
                        .borrow()
                        .contains(&mux_window_id)
                {
                    continue;
                }
                front_end()
                    .spawned_mux_window
                    .borrow_mut()
                    .insert(mux_window_id);
                log::trace!("Creating TermWindow for mux_window_id={}", mux_window_id);
                if let Err(err) = TermWindow::new_window(mux_window_id).await {
                    let err_text = format!("{:#}", err);
                    log::error!("Failed to create window: {:#}", err);
                    if err_text.contains("failed to create NSOpenGLPixelFormat") {
                        log::error!(
                            "OpenGL initialization failed. This often means no compatible GPU renderer is available (for example in some VMs). Try setting `front_end = 'WebGpu'` in kaku.lua or enabling VM GPU acceleration."
                        );
                    }
                    let mux = Mux::get();
                    mux.kill_window(mux_window_id);
                    front_end()
                        .spawned_mux_window
                        .borrow_mut()
                        .remove(&mux_window_id);
                }
            }
            *front_end().switching_workspaces.borrow_mut() = false;
            promise.ok(());
        })
        .detach();
        future
    }

    fn has_mux_window(&self, mux_window_id: MuxWindowId) -> bool {
        for &mux_id in self.known_windows.borrow().values() {
            if mux_id == mux_window_id {
                return true;
            }
        }
        false
    }

    pub fn switch_workspace(&self, workspace: &str) {
        let mux = Mux::get();
        mux.set_active_workspace_for_client(&self.client_id, workspace);
        *self.switching_workspaces.borrow_mut() = false;
        self.reconcile_workspace();
    }

    pub fn record_known_window(&self, window: Window, mux_window_id: MuxWindowId) {
        self.known_windows
            .borrow_mut()
            .insert(window, mux_window_id);
        if !self.is_switching_workspace() {
            self.reconcile_workspace();
        }
    }

    pub fn forget_known_window(&self, window: &Window) {
        self.known_windows.borrow_mut().remove(window);
        if !self.is_switching_workspace() {
            self.reconcile_workspace();
        }
    }

    fn update_unread_bell_badge(&self, current: usize) {
        // Always update badge: show count if enabled, clear otherwise
        if config::configuration().bell_dock_badge && current > 0 {
            self.connection.set_dock_badge(Some(&current.to_string()));
        } else {
            self.connection.set_dock_badge(None);
        }
    }

    /// Adjust the global unread bell count and update Dock badge.
    /// Pass positive value to increment, negative to decrement.
    pub fn adjust_unread_bell_count(&self, delta: isize) {
        let mut count = self.unread_bell_count.borrow_mut();
        if delta > 0 {
            *count = count.saturating_add(delta as usize);
        } else {
            *count = count.saturating_sub((-delta) as usize);
        }
        let current = *count;
        drop(count);

        self.update_unread_bell_badge(current);
    }

    /// Re-evaluate Dock badge visibility using the current unread count.
    pub fn sync_unread_bell_badge(&self) {
        let current = *self.unread_bell_count.borrow();
        self.update_unread_bell_badge(current);
    }

    pub fn is_switching_workspace(&self) -> bool {
        *self.switching_workspaces.borrow()
    }

    pub fn gui_window_for_mux_window(&self, mux_window_id: MuxWindowId) -> Option<GuiWin> {
        let windows = self.known_windows.borrow();
        for (window, v) in windows.iter() {
            if *v == mux_window_id {
                return Some(GuiWin {
                    mux_window_id,
                    window: window.clone(),
                });
            }
        }
        None
    }

    pub fn focused_mux_window_id(&self) -> Option<MuxWindowId> {
        let mux = Mux::get();
        mux.resolve_focused_pane(&self.client_id)
            .map(|(_, window_id, _, _)| window_id)
            .or_else(|| self.gui_windows().first().map(|w| w.mux_window_id))
    }
}

thread_local! {
    static FRONT_END: RefCell<Option<Rc<GuiFrontEnd>>> = RefCell::new(None);
}

pub fn try_front_end() -> Option<Rc<GuiFrontEnd>> {
    FRONT_END.with(|f| f.borrow().as_ref().map(Rc::clone))
}

pub fn front_end() -> Rc<GuiFrontEnd> {
    FRONT_END
        .with(|f| f.borrow().as_ref().map(Rc::clone))
        .expect("to be called on gui thread")
}

pub struct WorkspaceSwitcher {
    new_name: String,
}

impl WorkspaceSwitcher {
    pub fn new(new_name: &str) -> Self {
        *front_end().switching_workspaces.borrow_mut() = true;
        Self {
            new_name: new_name.to_string(),
        }
    }

    pub fn do_switch(self) {
        // Drop is invoked, which will complete the switch
    }
}

impl Drop for WorkspaceSwitcher {
    fn drop(&mut self) {
        front_end().switch_workspace(&self.new_name);
    }
}

pub fn shutdown() {
    FRONT_END.with(|f| drop(f.borrow_mut().take()));
}

pub fn try_new() -> Result<Rc<GuiFrontEnd>, Error> {
    let front_end = GuiFrontEnd::try_new()?;
    FRONT_END.with(|f| *f.borrow_mut() = Some(Rc::clone(&front_end)));

    let config_subscription = config::subscribe_to_config_reload({
        move || {
            // This callback may run while the config mutex is held;
            // refresh asynchronously to avoid re-locking config here.
            promise::spawn::spawn_into_main_thread(async {
                refresh_fast_config_snapshot();
                if let Some(conn) = Connection::get() {
                    conn.sync_global_hotkey();
                }
            })
            .detach();
            // TODO(macos): AppKit does not allow safe async menubar reconstruction
            // from a config-reload callback; the initial menubar is built synchronously
            // in try_new(). Re-enable on macOS once a safe main-thread dispatch path
            // is available.
            #[cfg(not(target_os = "macos"))]
            {
                promise::spawn::spawn_into_main_thread(async {
                    crate::commands::CommandDef::recreate_menubar(&config::configuration());
                })
                .detach();
            }
            true
        }
    });
    front_end
        .config_subscription
        .borrow_mut()
        .replace(config_subscription);

    Ok(front_end)
}
