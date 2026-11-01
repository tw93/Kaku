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
mod diagnostics;
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

#[cfg(feature = "remote")]
#[derive(Debug, Parser, Clone)]
enum RemoteAction {
    /// List the browsers paired with this Kaku
    Devices,
    /// Unpair a browser; its open sessions end within seconds
    Revoke {
        /// The id `kaku remote devices` shows
        id: String,
    },
    /// Turn remote access off; open web sessions end within seconds
    Off,
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

    #[command(
        name = "ai",
        about = "Configure AI models, endpoints, and coding tools"
    )]
    Ai(ai_config::AiConfigCommand),

    #[command(
        name = "chat",
        about = "Start interactive AI chat in this terminal (alias for `k`)"
    )]
    Chat(chat::ChatCommand),

    #[command(
        name = "config",
        about = "Open interactive terminal settings and preferences"
    )]
    Config(config_cmd::ConfigCommand),

    #[command(
        name = "init",
        about = "Set up shell integration and install optional CLI tools"
    )]
    Init(init::InitCommand),

    #[command(
        name = "doctor",
        about = "Check shell integration and environment, and write a diagnostic bundle"
    )]
    Doctor(doctor::DoctorCommand),

    #[command(
        name = "update",
        about = "Check for updates and install the latest release"
    )]
    Update(update::UpdateCommand),

    #[command(
        name = "reset",
        about = "Remove Kaku shell and tmux integration, reset managed defaults, delete AI memory"
    )]
    Reset(reset::ResetCommand),

    #[command(
        name = "cli",
        about = "Control tabs, panes, and windows from scripts",
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
    #[command(
        name = "remote",
        about = "Pair a browser with this Kaku, to open its terminals from another device"
    )]
    Remote {
        /// Start over: a new identity, and every paired browser removed
        #[arg(long)]
        reset: bool,
        #[command(subcommand)]
        action: Option<RemoteAction>,
    },

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
    // Only `start` and `cli` set up a logger. Without one log::error! goes
    // nowhere and a failing `kaku remote` or `kaku doctor` exits silently.
    if log::max_level() == log::LevelFilter::Off {
        eprintln!("kaku: {err}");
    } else {
        log::error!("{}; terminating", err);
    }
    std::process::exit(1);
}

fn terminate_with_error(err: anyhow::Error) -> ! {
    terminate_with_error_message(&format!("{:#}", err));
}

