use crate::scripting::guiwin::GuiWin;
use crate::spawn::SpawnWhere;
use crate::termwindow::TermWindowNotif;
use crate::{startup_trace, TermWindow};
use ::window::*;
use anyhow::{Context, Error};
use config::keyassignment::{KeyAssignment, SpawnCommand, SpawnTabDomain};
use config::{ConfigSubscription, NotificationHandling};
use mux::client::ClientId;
use mux::pane::PaneId;
use mux::tab::TabId;
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use promise::{Future, Promise};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
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

/// The bundled CLI as a single shell-quoted token, for typing into an
/// interactive shell (menu actions). Spawn paths take the unquoted program
/// via `kaku_cli_program_for_spawn` instead.
pub(crate) fn kaku_cli_shell_invocation() -> String {
    let kaku_bin = kaku_cli_program_for_spawn();
    shell_quote_program(&kaku_bin)
}

fn shell_quote_program(program: &str) -> String {
    shlex::try_quote(program)
        .map(|q| q.into_owned())
        .unwrap_or_else(|_| program.to_string())
}

struct SingletonState {
    window_id: MuxWindowId,
    pending: bool,
}

static SINGLETON_WINDOWS: LazyLock<Mutex<HashMap<&'static str, SingletonState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Open or focus a singleton window identified by `namespace`.
/// If a window for the given namespace already exists, focus it.
/// Otherwise, run `handler` to create one and track it by namespace.
fn ensure_singleton_window<F, Fut>(namespace: &'static str, handler: F)
where
    F: FnOnce() -> Fut + 'static,
    Fut: std::future::Future<Output = anyhow::Result<MuxWindowId>> + 'static,
{
    // Atomic check-and-mark: if window exists, focus and return;
    // otherwise mark as pending to prevent duplicate spawns.
    {
        let mut guard = SINGLETON_WINDOWS.lock().unwrap();
        if let Some(state) = guard.get(namespace) {
            if state.pending {
                return;
            }
            if Mux::get().get_window(state.window_id).is_some() {
                if let Some(fe) = try_front_end() {
                    if let Some(gw) = fe.gui_window_for_mux_window(state.window_id) {
                        gw.window.focus();
                        return;
                    }
                }
            }
        }
        guard.insert(
            namespace,
            SingletonState {
                window_id: 0,
                pending: true,
            },
        );
    }

    promise::spawn::spawn(async move {
        match handler().await {
            Ok(window_id) => {
                let mut guard = SINGLETON_WINDOWS.lock().unwrap();
                if let Some(state) = guard.get_mut(namespace) {
                    state.window_id = window_id;
                    state.pending = false;
                }
            }
            Err(err) => {
                log::error!("singleton window '{namespace}' error: {:#}", err);
                SINGLETON_WINDOWS.lock().unwrap().remove(namespace);
            }
        }
    })
    .detach();
}

fn kaku_config_command(kaku_bin: String, config_file: Option<&Path>) -> Vec<String> {
    let mut args = vec![kaku_bin];
    if let Some(path) = config_file {
        // Settings must edit the file that this GUI loads, even when its new
        // window starts in a different working directory.
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        args.push("--config-file".to_string());
        args.push(path.to_string_lossy().into_owned());
    }
    args.push("config".to_string());
    args
}

pub fn open_kaku_config() {
    let kaku_bin = kaku_cli_program_for_spawn();
    ensure_singleton_window("kaku-config", async move || {
        let config = fast_config_snapshot();
        let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi());
        let size = config.initial_size(dpi as u32, None);
        let term_config = Arc::new(config::TermConfig::with_config(config));
        crate::spawn::spawn_command_internal(
            SpawnCommand {
                domain: SpawnTabDomain::DomainName("local".to_string()),
                args: Some(kaku_config_command(
                    kaku_bin,
                    config::config_file_override().as_deref(),
                )),
                ..Default::default()
            },
            // Keep settings isolated from active coding tabs so ESC inside
            // config does not race with focus return on tab close.
            SpawnWhere::NewWindow,
            size,
            None,
            term_config,
        )
        .await
    });
}

