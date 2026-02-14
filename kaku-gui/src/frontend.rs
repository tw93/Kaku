use crate::scripting::guiwin::GuiWin;
use crate::spawn::SpawnWhere;
use crate::termwindow::TermWindowNotif;
use crate::TermWindow;
use ::window::*;
use anyhow::{Context, Error};
use config::keyassignment::{KeyAssignment, SpawnCommand};
use config::{ConfigSubscription, NotificationHandling};
use mux::client::ClientId;
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use promise::{Future, Promise};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;
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

pub fn open_kaku_config() {
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<()> {
            let current_exe = std::env::current_exe().context("resolve executable path")?;
            let exe_dir = current_exe
                .parent()
                .ok_or_else(|| anyhow::anyhow!("missing executable parent directory"))?;

            let kaku_bin = exe_dir.join("kaku");
            if !kaku_bin.exists() {
                anyhow::bail!("could not find kaku binary at {}", kaku_bin.display());
            }

            let ensure_status = Command::new(&kaku_bin)
                .arg("config")
                .arg("--ensure-only")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .with_context(|| format!("failed to launch {}", kaku_bin.display()))?;
            if !ensure_status.success() {
                anyhow::bail!(
                    "kaku config --ensure-only exited with status {}",
                    ensure_status.code().unwrap_or(-1)
                );
            }

            let home = std::env::var("HOME").context("resolve HOME for config path")?;
            let config_path = format!("{home}/.config/kaku/kaku.lua");
            let quoted_config_path =
                shlex::try_quote(&config_path).context("quote config path for shell open")?;
            let open_script = format!(
                "if command -v code >/dev/null 2>&1; then code -g {0}; else /usr/bin/open {0}; fi",
                quoted_config_path
            );
            let open_status = Command::new("/bin/sh")
                .arg("-lc")
                .arg(open_script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .context("open kaku.lua in editor")?;
            if !open_status.success() {
                anyhow::bail!("failed to open {}", config_path);
            }

            Ok(())
        })();

        if let Err(err) = result {
            let msg = format!("Failed to open settings: {:#}", err);
            log::error!("{}", msg);
            promise::spawn::spawn_into_main_thread(async move {
                if let Some(conn) = Connection::get() {
                    conn.alert("Settings", &msg);
                }
            })
            .detach();
        }
    });
}

pub fn set_default_terminal_with_feedback() {
    fn show_window_toast(message: &str) -> bool {
        let windows = front_end().gui_windows();
        if windows.is_empty() {
            return false;
        }

        for gui in windows {
            let text = message.to_string();
            gui.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
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

        let mux = Mux::get();
        let client_id = mux.active_identity().expect("to have set my own id");

        let front_end = Rc::new(GuiFrontEnd {
            connection,
            switching_workspaces: RefCell::new(false),
            spawned_mux_window: RefCell::new(HashSet::new()),
            known_windows: RefCell::new(BTreeMap::new()),
            client_id: client_id.clone(),
            config_subscription: RefCell::new(None),
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
                MuxNotification::TabResized(_) => {}
                MuxNotification::TabAddedToWindow { .. } => {}
                MuxNotification::PaneRemoved(_) => {}
                MuxNotification::WindowInvalidated(_) => {}
                MuxNotification::PaneOutput(_) => {}
                MuxNotification::PaneAdded(_) => {}
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
                                    Connection::get().unwrap().terminate_message_loop();
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
                                clipboard.unwrap_or_default(),
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

        // Build the initial menu bar synchronously during startup.
        // AppKit may inspect menu item selectors during reopen events,
        // so avoid deferring the first menubar reconstruction.
        crate::commands::CommandDef::recreate_menubar(&config::configuration());

        Ok(front_end)
    }

    fn app_event_handler(event: ApplicationEvent) {
        match event {
            ApplicationEvent::OpenCommandScript(file_name) => {
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
                    let window_id = None;
                    let pane_id = None;
                    let cmd = None;
                    let cwd = if is_directory {
                        Some(file_name.clone())
                    } else {
                        None
                    };
                    let workspace = mux.active_workspace();

                    match mux
                        .spawn_tab_or_window(
                            window_id,
                            SpawnTabDomain::DomainName("local".to_string()),
                            cmd,
                            cwd,
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
                                write!(writer, "{quoted_file_name} ; exit\n").ok();
                            } else {
                                log::trace!(
                                    "Spawned pane_id {} with cwd={file_name}",
                                    pane.pane_id()
                                );
                            }
                        }
                        Err(err) => {
                            log::error!("Failed to spawn {file_name}: {err:#?}");
                        }
                    };
                })
                .detach();
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
                        spawn_command(
                            &SpawnCommand {
                                args: Some(vec!["kaku".to_string(), "update".to_string()]),
                                ..Default::default()
                            },
                            SpawnWhere::NewWindow,
                        );
                    }
                    KeyAssignment::EmitEvent(event) if event == "run-kaku-cli" => {
                        spawn_command(
                            &SpawnCommand {
                                args: Some(vec!["kaku".to_string()]),
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
                        // Reload is handled by the active window showing a toast
                        // See termwindow/mod.rs ReloadConfiguration handling
                    }
                    KeyAssignment::QuitApplication => {
                        // If we get here, there are no windows that could have received
                        // the QuitApplication command, therefore it must be ok to quit
                        // immediately
                        Connection::get().unwrap().terminate_message_loop();
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
                    log::error!("Failed to create window: {:#}", err);
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