#[cfg(feature = "remote")]
fn run_remote(reset: bool, action: Option<RemoteAction>) -> anyhow::Result<()> {
    match action {
        Some(RemoteAction::Devices) => {
            let devices = kaku_remote::list_devices();
            if devices.is_empty() {
                println!("No paired browsers. Run `kaku remote` to pair one.");
            }
            for device in devices {
                println!("{}  {}", device.id, device.name);
            }
            return Ok(());
        }
        Some(RemoteAction::Revoke { id }) => {
            if !kaku_remote::revoke_device(&id)? {
                anyhow::bail!("no paired browser {id}; see `kaku remote devices`");
            }
            println!("Unpaired {id}. Its open sessions end within a few seconds.");
            return Ok(());
        }
        Some(RemoteAction::Off) => {
            if !config_tui::set_remote_enabled(false)? {
                anyhow::bail!("Your kaku.lua sets other remote options; set `enabled = false` in its `config.remote` yourself.");
            }
            // The bridge stops only once Kaku reloads the config; do not say
            // sessions end unless it did.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while kaku_remote::read_state().is_ok() {
                if std::time::Instant::now() > deadline {
                    anyhow::bail!("Saved, but Kaku has not applied it, so web sessions are still open. Choose Reload Configuration in Kaku or quit it.");
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            println!("Remote access is off. Open web sessions have ended.");
            return Ok(());
        }
        None => {}
    }
    if !config::configuration().remote.enabled {
        turn_remote_on()?;
    }
    let identity = if reset {
        kaku_remote::reset_identity()?
    } else {
        kaku_remote::load_or_create_identity()?
    };
    let state = kaku_remote::read_state().map_err(|_| {
        anyhow::anyhow!(
            "Remote access is on, but Kaku is not serving it right now. Open Kaku, or quit and reopen it, then run `kaku remote` again."
        )
    })?;
    if state.tunnel_relay.is_some() {
        ensure_relay_access()?;
    }
    let Some(relay) = &state.tunnel_relay else {
        // Without the relay the page is only reachable on this Mac, and its
        // token changes with every Kaku launch.
        if state.port == 0 {
            anyhow::bail!("Another app holds Kaku's local page port, so Kaku is not serving the page. Set `config.remote.port` to a free port, or quit that app.");
        }
        let url = format!("http://127.0.0.1:{}/#t={}", state.port, state.token);
        println!("{url}");
        return Ok(());
    };
    if reset {
        println!("Every paired browser was removed.");
    }
    // A link printed while this Mac cannot reach the relay only ever opens
    // to "offline" in the browser; say why here instead.
    // A browser already waiting on the current link: answer it rather than
    // printing a new link, which would turn it away.
    if !reset {
        if let Some(request) = kaku_remote::pending_request() {
            return confirm_pairing(&request);
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut down = relay_down_note(kaku_remote::read_relay_status().as_ref(), relay);
    while down.is_some() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(300));
        down = relay_down_note(kaku_remote::read_relay_status().as_ref(), relay);
    }
    print_pairing(&identity.pairing_link(relay)?, down.as_deref());
    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        println!("Run `kaku remote` in a terminal to confirm the browser once it opens the link.");
        return Ok(());
    }
    println!();
    println!("Waiting for the browser to open the link…");
    let link_lasts = std::time::Instant::now() + kaku_remote::OFFER_LIFETIME;
    loop {
        if let Some(request) = kaku_remote::pending_request() {
            return confirm_pairing(&request);
        }
        if std::time::Instant::now() > link_lasts {
            anyhow::bail!("The link expired. Run `kaku remote` for a new one.");
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// Wait for Enter on stdin. Ctrl+C (or the end of input) returns false
/// instead of ending the process, so a refused browser is told at once
/// rather than when the link runs out. `read_line` would retry the read
/// that Ctrl+C interrupts, so this reads the descriptor itself.
#[cfg(feature = "remote")]
fn enter_pressed() -> bool {
    extern "C" fn interrupted(_: libc::c_int) {}
    // SAFETY: an empty handler, installed without SA_RESTART so read(2)
    // returns EINTR; reads go into a one-byte buffer we own.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = interrupted as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
        let mut byte = 0u8;
        loop {
            match libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1) {
                1 if byte == b'\n' => return true,
                1 => continue,
                _ => return false,
            }
        }
    }
}

/// Show the browser waiting to pair and its code; Enter lets it in, Ctrl+C
/// turns it away.
#[cfg(feature = "remote")]
fn confirm_pairing(request: &kaku_remote::PairRequest) -> anyhow::Result<()> {
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    };
    println!();
    println!(
        "{}",
        paint(
            PURPLE_BOLD,
            &format!("{} wants to use this Mac's terminals", request.device.name)
        )
    );
    println!("Code {}", paint(GREEN, &request.code));
    println!(
        "{}",
        paint(
            GRAY,
            "If your browser shows the same code, press Enter to allow it. Otherwise press Ctrl+C."
        )
    );
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Run `kaku remote` in a terminal to answer it.");
    }
    if !enter_pressed() {
        println!();
        kaku_remote::answer_request(&request.code, false)?;
        println!("Refused. Run `kaku remote` for a new link.");
        return Ok(());
    }
    if !kaku_remote::answer_request(&request.code, true)? {
        anyhow::bail!("That browser is no longer waiting. Run `kaku remote` for a new link.");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !kaku_remote::is_paired(&request.device.public_key) {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Allowed, but Kaku has not let the browser in. Check that Kaku is open, then run `kaku remote` again.");
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    println!(
        "{}",
        paint(GREEN, "Paired. The browser is opening your terminals.")
    );
    Ok(())
}

/// Why the link cannot open yet, when the running Kaku is not connected to
/// `relay`. None when it is, or when no running Kaku has tried yet.
#[cfg(feature = "remote")]
fn relay_down_note(status: Option<&kaku_remote::RelayStatus>, relay: &str) -> Option<String> {
    let status = status.filter(|s| s.relay == relay && s.is_current())?;
    if status.connected {
        return None;
    }
    let reason = if status.last_error.is_empty() {
        String::new()
    } else {
        format!(" ({})", status.last_error)
    };
    Some(format!(
        "This Mac is not connected to the relay right now{reason}, so the link shows offline until it is. Check this network, proxy or VPN; `kaku doctor` shows more."
    ))
}

/// The one-time pairing link, what to know about it, and the commands that
/// manage what it pairs, styled like the main menu when shown in a terminal.
#[cfg(feature = "remote")]
fn print_pairing(url: &str, relay_down: Option<&str>) {
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    };
    let minutes = kaku_remote::OFFER_LIFETIME.as_secs() / 60;
    println!();
    println!(
        "{}",
        paint(
            PURPLE_BOLD,
            "Open this link in a browser on another computer"
        )
    );
    println!("{url}");
    println!();
    if let Some(note) = relay_down {
        println!("{}", paint(YELLOW, note));
        println!();
    }
    for (step, note) in [
        format!("It works once, within {minutes} minutes, and only after you press Enter here for the code the browser shows."),
        "Bookmark the page it opens. That browser stays paired, so next time open the bookmark, no new link needed.".to_string(),
        "Keep Kaku open on this Mac; the page shows offline while it is closed or cannot reach the relay.".to_string(),
    ]
    .iter()
    .enumerate()
    {
        println!("{}", paint(GRAY, &format!("{}. {note}", step + 1)));
    }
    println!();
    println!("{}", paint(GRAY, "Later, to manage access:"));
    for (command, what) in [
        (
            "kaku remote devices",
            "See which browsers can open your terminals",
        ),
        (
            "kaku remote revoke <id>",
            "Unpair one, using an id from devices",
        ),
        (
            "kaku remote off",
            "Turn remote access off for every browser",
        ),
    ] {
        println!(
            "{}  {}",
            paint(GREEN, &format!("{command:<23}")),
            paint(GRAY, what)
        );
    }
}

