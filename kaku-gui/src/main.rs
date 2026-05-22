// Don't create a new standard console window when launched from the windows GUI.
#![cfg_attr(not(test), windows_subsystem = "windows")]
#![allow(clippy::cast_abs_to_unsigned)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::assign_op_pattern)]
#![allow(clippy::borrow_deref_ref)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::from_over_into)]
#![allow(clippy::get_first)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::into_iter_on_ref)]
#![allow(clippy::io_other_error)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::legacy_numeric_constants)]
#![allow(clippy::len_zero)]
#![allow(clippy::let_and_return)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::manual_pattern_char_comparison)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_unwrap_or_default)]
#![allow(clippy::map_clone)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::missing_const_for_thread_local)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::needless_option_take)]
#![allow(clippy::neg_multiply)]
#![allow(clippy::needless_return)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::op_ref)]
#![allow(clippy::option_map_unit_fn)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::question_mark)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::redundant_field_names)]
#![allow(clippy::redundant_static_lifetimes)]
#![allow(clippy::result_large_err)]
#![allow(clippy::reserve_after_initialization)]
#![allow(clippy::search_is_some)]
#![allow(clippy::single_match)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::unnecessary_mut_passed)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::unneeded_struct_pattern)]
#![allow(clippy::vec_box)]
#![allow(clippy::while_let_on_iterator)]
#![allow(clippy::wildcard_in_or_patterns)]
#![allow(clippy::write_with_newline)]
#![allow(clippy::wrong_self_convention)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::useless_asref)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::useless_format)]

use crate::utilsprites::RenderMetrics;
use ::window::*;
use anyhow::{anyhow, Context};
use clap::builder::ValueParser;
use clap::{Parser, ValueHint};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::ConfigHandle;
use mux::activity::Activity;
use mux::domain::{Domain, LocalDomain};
use mux::Mux;
use mux_lua::MuxDomain;
use portable_pty::cmdbuilder::CommandBuilder;
use promise::spawn::block_on;
use std::borrow::Cow;
use std::env::current_dir;
use std::ffi::OsString;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use wezterm_client::domain::ClientDomain;
use wezterm_font::FontConfiguration;
use wezterm_gui_subcommands::{name_equals_value, StartCommand};
use wezterm_mux_server_impl::update_mux_domains;
use wezterm_toast_notification::*;

mod ai_auth;
mod ai_chat_engine;
mod ai_client;
mod ai_conversations;
#[cfg(feature = "remote")]
mod ai_remote;
mod ai_state;
mod ai_tools;
mod colorease;
mod commands;
mod customglyph;
mod download;
mod frontend;
mod glyphcache;
mod inputmap;
#[cfg(target_os = "macos")]
mod macos;
mod overlay;
mod quad;
mod renderstate;
mod resize_increment_calculator;
mod scripting;
mod scrollbar;
mod selection;
mod session_restore;
mod shapecache;
mod soul;
mod spawn;
mod startup_trace;
mod stats;
mod tabbar;
mod termwindow;
mod thread_util;
mod uniforms;
mod update;
mod utilsprites;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

pub use selection::SelectionMode;
pub use termwindow::{set_window_class, set_window_position, TermWindow, ICON_DATA};

#[derive(Debug, Parser)]
#[command(
    about = "Kaku Terminal Emulator\nhttp://github.com/tw93/Kaku",
    version = config::wezterm_version()
)]
struct Opt {
    /// Skip loading kaku.lua
    #[arg(long, short = 'n')]
    skip_config: bool,

    /// Specify the configuration file to use, overrides the normal
    /// configuration file resolution
    #[arg(
        long = "config-file",
        value_parser,
        conflicts_with = "skip_config",
        value_hint=ValueHint::FilePath,
    )]
    config_file: Option<OsString>,

    /// Override specific configuration values
    #[arg(
        long = "config",
        name = "name=value",
        value_parser=ValueParser::new(name_equals_value),
        number_of_values = 1)]
    config_override: Vec<(String, String)>,

    /// On Windows, whether to attempt to attach to the parent
    /// process console to display logging output
    #[arg(long = "attach-parent-console")]
    #[allow(dead_code)]
    attach_parent_console: bool,

    #[command(subcommand)]
    cmd: Option<SubCommand>,
}

