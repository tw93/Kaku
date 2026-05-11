#![allow(clippy::cast_abs_to_unsigned)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::double_ended_iterator_last)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::manual_find)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_return)]
#![allow(clippy::question_mark)]
#![allow(clippy::single_match)]
#![allow(clippy::single_range_in_vec_init)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::useless_conversion)]

use anyhow::{anyhow, Context};
use clap::builder::ValueParser;
use clap::{Parser, ValueEnum, ValueHint};
use clap_complete::{generate as generate_completion, shells, Generator as CompletionGenerator};
use config::{wezterm_version, ConfigHandle};
use mux::Mux;

// Register the i18n bundle for the `kaku` CLI. `t!()` invocations inside
// this crate look up keys against `locales/{en,zh-CN}.yml` at the
// workspace root.
rust_i18n::i18n!("../locales", fallback = "en");
use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use umask::UmaskSaver;
use wezterm_gui_subcommands::*;

mod ai_config;
mod assistant_config;
mod chat;
mod cli;
mod config_cmd;
mod config_tui;
mod doctor;
mod init;
mod kaku_theme;
mod reset;
mod shell;
mod tui_core;
mod tui_splash;
mod update;
mod utils;

#[derive(Debug, Parser)]
#[command(
    about = "Kaku Terminal Emulator\nhttp://github.com/tw93/Kaku",
    version = wezterm_version()
)]
pub struct Opt {
    /// Skip loading kaku.lua
    #[arg(long, short = 'n')]
    skip_config: bool,

    /// Specify the configuration file to use, overrides the normal
    /// configuration file resolution
    #[arg(
        long,
        value_parser,
        conflicts_with = "skip_config",
        value_hint=ValueHint::FilePath
    )]
    config_file: Option<OsString>,

    /// Override specific configuration values
    #[arg(
        long = "config",
        name = "name=value",
        value_parser=ValueParser::new(name_equals_value),
        number_of_values = 1)]
    config_override: Vec<(String, String)>,

    #[command(subcommand)]
    cmd: Option<SubCommand>,
}

#[derive(Debug, Clone, ValueEnum)]
enum Shell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
    Fig,
}

impl CompletionGenerator for Shell {
    fn file_name(&self, name: &str) -> String {
        match self {
            Shell::Bash => shells::Bash.file_name(name),
            Shell::Elvish => shells::Elvish.file_name(name),
            Shell::Fish => shells::Fish.file_name(name),
            Shell::PowerShell => shells::PowerShell.file_name(name),
            Shell::Zsh => shells::Zsh.file_name(name),
            Shell::Fig => clap_complete_fig::Fig.file_name(name),
        }
    }

    fn generate(&self, cmd: &clap::Command, buf: &mut dyn std::io::Write) {
        match self {
            Shell::Bash => shells::Bash.generate(cmd, buf),
            Shell::Elvish => shells::Elvish.generate(cmd, buf),
            Shell::Fish => shells::Fish.generate(cmd, buf),
            Shell::PowerShell => shells::PowerShell.generate(cmd, buf),
            Shell::Zsh => shells::Zsh.generate(cmd, buf),
            Shell::Fig => clap_complete_fig::Fig.generate(cmd, buf),
        }
    }
}

#[derive(Debug, Parser, Clone)]
enum SubCommand {
    #[command(
        name = "start",
        about = "Start the GUI, optionally running an alternative program [aliases: -e]",
        hide = true
    )]
    Start(StartCommand),

    /// Start the GUI in blocking mode. You shouldn't see this, but you
    /// may see it in shell completions because of this open clap issue:
    /// <https://github.com/clap-rs/clap/issues/1335>
    #[command(short_flag_alias = 'e', hide = true)]
    BlockingStart(StartCommand),

    #[command(name = "ai", about = "Manage AI settings")]
    Ai(ai_config::AiConfigCommand),

    #[command(
        name = "chat",
        about = "Start the AI chat in this terminal (alias for `k`)"
    )]
    Chat(chat::ChatCommand),

    #[command(name = "config", about = "Configure Kaku settings")]
    Config(config_cmd::ConfigCommand),

    #[command(name = "init", about = "Initialize Kaku shell integration")]
    Init(init::InitCommand),

    #[command(
        name = "doctor",
        about = "Check Kaku shell integration, environment, and runtime health"
    )]
    Doctor(doctor::DoctorCommand),

    #[command(
        name = "update",
        about = "Download and install the latest Kaku release automatically"
    )]
    Update(update::UpdateCommand),

    #[command(
        name = "reset",
        about = "Reset Kaku shell integration and managed defaults"
    )]
    Reset(reset::ResetCommand),

    #[command(
        name = "cli",
        about = "Interact with experimental mux server",
        hide = true
    )]
    Cli(cli::CliCommand),

    #[command(
        name = "set-working-directory",
        about = "Advise the terminal of the current working directory by \
                 emitting an OSC 7 escape sequence",
        hide = true
    )]
    SetCwd(SetCwdCommand),

    #[cfg(feature = "remote")]
    #[command(name = "remote", about = "Show QR code to connect Kaku iOS app")]
    Remote,

    /// Generate shell completion information
    #[command(name = "shell-completion", hide = true)]
    ShellCompletion {
        /// Which shell to generate for
        #[arg(long, value_parser)]
        shell: Shell,
    },
}