// Update entry matrix (every user-facing path must confirm before app replace):
// - Toast click              -> confirm_and_apply_update (overlay)
// - Menu Restart to Update  -> confirm_and_apply_update (overlay)
// - Menu Check (staged)     -> confirm_and_apply_update (overlay)
// - Menu Check (download)   -> run_kaku_update_in_tab(auto_confirm=false) -> CLI prompt
// - Overlay confirmed       -> apply_update_now -> staged restart or tab(auto_confirm=true)
// - CLI direct / brew       -> confirm_apply_update in kaku/src/update.rs
// AUTO_CONFIRM is set only after overlay confirm, never for exploratory menu check.

/// Menu "Check for Updates...". When a staged package is already ready, ask
/// before restarting (same overlay as toast). Otherwise open a tab that runs
/// `kaku update` interactively so the CLI can prompt before install.
pub fn check_for_updates_from_menu() {
    if crate::update::staged_update_available().is_some() {
        confirm_and_apply_update();
    } else {
        run_kaku_update_in_tab(/* auto_confirm */ false);
    }
}

/// Open a terminal tab running `kaku update` without pre-confirming install.
/// Used by the menu check path and by staged-update failure fallbacks.
pub fn run_kaku_update_from_menu() {
    run_kaku_update_in_tab(/* auto_confirm */ false);
}

fn run_kaku_update_in_tab(auto_confirm: bool) {
    static UPDATE_RUNNING: AtomicBool = AtomicBool::new(false);
    if UPDATE_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("run_kaku_update_in_tab: update already running, ignoring");
        return;
    }
    run_kaku_subcommand_in_new_tab("update", Some(&UPDATE_RUNNING), auto_confirm);
}

/// Entry point for toast click and menu "Restart to Update". Routes to the
/// front GUI window and shows a confirmation overlay before anything
/// destructive happens, so a stray click can no longer kill running tasks.
/// Falls back to applying directly only when there is no GUI window to host
/// the overlay.
pub fn confirm_and_apply_update() {
    promise::spawn::spawn_into_main_thread(async move {
        if let Some(fe) = try_front_end() {
            if let Some(gui) = fe.gui_windows().first() {
                gui.window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                    tw.show_update_confirmation();
                })));
                return;
            }
        }
        apply_update_now();
    })
    .detach();
}

/// Apply the pending update now. Called only after the user confirms in the
/// update overlay (or as a fallback when no window can host the overlay).
/// Uses the staged fast-path when available, else the terminal-tab flow with
/// auto-confirm (user already approved the restart in the overlay).
pub(crate) fn apply_update_now() {
    if crate::update::staged_update_available().is_some() {
        restart_to_update();
    } else {
        run_kaku_update_in_tab(/* auto_confirm */ true);
    }
}