#[derive(Debug, Parser, Clone)]
enum SubCommand {
    #[command(
        name = "start",
        about = "Start the GUI, optionally running an alternative program [aliases: -e]"
    )]
    Start(StartCommand),

    /// Start the GUI in blocking mode. You shouldn't see this, but you
    /// may see it in shell completions because of this open clap issue:
    /// <https://github.com/clap-rs/clap/issues/1335>
    #[command(short_flag_alias = 'e', hide = true)]
    BlockingStart(StartCommand),
}

fn have_panes_in_domain_and_ws(domain: &Arc<dyn Domain>, workspace: &Option<String>) -> bool {
    let mux = Mux::get();
    let have_panes_in_domain = mux
        .iter_panes()
        .iter()
        .any(|p| p.domain_id() == domain.domain_id());

    if !have_panes_in_domain {
        return false;
    }

    if let Some(ws) = &workspace {
        for window_id in mux.iter_windows_in_workspace(ws) {
            if let Some(win) = mux.get_window(window_id) {
                for t in win.iter() {
                    for p in t.iter_panes_ignoring_zoom() {
                        if p.pane.domain_id() == domain.domain_id() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    } else {
        true
    }
}

async fn spawn_tab_in_domain_if_mux_is_empty(
    cmd: Option<CommandBuilder>,
    is_connecting: bool,
    domain: Option<Arc<dyn Domain>>,
    workspace: Option<String>,
) -> anyhow::Result<()> {
    let mux = Mux::get();

    let domain = domain.unwrap_or_else(|| mux.default_domain());

    if !is_connecting {
        if have_panes_in_domain_and_ws(&domain, &workspace) {
            return Ok(());
        }
    }

    startup_trace::mark("  mux.new_empty_window start");
    let window_id = {
        // Force the builder to notify the frontend early,
        // so that the attach await below doesn't block it.
        // This has the consequence of creating the window
        // at the initial size instead of populating it
        // from the size specified in the remote mux.
        // We use the TabAddedToWindow mux notification
        // to detect and adjust the size later on.
        let position = None;
        let builder = mux.new_empty_window(workspace.clone(), position);
        *builder
    };
    startup_trace::mark("  mux.new_empty_window done (notification fired)");

    let config = config::configuration();
    config.update_ulimit()?;

    startup_trace::mark("  domain.attach start");
    domain.attach(Some(window_id)).await?;
    startup_trace::mark("  domain.attach done");

    if have_panes_in_domain_and_ws(&domain, &workspace) {
        trigger_and_log_gui_attached(MuxDomain(domain.domain_id())).await;
        return Ok(());
    }

    let _config_subscription = config::subscribe_to_config_reload(move || {
        promise::spawn::spawn_into_main_thread(async move {
            if let Err(err) = update_mux_domains(&config::configuration()) {
                log::error!("Error updating mux domains: {:#}", err);
            }
        })
        .detach();
        true
    });

    let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi());
    startup_trace::mark("  domain.spawn start (PTY fork)");
    let _tab = domain
        .spawn(
            &mux,
            // Keep spawn path light; GUI will publish definitive pixel geometry
            // right after the first window is created.
            config.initial_size(dpi as u32, None),
            cmd,
            None,
            config.default_encoding,
            window_id,
        )
        .await?;
    startup_trace::mark("  domain.spawn done (PTY forked)");
    trigger_and_log_gui_attached(MuxDomain(domain.domain_id())).await;
    startup_trace::mark("  gui-attached event done");
    Ok(())
}

async fn connect_to_auto_connect_domains() -> anyhow::Result<()> {
    let mux = Mux::get();
    let domains = mux.iter_domains();
    for dom in domains {
        if let Some(dom) = dom.downcast_ref::<ClientDomain>() {
            if dom.connect_automatically() {
                let domain_name = dom.domain_name().to_string();
                dom.attach(None)
                    .await
                    .with_context(|| format!("auto-connect domain `{domain_name}`"))?;
            }
        }
    }
    Ok(())
}

async fn trigger_gui_startup(
    lua: Option<Rc<mlua::Lua>>,
    spawn: Option<SpawnCommand>,
) -> anyhow::Result<()> {
    if let Some(lua) = lua {
        let args = lua.pack_multi(spawn)?;
        config::lua::emit_event(&lua, ("gui-startup".to_string(), args)).await?;
    }
    Ok(())
}

async fn trigger_and_log_gui_startup(spawn_command: Option<SpawnCommand>) {
    if let Err(err) =
        config::with_lua_config_on_main_thread(move |lua| trigger_gui_startup(lua, spawn_command))
            .await
    {
        let message = format!("while processing gui-startup event: {:#}", err);
        log::error!("{}", message);
        persistent_toast_notification("Error", &message);
    }
}

async fn trigger_gui_attached(lua: Option<Rc<mlua::Lua>>, domain: MuxDomain) -> anyhow::Result<()> {
    if let Some(lua) = lua {
        let args = lua.pack_multi(domain)?;
        config::lua::emit_event(&lua, ("gui-attached".to_string(), args)).await?;
    }
    Ok(())
}

async fn trigger_and_log_gui_attached(domain: MuxDomain) {
    if let Err(err) =
        config::with_lua_config_on_main_thread(move |lua| trigger_gui_attached(lua, domain)).await
    {
        let message = format!("while processing gui-attached event: {:#}", err);
        log::error!("{}", message);
        persistent_toast_notification("Error", &message);
    }
}

fn cell_pixel_dims(config: &ConfigHandle, dpi: f64) -> anyhow::Result<(usize, usize)> {
    startup_trace::mark("  FontConfiguration#1 start");
    let fontconfig = Rc::new(FontConfiguration::new(Some(config.clone()), dpi as usize)?);
    startup_trace::mark("  FontConfiguration#1 done");
    let render_metrics = RenderMetrics::new(&fontconfig)?;
    Ok((
        render_metrics.cell_size.width as usize,
        render_metrics.cell_size.height as usize,
    ))
}

async fn async_run_terminal_gui(
    cmd: Option<CommandBuilder>,
    opts: StartCommand,
    should_publish: bool,
) -> anyhow::Result<()> {
    let unix_socket_path =
        config::RUNTIME_DIR.join(format!("gui-sock-{}", unsafe { libc::getpid() }));
    std::env::set_var("KAKU_UNIX_SOCKET", unix_socket_path.clone());
    wezterm_blob_leases::register_storage(Arc::new(
        wezterm_blob_leases::simple_tempdir::SimpleTempDir::new_in(&*config::CACHE_DIR)?,
    ))?;
    startup_trace::mark("  register_storage done");

    if let Err(err) = spawn_mux_server(unix_socket_path, should_publish) {
        log::warn!("{:#}", err);
    }
    startup_trace::mark("  spawn_mux_server done");

    #[cfg(feature = "remote")]
    {
        ai_remote::register_if_configured();
        kaku_remote::start();
    }

    let default_domain_is_local = Mux::get().default_domain().domain_name() == "local";
    if default_domain_is_local {
        promise::spawn::spawn_with_low_priority(async {
            if let Err(err) = update_mux_domains(&config::configuration()) {
                log::warn!("deferred update_mux_domains failed: {err:#}");
                return;
            }
            if let Err(err) = connect_to_auto_connect_domains().await {
                log::warn!("deferred auto-connect domains failed: {err:#}");
            }
        })
        .detach();
    }

    if !opts.no_auto_connect {
        let explicit_domain_requested = opts.domain.is_some();

        if !(default_domain_is_local && !explicit_domain_requested) {
            // Preserve existing startup semantics when the startup target
            // is a non-local/explicit domain.
            connect_to_auto_connect_domains().await?;
        }
    }

    let spawn_command = match &cmd {
        Some(cmd) => Some(SpawnCommand::from_command_builder(cmd)?),
        None => None,
    };

    // Apply the domain to the command
    let spawn_command = match (spawn_command, &opts.domain) {
        (Some(spawn), Some(name)) => Some(SpawnCommand {
            domain: SpawnTabDomain::DomainName(name.to_string()),
            ..spawn
        }),
        (None, Some(name)) => Some(SpawnCommand {
            domain: SpawnTabDomain::DomainName(name.to_string()),
            ..SpawnCommand::default()
        }),
        (spawn, None) => spawn,
    };
    let mux = Mux::get();

    let domain = if let Some(name) = &opts.domain {
        let domain = mux
            .get_domain_by_name(name)
            .ok_or_else(|| anyhow!("invalid domain {name}"))?;
        Some(domain)
    } else {
        None
    };

    if !opts.attach {
        startup_trace::mark("  trigger_and_log_gui_startup start");
        trigger_and_log_gui_startup(spawn_command).await;
        startup_trace::mark("  trigger_and_log_gui_startup done");
    }

    let is_connecting = opts.attach;

    if let Some(domain) = &domain {
        if !opts.attach {
            let window_id = {
                // Force the builder to notify the frontend early,
                // so that the attach await below doesn't block it.
                let workspace = None;
                let position = None;
                let builder = mux.new_empty_window(workspace, position);
                *builder
            };

            domain.attach(Some(window_id)).await?;
            let config = config::configuration();
            let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi());
            let tab = domain
                .spawn(
                    &mux,
                    // Keep spawn path light; GUI will publish definitive pixel geometry
                    // right after the first window is created.
                    config.initial_size(dpi as u32, None),
                    cmd.clone(),
                    None,
                    config.default_encoding,
                    window_id,
                )
                .await?;
            let mut window = mux
                .get_window_mut(window_id)
                .ok_or_else(|| anyhow!("failed to get mux window id {window_id}"))?;
            if let Some(tab_idx) = window.idx_by_id(tab.tab_id()) {
                window.set_active_without_saving(tab_idx);
            }
            trigger_and_log_gui_attached(MuxDomain(domain.domain_id())).await;
        }
    }
    if cmd.is_none() && domain.is_none() && !is_connecting {
        let config = config::configuration();
        if config.restore_previous_session {
            startup_trace::mark("  auto-restore session start");
            match session_restore::try_restore_on_startup().await {
                Ok(true) => {
                    log::info!("auto-restored previous session from snapshot");
                    startup_trace::mark("  auto-restore session done");
                    return Ok(());
                }
                Ok(false) => {
                    log::debug!("no session snapshot available; normal startup");
                }
                Err(err) => {
                    log::warn!("auto-restore failed, falling back: {err:#}");
                }
            }
        }
    }

    startup_trace::mark("  spawn_tab_in_domain_if_mux_is_empty start");
    let res = spawn_tab_in_domain_if_mux_is_empty(cmd, is_connecting, domain, opts.workspace).await;
    startup_trace::mark("  spawn_tab_in_domain_if_mux_is_empty done");
    res
}

#[derive(Debug)]
enum Publish {
    TryPathOrPublish(PathBuf),
    NoConnectNoPublish,
    NoConnectButPublish,
}

fn should_try_existing_gui(
    mux_default_domain: &str,
    config_default_domain: Option<&str>,
    always_new_process: bool,
    config_overridden: bool,
) -> bool {
    if mux_default_domain != config_default_domain.unwrap_or("local") {
        return false;
    }

    if always_new_process {
        return false;
    }

    if config_overridden {
        return false;
    }

    true
}

fn should_spawn_in_current_window(new_tab: bool, prefer_to_spawn_tabs: bool) -> bool {
    new_tab || prefer_to_spawn_tabs
}

impl Publish {
    pub fn resolve(mux: &Arc<Mux>, config: &ConfigHandle, always_new_process: bool) -> Self {
        if !should_try_existing_gui(
            mux.default_domain().domain_name(),
            config.default_domain.as_deref(),
            always_new_process,
            config::is_config_overridden(),
        ) {
            if config::is_config_overridden() {
                // They're using a specific config file: assume that it is
                // different from the running gui
                log::trace!("skip existing gui: config is different");
            }
            return Self::NoConnectNoPublish;
        }

        match wezterm_client::discovery::resolve_gui_sock_path(
            &crate::termwindow::get_window_class(),
        ) {
            Ok(path) => Self::TryPathOrPublish(path),
            Err(_) => Self::NoConnectButPublish,
        }
    }

    pub fn should_publish(&self) -> bool {
        match self {
            Self::TryPathOrPublish(_) | Self::NoConnectButPublish => true,
            Self::NoConnectNoPublish => false,
        }
    }

    pub fn try_spawn(
        &mut self,
        cmd: Option<CommandBuilder>,
        config: &ConfigHandle,
        workspace: Option<&str>,
        domain: SpawnTabDomain,
        new_tab: bool,
    ) -> anyhow::Result<bool> {
        if let Publish::TryPathOrPublish(gui_sock) = &self {
            #[cfg(unix)]
            {
                if let Err(err) = std::os::unix::net::UnixStream::connect(gui_sock) {
                    // Fast-path stale socket detection to avoid paying the
                    // client retry backoff (which feels like slow startup).
                    log::trace!(
                        "existing gui socket {} is not connectable: {:#}",
                        gui_sock.display(),
                        err
                    );
                    return Ok(false);
                }
            }

            let dom = config::UnixDomain {
                socket_path: Some(gui_sock.clone()),
                no_serve_automatically: true,
                // Keep single-instance handoff snappy; if the running
                // instance is unhealthy, fail fast and start a fresh GUI.
                read_timeout: Duration::from_millis(250),
                write_timeout: Duration::from_millis(250),
                ..Default::default()
            };
            let mut ui = mux::connui::ConnectionUI::new_headless();
            match wezterm_client::client::Client::new_unix_domain(None, &dom, false, &mut ui, true)
            {
                Ok(client) => {
                    let executor = promise::spawn::ScopedExecutor::new();
                    let command = cmd.clone();
                    let res = block_on(executor.run(async move {
                        let vers = client.verify_version_compat(&mut ui).await?;

                        if vers.executable_path != std::env::current_exe().context("resolve executable path")? {
                            *self = Publish::NoConnectNoPublish;
                            anyhow::bail!(
                                "Running GUI is a different executable from us, will start a new one");
                        }
                        if vers.config_file_path
                            != std::env::var_os("KAKU_CONFIG_FILE").map(Into::into)
                        {
                            *self = Publish::NoConnectNoPublish;
                            anyhow::bail!(
                                "Running GUI has different config from us, will start a new one"
                            );
                        }

                        let window_id = if should_spawn_in_current_window(
                            new_tab,
                            config.prefer_to_spawn_tabs,
                        ) {
                            if let Ok(pane_id) = client.resolve_pane_id(None).await {
                                let panes = client.list_panes().await?;

                                let mut window_id = None;
                                'outer: for tabroot in panes.tabs {
                                    let mut cursor = tabroot.into_tree().cursor();

                                    loop {
                                        if let Some(entry) = cursor.leaf_mut() {
                                            if entry.pane_id == pane_id {
                                                window_id.replace(entry.window_id);
                                                break 'outer;
                                            }
                                        }
                                        match cursor.preorder_next() {
                                            Ok(c) => cursor = c,
                                            Err(_) => break,
                                        }
                                    }
                                }
                                window_id
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        client
                            .spawn_v2(codec::SpawnV2 {
                                domain,
                                window_id,
                                command,
                                command_dir: None,
                                size: config.initial_size(0, None),
                                workspace: workspace.unwrap_or(
                                    config
                                        .default_workspace
                                        .as_deref()
                                        .unwrap_or(mux::DEFAULT_WORKSPACE)
                                ).to_string(),
                            })
                            .await
                    }));

                    match res {
                        Ok(res) => {
                            log::debug!(
                                "Spawned your command via the existing GUI instance. \
                             Use kaku start --always-new-process if you do not want this behavior. \
                             Result={:?}",
                                res
                            );
                            Ok(true)
                        }
                        Err(err) => {
                            log::trace!(
                                "while attempting to ask existing instance to spawn: {:#}",
                                err
                            );
                            Ok(false)
                        }
                    }
                }
                Err(err) => {
                    // Couldn't connect: it's probably a stale symlink.
                    // That's fine: we can continue with starting a fresh gui below.
                    log::trace!("{:#}", err);
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }
}

fn spawn_mux_server(unix_socket_path: PathBuf, should_publish: bool) -> anyhow::Result<()> {
    let mut listener =
        wezterm_mux_server_impl::local::LocalListener::with_domain(&config::UnixDomain {
            socket_path: Some(unix_socket_path.clone()),
            ..Default::default()
        })?;
    crate::thread_util::spawn_with_pool(move || {
        let name_holder;
        if should_publish {
            name_holder = wezterm_client::discovery::publish_gui_sock_path(
                &unix_socket_path,
                &crate::termwindow::get_window_class(),
            );
            if let Err(err) = &name_holder {
                log::warn!("{:#}", err);
            }
        }

        listener.run();
        std::fs::remove_file(unix_socket_path).ok();
    });

    Ok(())
}

fn setup_mux(
    local_domain: Arc<dyn Domain>,
    config: &ConfigHandle,
    default_domain_name: Option<&str>,
    default_workspace_name: Option<&str>,
) -> anyhow::Result<Arc<Mux>> {
    let mux = Arc::new(mux::Mux::new(Some(local_domain.clone())));
    Mux::set_mux(&mux);
    let client_id = Arc::new(mux::client::ClientId::new());
    mux.register_client(client_id.clone());
    mux.replace_identity(Some(client_id));
    let default_workspace_name = default_workspace_name.unwrap_or(
        config
            .default_workspace
            .as_deref()
            .unwrap_or(mux::DEFAULT_WORKSPACE),
    );
    mux.set_active_workspace(&default_workspace_name);

    let default_name =
        default_domain_name.unwrap_or(config.default_domain.as_deref().unwrap_or("local"));
    // Startup fast-path: if local is the default domain, defer scanning and
    // constructing additional domains until after first window appears.
    if default_name != "local" {
        update_mux_domains(config)?;
    }

    let domain = mux.get_domain_by_name(default_name).ok_or_else(|| {
        anyhow::anyhow!(
            "desired default domain '{}' was not found in mux!?",
            default_name
        )
    })?;
    mux.set_default_domain(&domain);

    Ok(mux)
}

fn build_initial_mux(
    config: &ConfigHandle,
    default_domain_name: Option<&str>,
    default_workspace_name: Option<&str>,
) -> anyhow::Result<Arc<Mux>> {
    let domain: Arc<dyn Domain> = Arc::new(LocalDomain::new("local")?);
    setup_mux(domain, config, default_domain_name, default_workspace_name)
}

fn run_terminal_gui(opts: StartCommand, default_domain_name: Option<String>) -> anyhow::Result<()> {
    if let Some(cls) = opts.class.as_ref() {
        crate::set_window_class(cls);
    }
    if let Some(pos) = opts.position.as_ref() {
        set_window_position(pos.clone());
    }

    let config = config::configuration();

    // Prewarm font caches in a background thread so that
    // FontConfiguration::new() in new_window() hits warm caches instead of
    // blocking the async startup path.
    let font_dirs = config.font_dirs.clone();
    startup_trace::mark("font-prewarm spawn");
    if let Err(err) = std::thread::Builder::new()
        .name("font-prewarm".into())
        .spawn(move || {
            startup_trace::mark("  font-prewarm thread start");
            let _ = wezterm_font::db::FontDatabase::with_built_in();
            wezterm_font::db::FontDatabase::prewarm_font_dirs(&font_dirs);
            startup_trace::mark("  font-prewarm thread done");
        })
    {
        log::warn!("Failed to start font prewarm thread: {}", err);
    }

    let need_builder = !opts.prog.is_empty() || opts.cwd.is_some();

    let cmd = if need_builder {
        let prog = opts.prog.iter().map(|s| s.as_os_str()).collect::<Vec<_>>();
        let mut builder = config.build_prog(
            if prog.is_empty() { None } else { Some(prog) },
            config.default_prog.as_ref(),
            config.default_cwd.as_ref(),
        )?;
        if let Some(cwd) = &opts.cwd {
            builder.cwd(if cwd.is_relative() {
                current_dir()?.join(cwd).into_os_string().into()
            } else {
                Cow::Borrowed(cwd.as_ref())
            });
        }
        Some(builder)
    } else {
        None
    };

    startup_trace::mark("build_initial_mux() start");
    let mux = build_initial_mux(
        &config,
        default_domain_name.as_deref(),
        opts.workspace.as_deref(),
    )?;
    startup_trace::mark("build_initial_mux() done");

    // First, let's see if we can ask an already running Kaku instance to do this.
    // We must do this before we start the gui frontend as the scheduler
    // requirements are different.
    startup_trace::mark("Publish::resolve start");
    let mut publish = Publish::resolve(
        &mux,
        &config,
        opts.always_new_process || opts.position.is_some(),
    );
    startup_trace::mark("Publish::resolve done");
    log::trace!("{:?}", publish);
    if publish.try_spawn(
        cmd.clone(),
        &config,
        opts.workspace.as_deref(),
        match &opts.domain {
            Some(name) => SpawnTabDomain::DomainName(name.to_string()),
            None => SpawnTabDomain::DefaultDomain,
        },
        opts.new_tab,
    )? {
        return Ok(());
    }

    startup_trace::mark("GuiFrontEnd::try_new() start");
    let gui = crate::frontend::try_new()?;
    startup_trace::mark("GuiFrontEnd::try_new() done");
    let activity = Activity::new();

    promise::spawn::spawn(async move {
        if let Err(err) = async_run_terminal_gui(cmd, opts, publish.should_publish()).await {
            terminate_with_error(err);
        }
        drop(activity);
    })
    .detach();
    // Kick the startup future once so window creation can get underway
    // before entering the main loop, without blocking too long here.
    let _ = ::window::drain_spawn_queue_burst(8);

    maybe_show_configuration_error_window();
    startup_trace::mark("gui.run_forever() entering event loop");
    gui.run_forever()
}

fn fatal_toast_notification(title: &str, message: &str) {
    let should_show = if cfg!(debug_assertions) {
        std::env::var_os("KAKU_DEV_FATAL_TOAST").is_some()
    } else {
        true
    };

    if !should_show {
        log::error!(
            "suppressed fatal toast in debug build: {} - {}",
            title,
            message
        );
        return;
    }

    persistent_toast_notification(title, message);
    // We need a short delay otherwise the notification
    // will not show
    #[cfg(windows)]
    std::thread::sleep(std::time::Duration::new(2, 0));
}

fn notify_on_panic() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(s) = info.payload().downcast_ref::<&str>() {
            fatal_toast_notification("Kaku panic", s);
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            fatal_toast_notification("Kaku panic", s);
        }
        default_hook(info);
    }));
}

fn terminate_with_error_message(err: &str) -> ! {
    log::error!("{}; terminating", err);
    fatal_toast_notification("Kaku Error", &err);
    std::process::exit(1);
}

fn terminate_with_error(err: anyhow::Error) -> ! {
    let mut err_text = format!("{err:#}");

    let warnings = config::configuration_warnings_and_errors();
    if !warnings.is_empty() {
        let err = warnings.join("\n");
        err_text = format!("{err_text}\nConfiguration Error: {err}");
    }

    terminate_with_error_message(&err_text)
}

fn main() {
    startup_trace::init();
    startup_trace::mark("main() entry");

    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    config::designate_this_as_the_main_thread();
    config::assign_error_callback(mux::connui::show_configuration_error_message);
    notify_on_panic();
    if let Err(e) = run() {
        terminate_with_error(e);
    }
    if let Err(err) = session_restore::save_focused_window_snapshot() {
        log::warn!("failed to save last window snapshot on exit: {err:#}");
    }
    Mux::shutdown();
    frontend::shutdown();
}

fn maybe_show_configuration_error_window() {
    let warnings = config::configuration_warnings_and_errors();
    if !warnings.is_empty() {
        let err = warnings.join("\n");
        mux::connui::show_configuration_error_message(&err);
    }
}

fn run() -> anyhow::Result<()> {
    // Inform the system of our AppUserModelID.
    // Without this, our toast notifications won't be correctly
    // attributed to our application.
    #[cfg(windows)]
    {
        unsafe {
            ::windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
                ::windows::core::PCWSTR(wide_string("sh.kaku.Kaku").as_ptr()),
            )
            .unwrap();
        }
    }

    let opts = Opt::parse();

    if opts.config_file.is_none() && !opts.skip_config {
        if let Err(err) = config::ensure_user_config_exists() {
            log::warn!("Failed to ensure user config exists: {:#}", err);
        }
    }

    // This is a bit gross.
    // In order to not to automatically open a standard windows console when
    // we run, we use the windows_subsystem attribute at the top of this
    // source file.  That comes at the cost of causing the help output
    // to disappear if we are actually invoked from a console.
    // This AttachConsole call will attach us to the console of the parent
    // in that situation, but since we were launched as a windows subsystem
    // application we will be running asynchronously from the shell in
    // the command window, which means that it will appear to the user
    // that we hung at the end, when in reality the shell is waiting for
    // input but didn't know to re-draw the prompt.
    #[cfg(windows)]
    unsafe {
        if opts.attach_parent_console {
            winapi::um::wincon::AttachConsole(winapi::um::wincon::ATTACH_PARENT_PROCESS);
        }
    };

    startup_trace::mark("env_bootstrap::bootstrap() start");
    env_bootstrap::bootstrap();
    startup_trace::mark("env_bootstrap::bootstrap() done");
    // window_funcs is not set up by env_bootstrap as window_funcs is
    // GUI environment specific and env_bootstrap is used to setup the
    // headless mux server.
    config::lua::add_context_setup_func(window_funcs::register);
    config::lua::add_context_setup_func(crate::scripting::register);
    config::lua::add_context_setup_func(crate::stats::register);

    let _saver = umask::UmaskSaver::new();

    // Defer config file watcher setup until the first window is visible.
    // This preserves config loading behavior while moving notify watcher
    // initialization off the first-paint critical path.
    config::defer_watchers_until_enabled();

    startup_trace::mark("common_init() start (config + lua load)");
    config::common_init(
        opts.config_file.as_ref(),
        &opts.config_override,
        opts.skip_config,
    )?;
    startup_trace::mark("common_init() done");
    stats::Stats::init()?;
    let config = config::configuration();
    if let Some(value) = &config.default_ssh_auth_sock {
        std::env::set_var("SSH_AUTH_SOCK", value);
    }

    let sub = match opts.cmd.as_ref().cloned() {
        Some(SubCommand::BlockingStart(start)) => {
            // Act as if the normal start subcommand was used,
            // except that we always start a new instance.
            // This is needed for compatibility, because many tools assume
            // that "$TERMINAL -e $COMMAND" blocks until the command finished.
            SubCommand::Start(StartCommand {
                always_new_process: true,
                ..start
            })
        }
        Some(sub) => sub,
        None => {
            // Need to fake an argv0
            let mut argv = vec!["kaku-gui".to_string()];
            for a in &config.default_gui_startup_args {
                argv.push(a.clone());
            }
            SubCommand::try_parse_from(&argv).with_context(|| {
                format!(
                    "parsing the default_gui_startup_args config: {:?}",
                    config.default_gui_startup_args
                )
            })?
        }
    };

    match sub {
        SubCommand::Start(start) => {
            log::trace!("Using configuration: {:#?}\nopts: {:#?}", config, opts);
            let res = run_terminal_gui(start, None);
            wezterm_blob_leases::clear_storage();
            res
        }
        SubCommand::BlockingStart(_) => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::{should_spawn_in_current_window, should_try_existing_gui};

    #[test]
    fn existing_gui_handoff_requires_matching_default_domain() {
        assert!(should_try_existing_gui("local", None, false, false));
        assert!(should_try_existing_gui(
            "local",
            Some("local"),
            false,
            false
        ));
        assert!(!should_try_existing_gui("ssh", Some("local"), false, false));
    }

    #[test]
    fn existing_gui_handoff_respects_new_process_and_config_override() {
        assert!(!should_try_existing_gui(
            "local",
            Some("local"),
            true,
            false
        ));
        assert!(!should_try_existing_gui(
            "local",
            Some("local"),
            false,
            true
        ));
    }

    #[test]
    fn current_window_spawning_respects_new_tab_preferences() {
        assert!(should_spawn_in_current_window(true, false));
        assert!(should_spawn_in_current_window(false, true));
        assert!(!should_spawn_in_current_window(false, false));
    }
}