use termwiz::escape::osc::OperatingSystemCommand;

#[derive(Debug, Parser, Clone)]
struct SetCwdCommand {
    /// The directory to specify.
    /// If omitted, will use the current directory of the process itself.
    #[arg(value_parser, value_hint=ValueHint::DirPath)]
    cwd: Option<OsString>,

    /// How to manage passing the escape through to tmux
    #[arg(long, value_parser)]
    tmux_passthru: Option<TmuxPassthru>,

    /// The hostname to use in the constructed file:// URL.
    /// If omitted, the system hostname will be used.
    #[arg(value_parser, value_hint=ValueHint::Hostname)]
    host: Option<OsString>,
}

impl SetCwdCommand {
    fn run(&self) -> anyhow::Result<()> {
        let mut cwd = std::env::current_dir()?;
        if let Some(dir) = &self.cwd {
            cwd.push(dir);
        }

        let mut url = url::Url::from_directory_path(&cwd)
            .map_err(|_| anyhow::anyhow!("cwd {} is not an absolute path", cwd.display()))?;
        let host = match self.host.as_ref() {
            Some(h) => h.clone(),
            None => hostname::get()?,
        };
        let host = host.to_str().unwrap_or("localhost");
        url.set_host(Some(host))?;

        let osc = OperatingSystemCommand::CurrentWorkingDirectory(url.into());
        let tmux = self.tmux_passthru.unwrap_or_default();
        let encoded = tmux.encode(osc.to_string());
        print!("{encoded}");
        if tmux.enabled() {
            // Tmux understands OSC 7 but won't automatically pass it through.
            // <https://github.com/tmux/tmux/issues/3127#issuecomment-1076300455>
            // Let's do it again explicitly now.
            print!("{osc}");
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, ValueEnum, Default)]
enum TmuxPassthru {
    Disable,
    Enable,
    #[default]
    Detect,
}

impl TmuxPassthru {
    fn is_tmux() -> bool {
        std::env::var_os("TMUX").is_some()
    }

    fn enabled(&self) -> bool {
        match self {
            Self::Enable => true,
            Self::Detect => Self::is_tmux(),
            Self::Disable => false,
        }
    }