/// Ask before turning remote access on, write it to kaku.lua, and wait for
/// the running Kaku to pick it up from its config reload.
#[cfg(feature = "remote")]
fn turn_remote_on() -> anyhow::Result<()> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Remote access is off. Run `kaku remote` in a terminal to turn it on.");
    }
    println!("Remote access lets a browser on another device use this Mac's terminals, through Kaku's relay.");
    print!("Turn it on? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes" | "Yes") {
        anyhow::bail!("Remote access stays off.");
    }
    if !config_tui::set_remote_enabled(true)? {
        anyhow::bail!("Your kaku.lua sets other remote options; set `enabled = true` in its `config.remote` yourself.");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while kaku_remote::read_state().is_err() {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Remote access is on. Open Kaku, then run `kaku remote` again.");
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(())
}

/// When the relay turned this Mac away for its access code, ask for the
/// code, save it, and wait for the running Kaku to try it.
#[cfg(feature = "remote")]
fn ensure_relay_access() -> anyhow::Result<()> {
    let current = || kaku_remote::read_relay_status().filter(|s| s.is_current());
    // Kaku records its first attempt a moment after it starts.
    let first = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while current().is_none() && std::time::Instant::now() < first {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let mut asked = false;
    loop {
        let Some(status) = current() else {
            return Ok(());
        };
        if status.last_error.contains("too many wrong access codes") {
            anyhow::bail!(
                "Too many wrong access codes from this network. Try again in 10 minutes."
            );
        }
        if !status.needs_access_code() {
            return Ok(());
        }
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "The relay asks for an access code. Run `kaku remote` in a terminal to enter it."
            );
        }
        if asked {
            println!("That access code was not accepted.");
        }
        let code = read_hidden_line("Access code: ")?;
        if code.trim().is_empty() {
            anyhow::bail!("No access code entered.");
        }
        kaku_remote::save_access_code(&code)?;
        asked = true;
        // Wait for an attempt that presented this very code.
        let tried = kaku_remote::access_fingerprint(&code);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            match current() {
                Some(s) if s.access == tried => break,
                Some(_) if std::time::Instant::now() < deadline => continue,
                _ => anyhow::bail!("Kaku did not reach the relay in time; see `kaku doctor`."),
            }
        }
    }
}

