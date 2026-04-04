use anyhow::{anyhow, bail, Context};
use clap::Parser;
use std::io::{self, IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Parser, Clone, Default)]
pub struct ResetCommand {
    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

impl ResetCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        imp::run(self.yes)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::bail;

    pub fn run(_yes: bool) -> anyhow::Result<()> {
        bail!("`kaku reset` is currently supported on macOS only")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    const KAKU_SOURCE_PATTERN: &str = "kaku/zsh/kaku.zsh";
    const KAKU_PATH_MARKER: &str = "# Kaku PATH Integration";
    const KAKU_TMUX_SOURCE_PATTERN: &str = "kaku/tmux/kaku.tmux.conf";
    const KAKU_LEGACY_INLINE_MARKER: &str = "# Kaku Shell Integration";
    const KAKU_LEGACY_INLINE_VAR: &str = "KAKU_ZSH_DIR";
    const KAKU_LEGACY_SYNTAX_HINT: &str = "zsh-syntax-highlighting/zsh-syntax-highlighting.zsh";
    // SYNC: this list must stay in sync with the heredoc in
    // assets/shell-integration/setup_zsh.sh legacy_inline_block_has_only_kaku_managed_lines().
    // When adding or removing lines, update both places.
    const KAKU_LEGACY_INLINE_KNOWN_LINES: &[&str] = &[
        "# Kaku Zsh Integration - DO NOT EDIT MANUALLY",
        "# This file is managed by Kaku.app. Any changes may be overwritten.",
        r#"export KAKU_ZSH_DIR="$HOME/.config/kaku/zsh""#,
        "# Add bundled binaries to PATH",
        r#"export PATH="$KAKU_ZSH_DIR/bin:$PATH""#,
        "# Initialize Starship (Cross-shell prompt)",
        "# Check file existence to avoid \"no such file\" errors in some zsh configurations",
        r#"if [[ -x "$KAKU_ZSH_DIR/bin/starship" ]]; then"#,
        r#"    eval "$("$KAKU_ZSH_DIR/bin/starship" init zsh)""#,
        "elif command -v starship &> /dev/null; then",
        "    # Fallback to system starship if available",
        r#"    eval "$(starship init zsh)""#,
        "fi",
        "# Enable color output for ls",
        "export CLICOLOR=1",
        r#"export LSCOLORS="Gxfxcxdxbxegedabagacad""#,
        "# Smart History Configuration",
        "HISTSIZE=50000",
        "SAVEHIST=50000",
        r#"HISTFILE="$HOME/.zsh_history""#,
        "setopt HIST_IGNORE_DUPS          # Do not record an event that was just recorded again",
        "setopt HIST_IGNORE_SPACE         # Do not record an event starting with a space",
        "setopt HIST_FIND_NO_DUPS         # Do not display a line previously found",
        "setopt SHARE_HISTORY             # Share history between all sessions",
        "setopt APPEND_HISTORY            # Append history to the history file (no overwriting)",
        "# Set default Zsh options",
        "setopt interactive_comments",
        "bindkey -e",
        "# Directory Navigation Options",
        "setopt auto_cd",
        "setopt auto_pushd",
        "setopt pushd_ignore_dups",
        "setopt pushdminus",
        "# Common Aliases (Intuitive defaults)",
        "alias ll='ls -lhF'   # Detailed list (human-readable sizes, no hidden files)",
        "alias la='ls -lAhF'  # List all (including hidden, except . and ..)",
        "alias l='ls -CF'     # Compact list",
        "# Directory Navigation",
        "alias ...='../..'",
        "alias ....='../../..'",
        "alias .....='../../../..'",
        "alias ......='../../../../..'",
        "alias md='mkdir -p'",
        "alias rd=rmdir",
        "# Grep Colors",
        "alias grep='grep --color=auto'",
        "alias egrep='grep -E --color=auto'",
        "alias fgrep='grep -F --color=auto'",
        "# Common Git Aliases (The Essentials)",
        "alias g='git'",
        "alias ga='git add'",
        "alias gaa='git add --all'",
        "alias gb='git branch'",
        "alias gbd='git branch -d'",
        "alias gc='git commit -v'",
        "alias gcmsg='git commit -m'",
        "alias gco='git checkout'",
        "alias gcb='git checkout -b'",
        "alias gd='git diff'",
        "alias gds='git diff --staged'",
        "alias gf='git fetch'",
        "alias gl='git pull'",
        "alias gp='git push'",
        "alias gst='git status'",
        "alias gss='git status -s'",
        "alias glo='git log --oneline --decorate'",
        "alias glg='git log --stat'",
        "alias glgp='git log --stat -p'",
        "# Load Plugins (Performance Optimized)",
        "# Load zsh-completions into fpath before compinit",
        r#"if [[ -d "$KAKU_ZSH_DIR/plugins/zsh-completions/src" ]]; then"#,
        r#"    fpath=("$KAKU_ZSH_DIR/plugins/zsh-completions/src" $fpath)"#,
        "# Optimized compinit: Use cache and only rebuild when needed (~30ms saved)",
        "autoload -Uz compinit",
        r#"if [[ -n "${ZDOTDIR:-$HOME}/.zcompdump"(#qN.mh+24) ]]; then"#,
        "    # Rebuild completion cache if older than 24 hours",
        "    compinit",
        "else",
        "    # Load from cache (much faster)",
        "    compinit -C",
        "# Load zsh-z (smart directory jumping) - Fast, no delay needed",
        r#"if [[ -f "$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh" ]]; then"#,
        "    # Default to smart case matching so `z kaku` prefers `Kaku` over lowercase",
        "    # path entries. Users can still override this in their own shell config.",
        r#"    : "${ZSHZ_CASE:=smart}""#,
        "    export ZSHZ_CASE",
        r#"    source "$KAKU_ZSH_DIR/plugins/zsh-z/zsh-z.plugin.zsh""#,
        "# Load zoxide (smart directory jumping) if not already provided by user config.",
        r#"if command -v zoxide &>/dev/null && ! (( ${+functions[__zoxide_z]} )); then"#,
        r#"    eval "$(zoxide init zsh)""#,
        "# Load zsh-autosuggestions - Async, minimal impact",
        r#"if [[ -f "$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh" ]]; then"#,
        r#"    source "$KAKU_ZSH_DIR/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh""#,
        "    # Smart Tab: accept inline autosuggestion if present, otherwise run completion.",
        "    # Avoids running completion immediately after accepting a suggestion, which can",
        "    # introduce unexpected spacing for some command completers.",
        "    # Keep this widget out of autosuggestions rebinding, otherwise POSTDISPLAY is",
        "    # cleared before our condition check and Tab always falls back to completion.",
        "    typeset -ga ZSH_AUTOSUGGEST_IGNORE_WIDGETS",
        "    ZSH_AUTOSUGGEST_IGNORE_WIDGETS+=(kaku_tab_accept_or_complete)",
        "    kaku_tab_accept_or_complete() {",
        r#"        if [[ -n "$POSTDISPLAY" ]]; then"#,
        "            zle autosuggest-accept",
        "        else",
        "            zle expand-or-complete",
        "        fi",
        "    }",
        "    zle -N kaku_tab_accept_or_complete",
        r#"    bindkey -M emacs '^I' kaku_tab_accept_or_complete"#,
        r#"    bindkey -M main '^I' kaku_tab_accept_or_complete"#,
        r#"    bindkey -M viins '^I' kaku_tab_accept_or_complete"#,
        "# Defer zsh-syntax-highlighting to first prompt (~40ms saved at startup)",
        "# This plugin must be loaded LAST, and we delay it for faster shell startup",
        r#"source "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh""#,
        r#"if [[ -f "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" ]]; then"#,
        "    # Simplified highlighters for better performance (removed brackets, pattern, cursor)",
        "    export ZSH_HIGHLIGHT_HIGHLIGHTERS=(main)",
        "    # Defer loading until first prompt display",
        "    zsh_syntax_highlighting_defer() {",
        r#"        source "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh""#,
        "        # Remove this hook after first run",
        r#"        precmd_functions=("${precmd_functions[@]:#zsh_syntax_highlighting_defer}")"#,
        "    }",
        "    # Hook into precmd (runs before prompt is displayed)",
        "    precmd_functions+=(zsh_syntax_highlighting_defer)",
        "# Defer fast-syntax-highlighting to first prompt (~40ms saved at startup)",
        "# This plugin must be loaded LAST, and we delay it for faster shell startup.",
        r#"if ! (( ${+functions[_zsh_highlight]} )) && [[ -f "$KAKU_ZSH_DIR/plugins/fast-syntax-highlighting/fast-syntax-highlighting.plugin.zsh" ]]; then"#,
        "    fast_syntax_highlighting_defer() {",
        r#"        source "$KAKU_ZSH_DIR/plugins/fast-syntax-highlighting/fast-syntax-highlighting.plugin.zsh""#,
        r#"        precmd_functions=("${precmd_functions[@]:#fast_syntax_highlighting_defer}")"#,
        "    }",
        "    precmd_functions+=(fast_syntax_highlighting_defer)",
    ];

    const KAKU_GIT_DEFAULTS: &[(&str, &str)] = &[
        ("core.pager", "delta"),
        ("interactive.diffFilter", "delta --color-only"),
        ("delta.navigate", "true"),
        ("delta.pager", "less --mouse --wheel-lines=3 -R -F -X"),
        ("delta.line-numbers", "true"),
        ("delta.side-by-side", "true"),
        ("delta.line-fill-method", "spaces"),
        ("delta.syntax-theme", "Coldark-Dark"),
        ("delta.file-style", "omit"),
        ("delta.file-decoration-style", "omit"),
        ("delta.hunk-header-style", "file line-number syntax"),
    ];

    #[derive(Default)]
    struct ResetReport {
        changed: Vec<String>,
        skipped: Vec<String>,
    }

    impl ResetReport {
        fn changed(&mut self, msg: impl Into<String>) {
            self.changed.push(msg.into());
        }

        fn skipped(&mut self, msg: impl Into<String>) {
            self.skipped.push(msg.into());
        }

        fn print(self) {
            if !self.changed.is_empty() {
                println!("Applied reset actions:");
                for line in &self.changed {
                    println!("  - {}", line);
                }
            }

            if !self.skipped.is_empty() {
                println!("\nSkipped:");
                for line in &self.skipped {
                    println!("  - {}", line);
                }
            }

            println!("\nKaku reset completed.");
        }
    }

    pub fn run(yes: bool) -> anyhow::Result<()> {
        confirm_reset(yes)?;

        let mut report = ResetReport::default();

        remove_zsh_integration(&mut report)?;
        remove_kaku_shell_dir(&mut report)?;
        remove_fish_integration(&mut report)?;
        remove_tmux_integration(&mut report)?;
        remove_file_if_exists(
            config_home().join("tmux").join("kaku.tmux.conf"),
            "removed managed tmux integration script",
            &mut report,
        )?;
        cleanup_git_delta_defaults(&mut report)?;
        cleanup_theme_block(&mut report)?;
        remove_file_if_exists(
            config_home().join("state.json"),
            "removed persisted Kaku state",
            &mut report,
        )?;
        remove_file_if_exists(
            config_home().join(".kaku_config_version"),
            "removed legacy Kaku config version marker",
            &mut report,
        )?;
        remove_file_if_exists(
            config_home().join(".kaku_window_geometry"),
            "removed legacy Kaku window geometry marker",
            &mut report,
        )?;
        remove_file_if_exists(
            config_home().join("lazygit_state.json"),
            "removed Lazygit hint state",
            &mut report,
        )?;
        remove_dir_if_exists(
            config_home().join("backups"),
            "removed Kaku backup directory",
            &mut report,
        )?;
        remove_empty_kaku_config_dir(&mut report)?;

        report.print();

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let is_fish = std::path::Path::new(&shell)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "fish")
            .unwrap_or(false);

        let tools_dir = if is_fish {
            "~/.config/kaku/fish/"
        } else {
            "~/.config/kaku/zsh/"
        };
        let restart_cmd = if is_fish {
            "exec fish -l"
        } else {
            "exec zsh -l"
        };

        println!("\nShell restart required.");
        println!("Tools preserved in {}\n", tools_dir);

        if !yes && io::stdin().is_terminal() {
            print!("Restart shell now? [Y/n] ");
            io::stdout().flush().context("flush stdout")?;

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .context("read restart confirmation")?;

            let answer = input.trim().to_ascii_lowercase();
            if answer.is_empty() || answer == "y" || answer == "yes" {
                println!("\nRestarting shell...");
                println!("Tip: Run 'kaku init' to restore integration");
                let err = std::process::Command::new(&shell).arg("-l").exec();
                bail!("failed to restart shell: {}", err);
            } else {
                println!(
                    "\nRun '{}' when ready. Restore with 'kaku init'",
                    restart_cmd
                );
            }
        } else {
            println!("Run '{}' to restart. Restore with 'kaku init'", restart_cmd);
        }

        Ok(())
    }