    fn encode(&self, content: String) -> String {
        if self.enabled() {
            let mut result = "\u{1b}Ptmux;".to_string();
            for c in content.chars() {
                if c == '\u{1b}' {
                    // Quote the escape by doubling it up
                    result.push(c);
                }
                result.push(c);
            }
            result.push_str("\u{1b}\\");
            result
        } else {
            content
        }
    }
}

fn terminate_with_error_message(err: &str) -> ! {
    log::error!("{}; terminating", err);
    std::process::exit(1);
}

fn terminate_with_error(err: anyhow::Error) -> ! {
    terminate_with_error_message(&format!("{:#}", err));
}

fn main() {
    config::designate_this_as_the_main_thread();
    config::assign_error_callback(mux::connui::show_configuration_error_message);

    // Stage-1 locale apply: based purely on `$LC_ALL` / `$LC_MESSAGES` /
    // `$LANG`, no Lua config required. This makes every subcommand
    // (including `kaku config` / `kaku ai`, which historically skipped
    // `init_config`) render in the user's environment language. The
    // subcommands that DO call `init_config` later will refine this
    // based on the Lua-side `config.language` override.
    rust_i18n::set_locale(&config::i18n::resolve_locale(config::i18n::LANGUAGE_AUTO));

    if let Err(e) = run() {
        terminate_with_error(e);
    }
    Mux::shutdown();
}

fn init_config(opts: &Opt) -> anyhow::Result<ConfigHandle> {
    config::common_init(
        opts.config_file.as_ref(),
        &opts.config_override,
        opts.skip_config,
    )
    .context("config::common_init")?;
    let config = config::configuration();
    config.update_ulimit()?;
    if let Some(value) = &config.default_ssh_auth_sock {
        std::env::set_var("SSH_AUTH_SOCK", value);
    }
    apply_locale_from_config(&config);
    Ok(config)
}

/// Apply the locale resolved from `config.language` to the rust-i18n
/// runtime so that `t!()` invocations elsewhere in the CLI render the
/// right language. Resolution rules live in `config::i18n`.
fn apply_locale_from_config(config: &ConfigHandle) {
    let locale = config::i18n::resolve_locale(&config.language);
    rust_i18n::set_locale(&locale);
    log::trace!("i18n: active locale = {locale}");
}

/// Load just enough Kaku config to refine the rust-i18n locale.
///
/// CLI subcommands like `kaku config` / `kaku ai` / `kaku doctor` skip
/// the full `init_config` path (which also tweaks ulimits and SSH
/// environment); they still need the Lua-side `config.language` to win
/// over the stage-1 environment-based locale picked in `main()`.
/// Failures are intentionally swallowed — the stage-1 locale remains in
/// effect, which is the same fallback behavior `init_config` would have
/// chosen if Lua loading failed.
fn refine_locale_from_lua_config(opts: &Opt) {
    if config::common_init(
        opts.config_file.as_ref(),
        &opts.config_override,
        opts.skip_config,
    )
    .is_ok()
    {
        apply_locale_from_config(&config::configuration());
    }
}

fn run() -> anyhow::Result<()> {
    let saver = UmaskSaver::new();

    // Clap renders --help/--version during parse, so version info must be
    // assigned before Opt::parse() even when we skip full env bootstrap.
    config::assign_version_info(
        wezterm_version::wezterm_version(),
        wezterm_version::wezterm_target_triple(),
    );

    let opts = Opt::parse();

    let cmd = if let Some(cmd) = opts.cmd.as_ref().cloned() {
        Some(cmd)
    } else if should_show_main_menu(&opts) {
        select_main_menu_command()?
    } else {
        Some(SubCommand::Start(StartCommand::default()))
    };

    let Some(cmd) = cmd else {
        return Ok(());
    };

    match cmd {
        SubCommand::Start(_) | SubCommand::BlockingStart(_) => {
            env_bootstrap::bootstrap();
            delegate_to_gui(saver)
        }
        SubCommand::Cli(cli) => {
            env_bootstrap::bootstrap();
            cli::run_cli(&opts, cli)
        }
        #[cfg(feature = "remote")]
        SubCommand::Remote => {
            let state = kaku_remote::read_state()?;
            let output = if let Some(relay) = &state.tunnel_relay {
                kaku_remote::render_relay_qr_terminal(relay, &state.token)
            } else {
                let host = lan_ip().unwrap_or_else(|| "127.0.0.1".to_string());
                kaku_remote::render_qr_terminal(&host, state.port, &state.token)
            };
            println!("{output}");
            Ok(())
        }
        SubCommand::SetCwd(cmd) => cmd.run(),
        SubCommand::ShellCompletion { shell } => {
            use clap::CommandFactory;
            let mut cmd = Opt::command();
            let name = cmd.get_name().to_string();
            generate_completion(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
        SubCommand::Update(cmd) => cmd.run(),
        SubCommand::Config(cmd) => {
            refine_locale_from_lua_config(&opts);
            cmd.run(
                opts.config_file.as_ref().map(PathBuf::from),
                opts.config_file.clone(),
                opts.config_override.clone(),
                opts.skip_config,
            )
        }
        SubCommand::Init(cmd) => {
            refine_locale_from_lua_config(&opts);
            cmd.run()
        }
        SubCommand::Doctor(cmd) => {
            refine_locale_from_lua_config(&opts);
            cmd.run()
        }
        SubCommand::Reset(cmd) => {
            refine_locale_from_lua_config(&opts);
            cmd.run()
        }
        SubCommand::Ai(cmd) => {
            refine_locale_from_lua_config(&opts);
            cmd.run(
                opts.config_file.clone(),
                opts.config_override.clone(),
                opts.skip_config,
            )
        }
        SubCommand::Chat(cmd) => {
            refine_locale_from_lua_config(&opts);
            cmd.run()
        }
    }
}

fn should_show_main_menu(opts: &Opt) -> bool {
    opts.cmd.is_none()
        && !opts.skip_config
        && opts.config_file.is_none()
        && opts.config_override.is_empty()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
}

fn select_main_menu_command() -> anyhow::Result<Option<SubCommand>> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    const GREEN: &str = "\x1b[0;32m";
    const PURPLE_BOLD: &str = "\x1b[1;35m";
    const GRAY: &str = "\x1b[0;90m";
    const RESET: &str = "\x1b[0m";

    #[derive(Clone, Copy)]
    enum MenuChoice {
        Chat,
        Ai,
        Config,
        Init,
        Doctor,
        Update,
        Reset,
    }

    const MENU_ITEMS: [(&str, &str, MenuChoice); 7] = [
        ("chat", "Start AI chat in this terminal", MenuChoice::Chat),
        ("ai", "Manage AI tools and Kaku Assistant", MenuChoice::Ai),
        (
            "config",
            "Manage terminal and assistant settings",
            MenuChoice::Config,
        ),
        ("init", "Initialize shell integration", MenuChoice::Init),
        (
            "doctor",
            "Run diagnostics for shell and runtime health",
            MenuChoice::Doctor,
        ),
        (
            "update",
            "Check and install latest version",
            MenuChoice::Update,
        ),
        (
            "reset",
            "Remove Kaku shell integration and managed defaults",
            MenuChoice::Reset,
        ),
    ];

    fn render_menu(
        selected: usize,
        green: &str,
        purple_bold: &str,
        gray: &str,
        reset: &str,
    ) -> anyhow::Result<()> {
        use crossterm::cursor::MoveTo;
        use crossterm::queue;
        use crossterm::terminal::{Clear, ClearType};

        let mut stdout = std::io::stdout();
        queue!(stdout, MoveTo(0, 0), Clear(ClearType::All)).context("clear main menu")?;

        let mut out = String::new();
        out.push_str("\r\n");
        out.push_str(&format!("{green}  _  __      _          {reset}\r\n"));
        out.push_str(&format!("{green} | |/ /     | |         {reset}\r\n"));
        out.push_str(&format!("{green} | ' / __ _ | | __ _   _ {reset}\r\n"));
        out.push_str(&format!("{green} |  < / _` || |/ /| | | |{reset}\r\n"));
        out.push_str(&format!(
            "{green} | . \\ (_| ||   < | |_| |{reset}  {purple_bold}https://github.com/tw93/Kaku{reset}\r\n"
        ));
        out.push_str(&format!(
            "{green} |_|\\_\\__,_||_|\\_\\ \\__,_|{reset}  {green}A fast, out-of-the-box terminal built for AI coding.{reset}\r\n"
        ));
        out.push_str("\r\n");
        for (idx, (name, desc, _)) in MENU_ITEMS.iter().enumerate() {
            let is_selected = idx == selected;
            let number = idx + 1;
            if is_selected {
                out.push_str(&format!(
                    "{purple_bold}➤ {number}. {:<7}     {desc}{reset}\r\n",
                    name
                ));
            } else {
                out.push_str(&format!("  {number}. {:<7}     {desc}\r\n", name));
            }
        }
        out.push_str("\r\n");
        out.push_str(&format!(
            "  {gray}↑↓  |  Enter  |  1-7  |  Q Quit  |  Esc Exit{reset}\r\n"
        ));

        stdout
            .write_all(out.as_bytes())
            .context("write main menu")?;
        stdout.flush().context("flush stdout")
    }

    fn to_subcommand(choice: MenuChoice) -> SubCommand {
        match choice {
            MenuChoice::Chat => SubCommand::Chat(chat::ChatCommand::default()),
            MenuChoice::Ai => SubCommand::Ai(ai_config::AiConfigCommand::default()),
            MenuChoice::Config => SubCommand::Config(config_cmd::ConfigCommand::default()),
            MenuChoice::Init => SubCommand::Init(init::InitCommand::default()),
            MenuChoice::Doctor => SubCommand::Doctor(doctor::DoctorCommand::default()),
            MenuChoice::Update => SubCommand::Update(update::UpdateCommand::default()),
            MenuChoice::Reset => SubCommand::Reset(reset::ResetCommand::default()),
        }
    }

    fn can_use_menu_char_shortcut(modifiers: KeyModifiers) -> bool {
        !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    }

    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }

    enable_raw_mode().context("enable raw mode for main menu")?;
    let _raw_guard = RawModeGuard;

    let mut selected = 0usize;
    render_menu(selected, GREEN, PURPLE_BOLD, GRAY, RESET)?;

    loop {
        match event::read().context("read main menu input")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                        render_menu(selected, GREEN, PURPLE_BOLD, GRAY, RESET)?;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected + 1 < MENU_ITEMS.len() {
                        selected += 1;
                        render_menu(selected, GREEN, PURPLE_BOLD, GRAY, RESET)?;
                    }
                }
                KeyCode::Enter => return Ok(Some(to_subcommand(MENU_ITEMS[selected].2))),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Char(c @ '1'..='7') if can_use_menu_char_shortcut(key.modifiers) => {
                    let idx = (c as usize) - ('1' as usize);
                    return Ok(Some(to_subcommand(MENU_ITEMS[idx].2)));
                }
                // Letter shortcuts: first character of each menu item name.
                KeyCode::Char(c) if can_use_menu_char_shortcut(key.modifiers) => {
                    let lower = c.to_ascii_lowercase();
                    if let Some((_, _, choice)) = MENU_ITEMS
                        .iter()
                        .find(|(name, _, _)| name.as_bytes().first().copied() == Some(lower as u8))
                    {
                        return Ok(Some(to_subcommand(*choice)));
                    }
                }
                KeyCode::Char('q') | KeyCode::Char('Q')
                    if can_use_menu_char_shortcut(key.modifiers) =>
                {
                    return Ok(None);
                }
                KeyCode::Esc => return Ok(None),
                _ => {}
            },
            _ => {}
        }
    }
}