/// Read a line from the terminal without echoing it.
#[cfg(feature = "remote")]
fn read_hidden_line(prompt: &str) -> anyhow::Result<String> {
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let fd = std::io::stdin().as_raw_fd();
    let original = termios::Termios::from_fd(fd)?;
    let mut quiet = original;
    quiet.c_lflag &= !termios::ECHO;
    termios::tcsetattr(fd, termios::TCSANOW, &quiet)?;
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    termios::tcsetattr(fd, termios::TCSANOW, &original)?;
    println!();
    read?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Entry point of the `kaku` CLI. The release bundle has no separate CLI
/// binary: `Contents/MacOS/kaku` is a symlink to `kaku-gui`, which calls this
/// when it is started under that name.
pub fn cli_main() {
    config::designate_this_as_the_main_thread();
    config::assign_error_callback(mux::connui::show_configuration_error_message);
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
    Ok(config)
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
        SubCommand::Remote { reset, action } => {
            // Read the user's config: whether remote access is on, and where.
            init_config(&opts)?;
            run_remote(reset, action)
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
        SubCommand::Config(cmd) => cmd.run(
            opts.config_file.as_ref().map(PathBuf::from),
            opts.config_file.clone(),
            opts.config_override.clone(),
            opts.skip_config,
        ),
        SubCommand::Init(cmd) => cmd.run(),
        SubCommand::Doctor(cmd) => cmd.run(),
        SubCommand::Reset(cmd) => cmd.run(),
        SubCommand::Ai(cmd) => cmd.run(
            opts.config_file.clone(),
            opts.config_override.clone(),
            opts.skip_config,
        ),
        SubCommand::Chat(cmd) => cmd.run(),
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

const GREEN: &str = "\x1b[0;32m";
const PURPLE_BOLD: &str = "\x1b[1;35m";
const GRAY: &str = "\x1b[0;90m";
#[cfg(feature = "remote")]
const YELLOW: &str = "\x1b[0;33m";
const RESET: &str = "\x1b[0m";

fn select_main_menu_command() -> anyhow::Result<Option<SubCommand>> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

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
        (
            "ai",
            "Configure AI models, endpoints, and coding tools",
            MenuChoice::Ai,
        ),
        (
            "config",
            "Manage terminal and assistant settings",
            MenuChoice::Config,
        ),
        (
            "init",
            "Set up shell integration and install optional CLI tools",
            MenuChoice::Init,
        ),
        (
            "doctor",
            "Check shell integration and write a diagnostic bundle",
            MenuChoice::Doctor,
        ),
        (
            "update",
            "Check and install latest version",
            MenuChoice::Update,
        ),
        (
            "reset",
            "Remove shell and tmux integration, reset managed defaults",
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
            "{green} |_|\\_\\__,_||_|\\_\\ \\__,_|{reset}  {green}An AI-friendly Mac terminal, ready out of the box.{reset}\r\n"
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
                // These stay unguarded arms: a `selected > 0` style match guard
                // would let 'k'/'j' at the ends of the menu fall through to the
                // letter-shortcut arm below and launch a menu item.
                KeyCode::Up | KeyCode::Char('k') => {
                    let next = selected.saturating_sub(1);
                    if next != selected {
                        selected = next;
                        render_menu(selected, GREEN, PURPLE_BOLD, GRAY, RESET)?;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = (selected + 1).min(MENU_ITEMS.len().saturating_sub(1));
                    if next != selected {
                        selected = next;
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
                KeyCode::Char('q') | KeyCode::Char('Q')
                    if can_use_menu_char_shortcut(key.modifiers) =>
                {
                    return Ok(None);
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

    let exe = resolve_gui_executable("kaku-gui")?;

    let mut cmd = Command::new(&exe);
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

#[cfg(all(test, feature = "remote"))]
mod remote_tests {
    use super::relay_down_note;
    use kaku_remote::RelayStatus;

    fn status(connected: bool, error: &str, pid: u32) -> RelayStatus {
        RelayStatus {
            relay: "wss://r.example".to_string(),
            connected,
            last_error: error.to_string(),
            pid,
            ..Default::default()
        }
    }

    #[test]
    fn kaku_remote_says_why_a_link_would_open_offline() {
        let me = std::process::id();
        let relay = "wss://r.example";
        assert_eq!(relay_down_note(None, relay), None);
        assert_eq!(relay_down_note(Some(&status(true, "", me)), relay), None);
        let down = relay_down_note(Some(&status(false, "connect timed out", me)), relay).unwrap();
        assert!(down.contains("not connected to the relay"), "{}", down);
        assert!(down.contains("(connect timed out)"), "{}", down);
        // Another relay, or a Kaku that has quit, says nothing about this one.
        assert_eq!(
            relay_down_note(Some(&status(false, "x", me)), "wss://other"),
            None
        );
        assert_eq!(relay_down_note(Some(&status(false, "x", 0)), relay), None);
    }
}