/// Apply a previously staged update by spawning the helper script directly,
/// without opening a terminal tab.
pub fn restart_to_update() {
    use crate::update::{
        cleanup_staged_update, resolve_target_app_path, spawn_update_helper,
        staged_update_available, write_update_helper_script,
    };

    // Toast click can fire twice (rapid double-click, or two GUI processes
    // both registering the click callback). Without this guard, two helper
    // scripts race to ditto into the same Kaku.app.
    static UPDATE_RUNNING: AtomicBool = AtomicBool::new(false);
    if UPDATE_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("restart_to_update: already running, ignoring");
        return;
    }

    // Reset the guard on any non-success exit so the user can retry. On
    // success the helper kills us before the function returns, so the guard
    // value no longer matters.
    let release_guard = || UPDATE_RUNNING.store(false, Ordering::SeqCst);

    fn fallback_with_toast(msg: &str) {
        log::error!("restart_to_update: {}", msg);
        wezterm_toast_notification::persistent_toast_notification(
            "Update Failed",
            "Automatic update failed. Trying manual update.",
        );
        run_kaku_update_from_menu();
    }

    let info = match staged_update_available() {
        Some(info) => info,
        None => {
            log::warn!("restart_to_update: no staged update available, falling back to menu flow");
            run_kaku_update_from_menu();
            release_guard();
            return;
        }
    };

    let target_app = match resolve_target_app_path() {
        Ok(p) => p,
        Err(e) => {
            fallback_with_toast(&format!("failed to resolve target app: {}", e));
            release_guard();
            return;
        }
    };

    let new_app = std::path::PathBuf::from(&info.app_path);
    // The work_dir for the helper script is the staged_update directory itself.
    let work_dir = config::DATA_DIR.join("staged_update");

    let update_root = config::DATA_DIR.join("updates");
    if let Err(e) = config::create_user_owned_dirs(&update_root) {
        fallback_with_toast(&format!("failed to create updates dir: {}", e));
        release_guard();
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let helper_script = update_root.join(format!("apply-staged-{}.sh", now));

    if let Err(e) = write_update_helper_script(&helper_script) {
        fallback_with_toast(&format!("failed to write helper script: {}", e));
        release_guard();
        return;
    }

    if let Err(e) = spawn_update_helper(&helper_script, &target_app, &new_app, &work_dir) {
        log::error!("restart_to_update: failed to spawn helper: {}", e);
        cleanup_staged_update();
        wezterm_toast_notification::persistent_toast_notification(
            "Update Failed",
            "Automatic update failed. Trying manual update.",
        );
        run_kaku_update_from_menu();
        release_guard();
        return;
    }

    log::info!(
        "restart_to_update: helper spawned for {} -> {}",
        info.tag,
        target_app.display()
    );
}

pub fn run_kaku_doctor_in_new_tab() {
    run_kaku_subcommand_in_new_tab("doctor", None, false);
}