    fn confirm_reset(yes: bool) -> anyhow::Result<()> {
        if yes {
            return Ok(());
        }

        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            bail!("non-interactive terminal detected; rerun with --yes to confirm reset")
        }

        println!(
            "This will remove Kaku shell and tmux integration and reset Kaku-managed git defaults."
        );
        print!("Continue with reset? [y/N] ");
        io::stdout().flush().context("flush stdout")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("read reset confirmation")?;

        let answer = input.trim().to_ascii_lowercase();
        if answer == "y" || answer == "yes" {
            Ok(())
        } else {
            println!("Reset cancelled.");
            std::process::exit(0);
        }
    }

    fn home_dir() -> PathBuf {
        config::HOME_DIR.clone()
    }

    fn config_home() -> PathBuf {
        home_dir().join(".config").join("kaku")
    }

    fn zshrc_path() -> PathBuf {
        if let Some(zdotdir) = std::env::var_os("ZDOTDIR") {
            PathBuf::from(zdotdir).join(".zshrc")
        } else {
            home_dir().join(".zshrc")
        }
    }

    fn tmuxrc_path() -> PathBuf {
        home_dir().join(".tmux.conf")
    }

    fn contains_tmux_source_command(line: &str) -> bool {
        line.split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')'))
            .any(|token| token == "source-file")
    }

    fn is_active_kaku_tmux_source_line(trimmed_line: &str) -> bool {
        if trimmed_line.starts_with('#') || !trimmed_line.contains(KAKU_TMUX_SOURCE_PATTERN) {
            return false;
        }
        contains_tmux_source_command(trimmed_line)
    }

    fn remove_zsh_integration(report: &mut ResetReport) -> anyhow::Result<()> {
        let zshrc = zshrc_path();
        if !zshrc.exists() {
            report.skipped(format!("{} not found", zshrc.display()));
            return Ok(());
        }

        let original =
            std::fs::read_to_string(&zshrc).with_context(|| format!("read {}", zshrc.display()))?;
        let filtered: Vec<&str> = original
            .lines()
            .filter(|line| !line.contains(KAKU_SOURCE_PATTERN) && !line.contains(KAKU_PATH_MARKER))
            .collect();
        let removed_managed_lines = filtered.len() != original.lines().count();

        let mut updated = filtered.join("\n");
        if !updated.is_empty() {
            updated.push('\n');
        }

        let (updated, removed_legacy_block) = strip_legacy_inline_block(&updated);
        if !removed_managed_lines && !removed_legacy_block {
            report.skipped(format!(
                "no Kaku shell integration found in {}",
                zshrc.display()
            ));
            return Ok(());
        }

        std::fs::write(&zshrc, updated).with_context(|| format!("write {}", zshrc.display()))?;
        if removed_managed_lines && removed_legacy_block {
            report.changed(format!(
                "removed Kaku-managed .zshrc lines and legacy inline block from {}",
                zshrc.display()
            ));
        } else if removed_managed_lines {
            report.changed(format!(
                "removed Kaku-managed .zshrc lines from {}",
                zshrc.display()
            ));
        } else {
            report.changed(format!(
                "removed legacy inline Kaku block from {}",
                zshrc.display()
            ));
        }
        Ok(())
    }

    fn remove_kaku_shell_dir(report: &mut ResetReport) -> anyhow::Result<()> {
        let kaku_init = config_home().join("zsh").join("kaku.zsh");
        if kaku_init.exists() {
            std::fs::remove_file(&kaku_init)
                .with_context(|| format!("remove {}", kaku_init.display()))?;
            report.changed(format!("removed {}", kaku_init.display()));
        } else {
            report.skipped(format!("{} not found", kaku_init.display()));
        }
        Ok(())
    }

    fn remove_fish_integration(report: &mut ResetReport) -> anyhow::Result<()> {
        // Remove the conf.d entry point (sourced by fish on startup)
        let conf_d_file = home_dir()
            .join(".config")
            .join("fish")
            .join("conf.d")
            .join("kaku.fish");
        remove_file_if_exists(
            conf_d_file,
            "removed ~/.config/fish/conf.d/kaku.fish",
            report,
        )?;

        // Remove managed fish init file
        let fish_init = config_home().join("fish").join("kaku.fish");
        remove_file_if_exists(fish_init, "removed ~/.config/kaku/fish/kaku.fish", report)?;

        // Remove fish wrapper bin
        let fish_wrapper = config_home().join("fish").join("bin").join("kaku");
        remove_file_if_exists(
            fish_wrapper,
            "removed ~/.config/kaku/fish/bin/kaku wrapper",
            report,
        )?;

        Ok(())
    }

    fn remove_tmux_integration(report: &mut ResetReport) -> anyhow::Result<()> {
        let tmuxrc = tmuxrc_path();
        if !tmuxrc.exists() {
            report.skipped(format!("{} not found", tmuxrc.display()));
            return Ok(());
        }

        let original = std::fs::read_to_string(&tmuxrc)
            .with_context(|| format!("read {}", tmuxrc.display()))?;
        let filtered: Vec<&str> = original
            .lines()
            .filter(|line| !is_active_kaku_tmux_source_line(line.trim_start()))
            .collect();

        if filtered.len() == original.lines().count() {
            report.skipped(format!(
                "no Kaku tmux integration found in {}",
                tmuxrc.display()
            ));
            return Ok(());
        }

        let mut updated = filtered.join("\n");
        if !updated.is_empty() {
            updated.push('\n');
        }
        std::fs::write(&tmuxrc, updated).with_context(|| format!("write {}", tmuxrc.display()))?;
        report.changed(format!(
            "removed Kaku-managed tmux source line from {}",
            tmuxrc.display()
        ));
        Ok(())
    }

    fn cleanup_git_delta_defaults(report: &mut ResetReport) -> anyhow::Result<()> {
        if !command_exists("git") {
            report.skipped("git not found; skipped git config cleanup");
            return Ok(());
        }

        let mut removed = Vec::new();
        for (key, expected) in KAKU_GIT_DEFAULTS {
            if unset_git_key_if_matches(key, expected)? {
                removed.push(*key);
            }
        }

        if removed.is_empty() {
            report.skipped("no Kaku-managed git defaults to remove");
        } else {
            report.changed(format!("removed git defaults: {}", removed.join(", ")));
        }

        Ok(())
    }

    fn unset_git_key_if_matches(key: &str, expected: &str) -> anyhow::Result<bool> {
        let output = Command::new("git")
            .args(["config", "--global", "--get-all", key])
            .output()
            .with_context(|| format!("query git config key {}", key))?;

        if !output.status.success() {
            if output.status.code() == Some(1) {
                return Ok(false);
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "git config --get-all {} failed: {}",
                key,
                stderr.trim()
            ));
        }

        let values: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();

        if values.is_empty() || values.iter().any(|v| v != expected) {
            return Ok(false);
        }

        let status = Command::new("git")
            .args(["config", "--global", "--unset-all", key])
            .status()
            .with_context(|| format!("unset git config key {}", key))?;

        Ok(status.success())
    }

    fn command_exists(name: &str) -> bool {
        Command::new(name)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn cleanup_theme_block(report: &mut ResetReport) -> anyhow::Result<()> {
        let config_path = config_home().join("kaku.lua");
        if !config_path.exists() {
            report.skipped(format!("{} not found", config_path.display()));
            return Ok(());
        }

        let original = std::fs::read_to_string(&config_path)
            .with_context(|| format!("read {}", config_path.display()))?;

        let (after_managed, changed_managed) =
            strip_theme_block(&original, "-- ===== Kaku Theme Defaults (managed) =====");
        let (after_legacy, changed_legacy) =
            strip_theme_block(&after_managed, "-- ===== Kaku Theme =====");

        if !changed_managed && !changed_legacy {
            report.skipped("no managed Kaku theme block found in ~/.config/kaku/kaku.lua");
            return Ok(());
        }

        std::fs::write(&config_path, after_legacy)
            .with_context(|| format!("write {}", config_path.display()))?;
        report.changed("removed managed Kaku theme block from ~/.config/kaku/kaku.lua");
        Ok(())
    }

    fn strip_theme_block(content: &str, marker: &str) -> (String, bool) {
        let lines: Vec<&str> = content.lines().collect();
        let Some(start) = lines.iter().position(|line| line.contains(marker)) else {
            return (content.to_string(), false);
        };

        // Safety-first: only strip blocks that have a clear `return config`
        // terminator after the marker. Otherwise we leave user content untouched.
        let return_after = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.trim() == "return config")
            .map(|(idx, _)| idx);
        let Some(return_idx) = return_after else {
            return (content.to_string(), false);
        };

        let mut out = Vec::new();
        out.extend_from_slice(&lines[..start]);

        let return_before = out.iter().any(|line| line.trim() == "return config");
        if !return_before {
            out.push("return config");
        }
        out.extend_from_slice(&lines[return_idx + 1..]);

        let mut merged = out.join("\n");
        if !merged.is_empty() {
            merged.push('\n');
        }

        (merged, true)
    }

    /// Strips the old inline Kaku shell integration block that was written directly
    /// into .zshrc in early Kaku versions (before the source-based kaku.zsh approach).
    /// The block starts with `# Kaku Shell Integration`, contains `KAKU_ZSH_DIR`, and
    /// ends with a `fi` that follows the zsh-syntax-highlighting line.
    fn strip_legacy_inline_block(content: &str) -> (String, bool) {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return (content.to_string(), false);
        }

        let mut out = Vec::with_capacity(lines.len());
        let mut i = 0usize;
        let mut changed = false;

        while i < lines.len() {
            let current = lines[i].trim();
            if current == KAKU_LEGACY_INLINE_MARKER {
                let mut j = i + 1;
                let mut saw_kaku_var = false;
                let mut saw_syntax_line = false;
                let mut end = None;

                while j < lines.len() {
                    let line = lines[j];
                    if line.contains(KAKU_LEGACY_INLINE_VAR) {
                        saw_kaku_var = true;
                    }
                    if line.contains(KAKU_LEGACY_SYNTAX_HINT) {
                        saw_syntax_line = true;
                    }
                    if saw_syntax_line && line.trim() == "fi" {
                        end = Some(j);
                        break;
                    }
                    if j.saturating_sub(i) > 600 {
                        break;
                    }
                    j += 1;
                }

                if saw_kaku_var {
                    if let Some(end_idx) = end {
                        let block_lines = &lines[i + 1..=end_idx];
                        if block_lines
                            .iter()
                            .all(|line| is_known_legacy_inline_line(line))
                        {
                            changed = true;
                            i = end_idx + 1;
                            while i < lines.len() && lines[i].trim().is_empty() {
                                i += 1;
                            }
                            continue;
                        }
                    } else {
                        return (content.to_string(), false);
                    }
                }
            }

            out.push(lines[i]);
            i += 1;
        }

        if !changed {
            return (content.to_string(), false);
        }

        let mut merged = out.join("\n");
        if !merged.is_empty() {
            merged.push('\n');
        }
        (merged, true)
    }

    fn is_known_legacy_inline_line(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.is_empty() || KAKU_LEGACY_INLINE_KNOWN_LINES.contains(&trimmed)
    }

    fn remove_file_if_exists(
        path: PathBuf,
        changed_msg: &str,
        report: &mut ResetReport,
    ) -> anyhow::Result<()> {
        if !path.exists() {
            report.skipped(format!("{} not found", path.display()));
            return Ok(());
        }

        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        report.changed(changed_msg.to_string());
        Ok(())
    }

    fn remove_dir_if_exists(
        path: PathBuf,
        changed_msg: &str,
        report: &mut ResetReport,
    ) -> anyhow::Result<()> {
        if !path.exists() {
            report.skipped(format!("{} not found", path.display()));
            return Ok(());
        }

        std::fs::remove_dir_all(&path).with_context(|| format!("remove {}", path.display()))?;
        report.changed(changed_msg.to_string());
        Ok(())
    }

    fn remove_empty_kaku_config_dir(report: &mut ResetReport) -> anyhow::Result<()> {
        let dir = config_home();
        if !dir.exists() {
            return Ok(());
        }

        if is_dir_empty(&dir)? {
            std::fs::remove_dir(&dir).with_context(|| format!("remove {}", dir.display()))?;
            report.changed(format!("removed empty {}", dir.display()));
        }

        Ok(())
    }

    fn is_dir_empty(path: &Path) -> anyhow::Result<bool> {
        let mut iter =
            std::fs::read_dir(path).with_context(|| format!("read {}", path.display()))?;
        Ok(iter.next().is_none())
    }

    #[cfg(test)]
    mod tests {
        use super::{
            is_active_kaku_tmux_source_line, strip_legacy_inline_block, KAKU_TMUX_SOURCE_PATTERN,
        };

        #[test]
        fn active_tmux_source_line_is_detected() {
            let line =
                r#"source-file "$HOME/.config/kaku/tmux/kaku.tmux.conf" # Kaku tmux Integration"#;
            assert!(is_active_kaku_tmux_source_line(line));
        }

        #[test]
        fn commented_tmux_source_line_is_ignored() {
            let line = r#"# source-file "$HOME/.config/kaku/tmux/kaku.tmux.conf""#;
            assert!(!is_active_kaku_tmux_source_line(line));
        }

        #[test]
        fn plain_text_reference_is_ignored() {
            let line = format!("note: {}", KAKU_TMUX_SOURCE_PATTERN);
            assert!(!is_active_kaku_tmux_source_line(&line));
        }

        #[test]
        fn strip_legacy_inline_block_removes_known_minimal_block() {
            let input = r#"export PATH="$HOME/bin:$PATH"
# Kaku Shell Integration
export KAKU_ZSH_DIR="$HOME/.config/kaku/zsh"
source "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"
fi
export FOO=bar
"#;

            let (updated, changed) = strip_legacy_inline_block(input);
            assert!(changed);
            assert_eq!(
                updated,
                r#"export PATH="$HOME/bin:$PATH"
export FOO=bar
"#
            );
        }

        #[test]
        fn strip_legacy_inline_block_preserves_unknown_user_lines() {
            let input = r#"export PATH="$HOME/bin:$PATH"
# Kaku Shell Integration
export KAKU_ZSH_DIR="$HOME/.config/kaku/zsh"
export PATH="$HOME/.claude/bin:$PATH"
source "$HOME/.claude/shell/zshrc"
source "$KAKU_ZSH_DIR/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"
fi
export FOO=bar
"#;

            let (updated, changed) = strip_legacy_inline_block(input);
            assert!(!changed);
            assert_eq!(updated, input);
        }
    }
}