fn delegate_to_gui(saver: UmaskSaver) -> anyhow::Result<()> {
    use std::process::Command;

    // Restore the original umask
    drop(saver);

    let exe_name = if cfg!(windows) {
        "kaku-gui.exe"
    } else {
        "kaku-gui"
    };

    let exe = resolve_gui_executable(exe_name)?;

    let mut cmd = Command::new(&exe);
    if cfg!(windows) {
        cmd.arg("--attach-parent-console");
    }

    cmd.args(std::env::args_os().skip(1));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Clean up random fds, except when we're running in an AppImage.
        // AppImage relies on child processes keeping alive an fd that
        // references the mount point and if we close it as part of execing
        // the gui binary, the appimage gets unmounted before we can exec.
        if std::env::var_os("APPIMAGE").is_none() {
            portable_pty::unix::close_random_fds();
        }
        let res = cmd.exec();
        return Err(anyhow::anyhow!("failed to exec {cmd:?}: {res:?}"));
    }

    #[cfg(windows)]
    {
        let mut child = cmd.spawn()?;
        let status = child.wait()?;
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }
}

fn resolve_gui_executable(exe_name: &str) -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe()?;
    let mut candidates = Vec::new();

    if let Some(parent) = current_exe.parent() {
        candidates.push(parent.join(exe_name));
    }

    if let Ok(resolved_exe) = std::fs::canonicalize(&current_exe) {
        if let Some(parent) = resolved_exe.parent() {
            let resolved_candidate = parent.join(exe_name);
            if !candidates
                .iter()
                .any(|candidate| candidate == &resolved_candidate)
            {
                candidates.push(resolved_candidate);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from("/Applications/Kaku.app/Contents/MacOS").join(exe_name));
        candidates.push(
            config::HOME_DIR
                .join("Applications")
                .join("Kaku.app")
                .join("Contents")
                .join("MacOS")
                .join(exe_name),
        );
    }

    if let Some(path) = candidates.iter().find(|path| path.exists()) {
        return Ok(path.clone());
    }

    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("unable to resolve GUI executable path"))
}

#[cfg(feature = "remote")]
fn lan_ip() -> Option<String> {
    use std::net::UdpSocket;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}