fn run_kaku_subcommand_in_new_tab(
    subcommand: &str,
    running_flag: Option<&'static AtomicBool>,
    auto_confirm: bool,
) {
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
    // Use login + interactive shell (-lic / -l -i -c) to match the default Kaku
    // tab behavior (argv0 = "-zsh"). This ensures ~/.zprofile is sourced, where
    // macOS users typically export proxy variables (https_proxy, ALL_PROXY, etc.).
    // Without -l, the GUI process env (launched via launchd with a minimal
    // environment) is inherited and ~/.zprofile is never loaded, so curl hits
    // api.github.com without a proxy -- causing 30+ second timeouts on Chinese
    // networks. The extra few hundred ms of profile loading is negligible.
    // Only set KAKU_UPDATE_AUTO_CONFIRM after the user already confirmed in the
    // GUI overlay. Menu "Check for Updates" leaves this unset so the CLI still
    // prompts before replacing the app and quitting.
    let env_prefix = if subcommand == "update" && auto_confirm {
        "KAKU_UPDATE_AUTO_CONFIRM=1 "
    } else {
        ""
    };
    // After the subcommand finishes, sleep briefly so the user can read the
    // output before the helper script kills the process. No interactive
    // "Press Enter to close" prompt that leaves a dead tab.
    let shell_args = if shell_name == "fish" {
        vec![
            shell.clone(),
            "-l".to_string(),
            "-i".to_string(),
            "-c".to_string(),
            format!("{env_prefix}{fallback_bin} {subcommand}; sleep 2"),
        ]
    } else {
        vec![
            shell.clone(),
            "-lic".to_string(),
            format!("{env_prefix}{fallback_bin} {subcommand}; sleep 2"),
        ]
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
            args: Some(shell_args),
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
        // This does not wait for the subcommand to finish - it only prevents
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
        startup_trace::mark("  Connection::init() start");
        let connection = Connection::init()?;
        startup_trace::mark("  Connection::init() done");
        connection.set_event_handler(Self::app_event_handler);
        startup_trace::mark("  flush_pending_service_events #1 start");
        connection.flush_pending_service_events();
        startup_trace::mark("  flush_pending_service_events #1 done");
        ::window::connection::mark_app_event_handler_ready();
        startup_trace::mark("  flush_pending_service_events #2 start");
        connection.flush_pending_service_events();
        startup_trace::mark("  flush_pending_service_events #2 done");

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
            let n = n.clone();
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
                | MuxNotification::WindowCreated(_) => {
                    crate::session_restore::mark_dirty();
                    promise::spawn::spawn_into_main_thread(async move {
                        let fe = crate::frontend::front_end();
                        if !fe.is_switching_workspace() {
                            fe.reconcile_workspace();
                        }
                    })
                    .detach();
                }
                MuxNotification::WindowRemoved(window_id) => {
                    // Window is gone from mux for real (Linux close / quit).
                    // Drop any stale "logically closed" marker so it cannot
                    // pollute a future save iteration.
                    crate::session_restore::forget_logically_closed(window_id);
                    crate::session_restore::mark_dirty();
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
                MuxNotification::TabResized(_) => {
                    crate::session_restore::mark_dirty();
                }
                MuxNotification::TabAddedToWindow { .. } => {
                    crate::session_restore::mark_dirty();
                }
                MuxNotification::PaneRemoved(_) => {
                    crate::session_restore::mark_dirty();
                }
                MuxNotification::WindowInvalidated(_) => {}
                MuxNotification::PaneOutput(_) => {}
                MuxNotification::PaneAdded(_) => {
                    crate::session_restore::mark_dirty();
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
        startup_trace::mark("    mux.subscribe registered");
        // When `wezterm.gui.get_appearance()` is called during initial Lua
        // load, the GUI connection does not yet exist and the helper returns
        // `Appearance::Light` unconditionally. Re-parsing the whole Lua
        // config after the connection is ready costs ~38ms, so only do it
        // when the real appearance differs from that assumption.
        if window_funcs::take_appearance_queried_before_gui_ready() {
            let real_appearance = front_end.connection.get_appearance();
            if real_appearance != Appearance::Light {
                startup_trace::mark("    config::reload start (real appearance != Light)");
                config::reload();
                startup_trace::mark("    config::reload done");
            } else {
                startup_trace::mark("    config::reload skipped (Light)");
            }
        }
        refresh_fast_config_snapshot();
        startup_trace::mark("    refresh_fast_config_snapshot done");

        // Build the initial menubar synchronously so AppKit has its menu
        // hierarchy established before key events arrive. On macOS 26,
        // deferring this causes routeKeyEquivalent to swallow arrow keys
        // and Ctrl+C.
        startup_trace::mark("    recreate_menubar start");
        crate::commands::CommandDef::recreate_menubar(&config::configuration());
        startup_trace::mark("    recreate_menubar done");
        startup_trace::mark("    sync_global_hotkey start");
        front_end.connection.sync_global_hotkey();
        startup_trace::mark("    sync_global_hotkey done");

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
            Self::focus_gui_window(window_id);
        }
    }

    fn focus_gui_window(window_id: MuxWindowId) {
        if let Some(fe) = try_front_end() {
            if let Some(gui_window) = fe.gui_window_for_mux_window(window_id) {
                gui_window.window.focus();
            }
        }
    }

    fn activate_pane_by_id(pane_id: usize) {
        let pane_id = PaneId::new(pane_id);
        let mux = Mux::get();

        if mux.get_pane(pane_id).is_none() {
            log::warn!("ActivatePaneById called with unknown pane_id={pane_id}");
            return;
        }

        if let Err(err) = mux.focus_pane_and_containing_tab(pane_id) {
            log::error!("Failed to focus pane {pane_id}: {err:#}");
            return;
        }

        if let Some((_domain, window_id, _tab_id)) = mux.resolve_pane_id(pane_id) {
            Self::focus_gui_window(window_id);
        }
    }

    fn activate_tab_by_id(tab_id: usize) {
        let tab_id = TabId::new(tab_id);
        let mux = Mux::get();

        let Some(tab) = mux.get_tab(tab_id) else {
            log::warn!("ActivateTabById called with unknown tab_id={tab_id}");
            return;
        };

        let pane_id = tab
            .get_active_pane()
            .map(|pane| pane.pane_id())
            .or_else(|| tab.iter_panes().first().map(|pos| pos.pane.pane_id()));
        let Some(pane_id) = pane_id else {
            log::warn!("ActivateTabById found no panes in tab_id={tab_id}");
            return;
        };

        if let Err(err) = mux.focus_pane_and_containing_tab(pane_id) {
            log::error!("Failed to focus tab {tab_id} via pane {pane_id}: {err:#}");
            return;
        }

        if let Some(window_id) = mux.window_containing_tab(tab_id) {
            Self::focus_gui_window(window_id);
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
            ApplicationEvent::ActivatePaneById(pane_id) => {
                Self::activate_pane_by_id(pane_id);
            }
            ApplicationEvent::ActivateTabById(tab_id) => {
                Self::activate_tab_by_id(tab_id);
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
                        check_for_updates_from_menu();
                    }
                    KeyAssignment::EmitEvent(event) if event == "restart-to-update" => {
                        confirm_and_apply_update();
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
                // Rebuild on the main thread, outside the config lock, so
                // launch_menu and key binding edits reach the menubar (#570).
                crate::commands::CommandDef::recreate_menubar(&config::configuration());
            })
            .detach();
            true
        }
    });
    front_end
        .config_subscription
        .borrow_mut()
        .replace(config_subscription);

    Ok(front_end)
}

