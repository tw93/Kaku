//! Thin entry point for the `k` standalone AI chat CLI.

use clap::Parser;
use kaku_gui_lib::cli_chat::{run, CliArgs};

// The `k` CLI does not load the full Kaku Lua config (it would be
// wasteful for a one-shot chat invocation). Locale is resolved from the
// process environment only — `kaku.lua`'s `config.language` is honored
// inside the desktop app, not here.
fn apply_locale_from_environment() {
    let locale = config::i18n::resolve_locale(config::i18n::LANGUAGE_AUTO);
    rust_i18n::set_locale(&locale);
}

#[derive(Parser)]
#[command(
    name = "k",
    about = "AI chat from any terminal",
    long_about = "Slash commands (interactive mode): /new  /resume [id]  /clear  /status  /memory  /exit\n\nThe CLI intentionally supports a smaller command set than the Cmd+L overlay."
)]
struct Cli {
    /// One-shot query (omit for interactive mode)
    prompt: Vec<String>,
    /// Force a new conversation
    #[arg(long, short)]
    new: bool,
    /// List recent conversations or resume by ID
    #[arg(long, short = 'r', value_name = "ID", num_args = 0..=1, default_missing_value = "")]
    resume: Option<String>,
}

fn main() {
    apply_locale_from_environment();
    let cli = Cli::parse();
    let args = CliArgs {
        prompt: if cli.prompt.is_empty() {
            None
        } else {
            Some(cli.prompt.join(" "))
        },
        new: cli.new,
        resume: cli
            .resume
            .map(|id| if id.is_empty() { None } else { Some(id) }),
    };
    if let Err(e) = run(args) {
        eprintln!("k: {}", e);
        std::process::exit(1);
    }
}