#[cfg(test)]
mod tests {
    use super::{kaku_config_command, shell_quote_program};
    use std::path::Path;

    #[test]
    fn initial_menubar_is_built_synchronously_without_appkit_windows_menu() {
        // Production source only, so the assertions cannot match themselves.
        let frontend = include_str!("frontend.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("frontend production source");
        let try_new = frontend
            .split("pub fn try_new() -> anyhow::Result<Rc<GuiFrontEnd>> {")
            .nth(1)
            .and_then(|body| body.split("        Ok(front_end)\n").next())
            .expect("try_new body");
        // Function-body indentation: a call moved into a spawned closure or a
        // deferred block would be indented deeper and lose early key events.
        assert!(
            try_new.contains(
                "\n        crate::commands::CommandDef::recreate_menubar(&config::configuration());\n"
            ),
            "the initial menubar must be built synchronously in try_new"
        );

        // AppKit must never own the Windows menu (dangling
        // NSWindowRepresentingMenuItem, PAC faults on macOS 26).
        for (name, source) in [
            ("commands.rs", include_str!("commands.rs")),
            ("menu.rs", include_str!("../../window/src/os/macos/menu.rs")),
        ] {
            assert!(
                !source.contains("setWindowsMenu_(") && !source.contains(", setWindowsMenu:"),
                "{} must not hand the Windows menu to AppKit",
                name
            );
        }
    }

    #[test]
    fn config_reload_rebuilds_menubar_on_macos_in_place() {
        // Production source only, so the assertions cannot match themselves.
        let frontend = include_str!("frontend.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("frontend production source");
        let reload = frontend
            .split("config::subscribe_to_config_reload({")
            .nth(1)
            .and_then(|body| body.split("\n            true\n").next())
            .expect("config reload callback");
        let spawned = reload
            .split("promise::spawn::spawn_into_main_thread(async {")
            .nth(1)
            .and_then(|body| body.split("            })\n            .detach();").next())
            .expect("main-thread task in the reload callback");
        // The rebuild runs inside the main-thread task (not in the callback,
        // which may hold the config lock) and on macOS too (#570).
        assert!(
            spawned.contains(
                "\n                crate::commands::CommandDef::recreate_menubar(&config::configuration());\n"
            ),
            "config reload must rebuild the menubar from the main-thread task"
        );
        assert!(
            !reload.contains("#[cfg(not(target_os = \"macos\"))]"),
            "config reload must not skip the menubar rebuild on macOS"
        );

        // A rebuild must reproduce the first build without dropping what
        // AppKit injected at launch (AutoFill, Start Dictation, Emoji &
        // Symbols in Edit): remove only the items Kaku tagged, keep the
        // existing menus, re-add the fixed Kaku menu items every time and
        // insert ahead of the injected items rather than append after them.
        let commands = include_str!("commands.rs");
        let rebuild = commands
            .split("pub fn recreate_menubar(config: &ConfigHandle) {")
            .nth(1)
            .and_then(|body| body.split("\n    }\n}\n").next())
            .expect("macOS recreate_menubar body");
        assert!(
            rebuild.contains(
                "if item.get_tag() == KAKU_MENU_ITEM_TAG {\n                                menu.remove_item(&item);\n"
            ),
            "a rebuild must remove only the menu items Kaku added"
        );
        assert!(
            !rebuild.contains(".remove_all_items();"),
            "emptying the main menu loses the Edit items AppKit adds only at launch"
        );
        assert!(
            rebuild.contains(
                "\n                if next_index == 0 {\n                    let menu = &submenu;\n"
            ) && rebuild.contains("insert_owned(menu, &about_item, &mut next_index);"),
            "the fixed Kaku menu items must be re-added on every build, not only when the menu is new"
        );
        assert!(
            rebuild.contains("insert_owned(&submenu, &item, &mut next_index);"),
            "command items must be inserted in rank order ahead of AppKit items"
        );
    }

    #[test]
    fn settings_command_preserves_custom_config_path() {
        assert_eq!(
            kaku_config_command("kaku".into(), Some(Path::new("/tmp/custom config.lua"))),
            vec!["kaku", "--config-file", "/tmp/custom config.lua", "config"]
        );
    }

    #[test]
    fn settings_command_keeps_default_config_resolution() {
        assert_eq!(
            kaku_config_command("kaku".into(), None),
            vec!["kaku", "config"]
        );
    }

    #[test]
    fn settings_command_resolves_relative_config_before_spawn() {
        let relative = Path::new("Cargo.toml");
        let args = kaku_config_command("kaku".into(), Some(relative));
        let expected = std::fs::canonicalize(relative).expect("existing fixture");
        assert_eq!(Path::new(&args[2]), expected);
        assert!(Path::new(&args[2]).is_absolute());
    }

    #[test]
    fn shell_program_with_spaces_round_trips_as_one_token() {
        let program = "/Applications/Kaku Nightly.app/Contents/MacOS/kaku";
        let quoted = shell_quote_program(program);
        assert_eq!(shlex::split(&quoted), Some(vec![program.to_string()]));
    }

    /// User-facing update events must not call `restart_to_update` directly.
    /// Regression for toast-only confirm (d9e8500e) + menu sibling (06cbdc00).
    #[test]
    fn user_facing_update_events_route_through_confirm() {
        let termwindow = include_str!("termwindow/mod.rs");
        let frontend = include_str!("frontend.rs");
        let update = include_str!("update.rs");

        // Window-scoped EmitEvent handlers.
        assert!(
            termwindow.contains("check_for_updates_from_menu()"),
            "run-kaku-update must go through check_for_updates_from_menu"
        );
        assert!(
            termwindow.contains("confirm_and_apply_update()"),
            "restart-to-update must go through confirm_and_apply_update"
        );
        // Direct restart from the event arm is the original bug.
        let restart_arm = termwindow
            .split("name == \"restart-to-update\"")
            .nth(1)
            .expect("restart-to-update arm");
        let restart_body = restart_arm.split("} else if name ==").next().unwrap();
        assert!(
            restart_body.contains("confirm_and_apply_update()"),
            "restart-to-update arm must confirm"
        );
        assert!(
            !restart_body.contains("restart_to_update()"),
            "restart-to-update arm must not call restart_to_update directly"
        );

        // Menubar path when no window has focus.
        assert!(
            frontend.contains("check_for_updates_from_menu()"),
            "menubar run-kaku-update must confirm-or-prompt"
        );
        let fe_restart = frontend
            .split("event == \"restart-to-update\"")
            .nth(1)
            .expect("frontend restart-to-update arm");
        let fe_restart_body = fe_restart.split("KeyAssignment::EmitEvent").next().unwrap();
        assert!(
            fe_restart_body.contains("confirm_and_apply_update()"),
            "frontend restart-to-update must confirm"
        );

        // Toast click callback.
        assert!(
            update.contains("confirm_and_apply_update()"),
            "toast update click must confirm"
        );

        // AUTO_CONFIRM only when auto_confirm is true (post-overlay).
        assert!(
            frontend.contains("if subcommand == \"update\" && auto_confirm"),
            "AUTO_CONFIRM must require auto_confirm flag, not every update tab"
        );
    }
}
