use crate::termwindow::{PaneInformation, TabInformation, UIItem, UIItemType};
use config::{ConfigHandle, TabBarColors};
use finl_unicode::grapheme_clusters::Graphemes;
use mlua::FromLua;
use mux::pane::{CachePolicy, Pane};
use mux::tab::TabId;
use mux::Mux;
use std::path::Path;
use termwiz::cell::{unicode_column_width, Cell, CellAttributes};
use termwiz::color::ColorSpec;
use termwiz::escape::csi::Sgr;
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode, CSI};
use termwiz::surface::SEQ_ZERO;
use termwiz_funcs::{format_as_escapes, FormatColor, FormatItem};
use wezterm_term::{Line, Progress};
use window::{IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct TabBarState {
    line: Line,
    items: Vec<TabEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBarItem {
    None,
    LeftStatus,
    RightStatus,
    Tab { tab_idx: usize, active: bool },
    NewTabButton,
    WindowButton(IntegratedTitleButton),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabEntry {
    pub item: TabBarItem,
    pub title: Line,
    pub progress: Progress,
    x: usize,
    width: usize,
}

#[derive(Clone, Debug)]
struct TitleText {
    items: Vec<FormatItem>,
}

/// The visible text of a tab title, without colors or attributes.
#[cfg(feature = "remote")]
fn title_plain_text(title: &TitleText) -> String {
    title
        .items
        .iter()
        .filter_map(|item| match item {
            FormatItem::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Debug)]
struct BatchTabTitles {
    callback_present: bool,
    titles: Vec<Option<TitleText>>,
}

impl BatchTabTitles {
    fn without_callback(count: usize) -> Self {
        Self {
            callback_present: false,
            titles: vec![None; count],
        }
    }
}

fn parse_format_tab_title_result<'lua>(
    v: mlua::Value<'lua>,
    lua: &'lua mlua::Lua,
) -> mlua::Result<Option<TitleText>> {
    match &v {
        mlua::Value::Nil => Ok(None),
        mlua::Value::Table(_) => {
            let items = <Vec<FormatItem>>::from_lua(v, lua)?;
            // Validate table payload from Lua early so downstream
            // format_as_escapes(...).expect() stays infallible.
            let _ = format_as_escapes(items.clone()).map_err(mlua::Error::external)?;
            Ok(Some(TitleText { items }))
        }
        _ => {
            let s = String::from_lua(v, lua)?;
            Ok(Some(TitleText {
                items: vec![FormatItem::Text(s)],
            }))
        }
    }
}

fn has_format_tab_title_callback(lua: &mlua::Lua) -> mlua::Result<bool> {
    let tbl: mlua::Value = lua.named_registry_value("wezterm-event-format-tab-title")?;
    Ok(matches!(tbl, mlua::Value::Table(_)))
}

/// The title a fresh terminal reports before any program sets one through
/// OSC 0/2 (`term/src/terminalstate/mod.rs`). It is a placeholder, not a title
/// an application chose, so `tab_title_use_pane_title` must fall through it.
const DEFAULT_PANE_TITLE: &str = "kaku";

/// The pane title an application set through OSC 0/2, when the user opted into
/// showing it with `tab_title_use_pane_title`.
fn app_pane_title<'a>(config: &ConfigHandle, pane: &'a PaneInformation) -> Option<&'a str> {
    (config.tab_title_use_pane_title && !pane.title.is_empty() && pane.title != DEFAULT_PANE_TITLE)
        .then_some(pane.title.as_str())
}

fn ssh_destination_for_default_title(
    tab: &TabInformation,
    config: &ConfigHandle,
) -> Option<String> {
    let pane = tab.active_pane.as_ref()?;
    if tab.tab_title.is_empty() && app_pane_title(config, pane).is_none() {
        ssh_destination_for_pane(pane)
    } else {
        None
    }
}

fn call_format_tab_titles_batch_with_lua(
    lua: &mlua::Lua,
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> mlua::Result<BatchTabTitles> {
    let n = tab_info.len();
    if !has_format_tab_title_callback(lua)? {
        return Ok(BatchTabTitles::without_callback(n));
    }

    // Serialize shared data once for all tabs.
    let tabs = lua.create_sequence_from(tab_info.iter().cloned())?;
    let panes = lua.create_sequence_from(pane_info.iter().cloned())?;
    let lua_config = luahelper::to_lua(lua, (**config).clone())?;

    let mut results = Vec::with_capacity(n);
    for tab in tab_info {
        // Default SSH titles skip Lua unless pane titles are enabled.
        if ssh_destination_for_default_title(tab, config).is_some() {
            results.push(None);
            continue;
        }

        let result = config::lua::emit_sync_callback(
            lua,
            (
                "format-tab-title".to_string(),
                (
                    tab.clone(),
                    tabs.clone(),
                    panes.clone(),
                    lua_config.clone(),
                    false,
                    tab_max_width,
                ),
            ),
        )
        .and_then(|v| parse_format_tab_title_result(v, lua));
        match result {
            Ok(title) => results.push(title),
            Err(err) => {
                log::warn!("format-tab-title: {}", err);
                results.push(None);
            }
        }
    }

    Ok(BatchTabTitles {
        callback_present: true,
        titles: results,
    })
}

/// Calls format-tab-title for all tabs in a single Lua scope, serializing
/// Config, tabs, and panes sequences only once instead of once per tab.
/// Returns None for default SSH titles or when no callback is registered.
fn call_format_tab_titles_batch(
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> BatchTabTitles {
    let n = tab_info.len();
    match config::run_immediate_with_lua_config(|lua| {
        let Some(lua) = lua else {
            return Ok(BatchTabTitles::without_callback(n));
        };
        Ok(call_format_tab_titles_batch_with_lua(
            &lua,
            tab_info,
            pane_info,
            config,
            tab_max_width,
        )?)
    }) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("format-tab-title (batch): {}", err);
            BatchTabTitles::without_callback(n)
        }
    }
}

/// Calls format-tab-title for a single tab with hover=true.
/// Only invoked when the mouse is actually over a non-active tab and a
/// format-tab-title callback is registered.
fn call_format_tab_title_hover_with_lua(
    lua: &mlua::Lua,
    tab: &TabInformation,
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> mlua::Result<Option<TitleText>> {
    let tabs = lua.create_sequence_from(tab_info.iter().cloned())?;
    let panes = lua.create_sequence_from(pane_info.iter().cloned())?;
    let v = config::lua::emit_sync_callback(
        lua,
        (
            "format-tab-title".to_string(),
            (
                tab.clone(),
                tabs,
                panes,
                (**config).clone(),
                true,
                tab_max_width,
            ),
        ),
    )?;
    parse_format_tab_title_result(v, lua)
}

fn call_format_tab_title_hover(
    tab: &TabInformation,
    tab_info: &[TabInformation],
    pane_info: &[PaneInformation],
    config: &ConfigHandle,
    tab_max_width: usize,
) -> Option<TitleText> {
    match config::run_immediate_with_lua_config(|lua| {
        let Some(lua) = lua else {
            return Ok(None);
        };
        Ok(call_format_tab_title_hover_with_lua(
            &lua,
            tab,
            tab_info,
            pane_info,
            config,
            tab_max_width,
        )?)
    }) {
        Ok(s) => s,
        Err(err) => {
            log::warn!("format-tab-title (hover): {}", err);
            None
        }
    }
}

const CONTEXT_PROCESS_SEPARATOR: &str = "\u{00b7}";
const MULTI_PANE_TITLE_SEPARATOR: &str = "\u{2219}";

fn path_title_from_str(path: &str) -> Option<String> {
    let path_str = path.trim_end_matches('/');
    if path_str.is_empty() {
        return None;
    }

    let path = Path::new(path_str);
    let current = path
        .file_name()
        .and_then(|n| n.to_str())
        .or_else(|| path.to_str())?;
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if parent.is_empty() {
        Some(current.to_string())
    } else {
        Some(format!("{parent}/{current}"))
    }
}

fn context_process_title(context: Option<&str>, process: Option<&str>) -> Option<String> {
    let context = context.filter(|s| !s.is_empty());
    let process = process.filter(|s| !s.is_empty());

    match (context, process) {
        (Some(context), Some(process)) if context == process => Some(context.to_string()),
        (Some(context), Some(process)) => {
            Some(format!("{context}{CONTEXT_PROCESS_SEPARATOR}{process}"))
        }
        (Some(context), None) => Some(context.to_string()),
        (None, Some(process)) => Some(process.to_string()),
        (None, None) => None,
    }
}

fn tab_multi_pane_title(tab_id: TabId, include_foreground_process: bool) -> Option<String> {
    let mux = Mux::try_get()?;
    let tab = mux.get_tab(tab_id)?;
    let panes = tab.iter_panes();
    if panes.len() <= 1 {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for pos in panes.iter() {
        let Some(real_pane) = mux.get_pane(pos.pane.pane_id()) else {
            continue;
        };
        // A remote pane's cwd path is on the other host; the host name is the
        // useful context there, not the path or the local `ssh` process.
        let segment = if let Some(host) = ssh_destination_for_real_pane(&real_pane) {
            ssh_title(&host)
        } else {
            let process_title = if include_foreground_process {
                foreground_process_title(&*real_pane)
            } else {
                None
            };
            let path_title = real_pane
                .get_current_working_dir(CachePolicy::AllowStale)
                .and_then(|cwd| path_title_from_str(cwd.path()));
            let Some(segment) =
                context_process_title(path_title.as_deref(), process_title.as_deref())
            else {
                continue;
            };
            segment
        };
        if !parts.iter().any(|p| p == &segment) {
            parts.push(segment);
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(MULTI_PANE_TITLE_SEPARATOR))
}

fn compute_tab_title_from_precomputed(
    tab: &TabInformation,
    config: &ConfigHandle,
    precomputed: Option<TitleText>,
) -> TitleText {
    if let Some(ssh_host) = ssh_destination_for_default_title(tab, config) {
        return build_default_title(tab, config, &ssh_title(&ssh_host), false, true);
    }
    match precomputed {
        Some(title) => title,
        None => {
            if let Some(pane) = &tab.active_pane {
                let title = if !tab.tab_title.is_empty() {
                    tab.tab_title.clone()
                } else if let Some(app_title) = app_pane_title(config, pane) {
                    app_title.to_string()
                } else if let Some(multi) =
                    tab_multi_pane_title(tab.tab_id, config.tab_title_show_foreground_process)
                {
                    multi
                } else if let Some(context_title) =
                    pane_context_title(pane, config.tab_title_show_foreground_process)
                {
                    context_title
                } else if let Some(ssh_host) = ssh_destination_for_pane(pane) {
                    ssh_title(&ssh_host)
                } else {
                    pane.title.clone()
                };
                build_default_title(tab, config, &title, true, false)
            } else {
                TitleText {
                    items: vec![FormatItem::Text(" no pane ".to_string())],
                }
            }
        }
    }
}

fn pane_context_title(pane: &PaneInformation, include_foreground_process: bool) -> Option<String> {
    let path_title = pane_cwd_title(pane);
    let process_title = if include_foreground_process {
        foreground_process_title_for_pane_info(pane)
    } else {
        None
    };
    context_process_title(path_title.as_deref(), process_title.as_deref())
}

fn pane_cwd_title(pane: &PaneInformation) -> Option<String> {
    let mux = Mux::try_get()?;
    let real_pane = mux.get_pane(pane.pane_id)?;
    let cwd = real_pane.get_current_working_dir(CachePolicy::AllowStale)?;
    path_title_from_str(cwd.path())
}

pub fn compute_tab_plain_title(tab: &TabInformation) -> String {
    if !tab.tab_title.is_empty() {
        return tab.tab_title.clone();
    }

    if let Some(pane) = &tab.active_pane {
        let config = config::configuration();
        if let Some(app_title) = app_pane_title(&config, pane) {
            return app_title.to_string();
        }
        let include_foreground_process = config.tab_title_show_foreground_process;
        return choose_plain_tab_title(
            ssh_destination_for_pane(pane),
            tab_multi_pane_title(tab.tab_id, include_foreground_process),
            compute_pane_plain_title(pane, include_foreground_process),
        );
    }

    "no pane".to_string()
}

fn choose_plain_tab_title(
    ssh_host: Option<String>,
    multi_pane_title: Option<String>,
    active_pane_title: String,
) -> String {
    ssh_host.or(multi_pane_title).unwrap_or(active_pane_title)
}

pub(crate) fn compute_pane_plain_title(
    pane: &PaneInformation,
    include_foreground_process: bool,
) -> String {
    if let Some(app_title) = app_pane_title(&config::configuration(), pane) {
        return app_title.to_string();
    }
    ssh_destination_for_pane(pane)
        .or_else(|| pane_context_title(pane, include_foreground_process))
        .unwrap_or_else(|| pane.title.clone())
}

fn build_default_title(
    tab: &TabInformation,
    config: &ConfigHandle,
    title: &str,
    with_tab_index: bool,
    with_edge_padding: bool,
) -> TitleText {
    let mut items = vec![];
    let mut len = 0;
    let mut title = title.to_string();

    let classic_spacing = if config.use_fancy_tab_bar { "" } else { " " };
    if with_tab_index && config.show_tab_index_in_tab_bar {
        let index = format!(
            "{classic_spacing}{}: ",
            tab.tab_index
                + if config.tab_and_split_indices_are_zero_based {
                    0
                } else {
                    1
                }
        );
        len += unicode_column_width(&index, None);
        items.push(FormatItem::Text(index));
        title = format!("{}{classic_spacing}", title);
    }

    if with_edge_padding {
        title = format!(" {} ", title);
    } else if !config.use_fancy_tab_bar {
        while len + unicode_column_width(&title, None) < 5 {
            title.push(' ');
        }
    }

    items.push(FormatItem::Text(title));

    let attention = matches!(tab.progress, Progress::Paused | Progress::Error(_))
        || (config.bell_tab_indicator && tab.has_unread_bell);
    if attention {
        items.push(FormatItem::Foreground(FormatColor::Color(
            "#daae76".to_string(),
        )));
        items.push(FormatItem::Text("\u{2022}".to_string()));
        items.push(FormatItem::Foreground(FormatColor::Default));
    } else {
        items.push(FormatItem::Text(" ".to_string()));
    }

    TitleText { items }
}

/// Nerd Font `md-ssh` glyph (present in the bundled SymbolsNerdFontMono),
/// prefixed to remote host titles so remote tabs read differently from a
/// local directory of the same name.
const SSH_TITLE_GLYPH: char = '\u{f08c0}';

fn ssh_title(host: &str) -> String {
    format!("{} {}", SSH_TITLE_GLYPH, host)
}

/// Detect the SSH destination for a pane, used to show the remote host in tab titles.
///
/// Fallback chain (first match wins):
///   1. `WEZTERM_PROG` user var → parse SSH command
///   2. Domain name prefix (`SSH:` / `SSHMUX:`)
///   3. Foreground process named `ssh` → parse its argv
///   4. CWD host component (e.g. from `file://host/…`)
fn ssh_destination_for_pane(pane: &PaneInformation) -> Option<String> {
    if let Some(command) = pane.user_vars.get("WEZTERM_PROG") {
        if let Some(host) = ssh_target_from_command(command) {
            return Some(host);
        }
    }

    let mux = Mux::try_get()?;
    let real_pane = mux.get_pane(pane.pane_id)?;
    ssh_destination_for_real_pane(&real_pane)
}

/// Same fallback chain as [`ssh_destination_for_pane`], for call sites that
/// hold a mux pane rather than a `PaneInformation` (e.g. per-pane segments in
/// split-tab titles).
fn ssh_destination_for_real_pane(real_pane: &std::sync::Arc<dyn Pane>) -> Option<String> {
    if let Some(command) = real_pane.copy_user_vars().get("WEZTERM_PROG") {
        if let Some(host) = ssh_target_from_command(command) {
            return Some(host);
        }
    }

    let mux = Mux::try_get()?;
    if let Some(domain) = mux.get_domain(real_pane.domain_id()) {
        let name = domain.domain_name();
        if let Some(host) = name
            .strip_prefix("SSH:")
            .or_else(|| name.strip_prefix("SSHMUX:"))
        {
            return Some(host.to_string());
        }
    }

    let fg = real_pane.get_foreground_process_name(CachePolicy::AllowStale)?;
    if !is_remote_shell_command(command_basename(&fg)) {
        return None;
    }

    if let Some(info) = real_pane.get_foreground_process_info(CachePolicy::AllowStale) {
        if let Some(host) = ssh_target_from_tokens(&info.argv) {
            return Some(host);
        }
    }

    real_pane
        .get_current_working_dir(CachePolicy::AllowStale)
        .and_then(|cwd| cwd.host_str().map(ToString::to_string))
}

/// Remote-shell launchers whose first free argument is the destination host.
/// `mosh-client` is excluded on purpose: its argv carries a resolved IP and a
/// key, not the typed host, so it is only detectable via `WEZTERM_PROG`.
fn is_remote_shell_command(basename: &str) -> bool {
    matches!(basename, "ssh" | "mosh" | "autossh" | "et")
}

fn ssh_target_from_command(command: &str) -> Option<String> {
    let tokens = shlex::split(command).unwrap_or_else(|| {
        command
            .split_whitespace()
            .map(ToString::to_string)
            .collect()
    });

    ssh_target_from_tokens(&tokens)
}

fn ssh_target_from_tokens(tokens: &[String]) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    let program = command_basename(&tokens[0]);
    if !is_remote_shell_command(program) {
        return None;
    }

    let mut expect_value = false;
    for token in tokens.iter().skip(1) {
        if expect_value {
            expect_value = false;
            continue;
        }
        if token == "--" {
            return None;
        }
        if token.starts_with('-') {
            // autossh's -M (monitor port) takes a value, unlike ssh's -M.
            expect_value = ssh_option_needs_value(token) || (program == "autossh" && token == "-M");
            continue;
        }
        return normalize_ssh_target(token);
    }
    None
}

fn ssh_option_needs_value(token: &str) -> bool {
    if token.len() != 2 || !token.starts_with('-') {
        return false;
    }
    matches!(
        token.chars().nth(1),
        Some(
            'B' | 'b'
                | 'c'
                | 'D'
                | 'E'
                | 'e'
                | 'F'
                | 'I'
                | 'i'
                | 'J'
                | 'L'
                | 'l'
                | 'm'
                | 'O'
                | 'o'
                | 'p'
                | 'Q'
                | 'R'
                | 'S'
                | 'W'
                | 'w'
        )
    )
}

fn command_basename(command: &str) -> &str {
    Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command)
}

fn normalized_process_basename(command: &str) -> String {
    let mut name = command_basename(command)
        .trim_start_matches('-')
        .to_ascii_lowercase();
    for suffix in [
        "-aarch64-apple-darwin",
        "-arm64-apple-darwin",
        "-x86_64-apple-darwin",
    ] {
        if name.ends_with(suffix) {
            name.truncate(name.len() - suffix.len());
            break;
        }
    }
    name
}

fn is_version_like_process_name(name: &str) -> bool {
    let mut numeric_parts = 0;
    for part in name.split('.') {
        let digit_count = part.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digit_count == 0 {
            return false;
        }
        numeric_parts += 1;
        if numeric_parts < 3 {
            if digit_count != part.len() {
                return false;
            }
            continue;
        }

        let suffix = &part[digit_count..];
        return suffix.is_empty()
            || suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    }
    false
}

fn process_title_from_program(command: &str) -> Option<String> {
    let lowered = command.to_ascii_lowercase();
    let command = normalized_process_basename(command);
    if command.is_empty() || command == "ssh" || is_shell_name(&command) {
        return None;
    }
    if is_version_like_process_name(&command) {
        return if lowered.contains("/claude/versions/") {
            Some("claude".to_string())
        } else {
            None
        };
    }
    Some(command)
}

fn is_shell_name(name: &str) -> bool {
    matches!(
        name,
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh"
    )
}

fn process_title_from_argv(argv: &[String]) -> Option<String> {
    let command = argv
        .first()
        .and_then(|arg| process_title_from_program(arg))?;

    if matches!(command.as_str(), "npm" | "pnpm" | "yarn" | "bun") {
        // A foreground process that rewrites its title leaves KERN_PROCARGS2
        // with environment strings where argv entries should be; never join
        // tokens that look like env vars or flags into the title.
        let task: Vec<&str> = argv
            .iter()
            .skip(1)
            .map(|arg| arg.as_str())
            .take_while(|arg| !arg.contains('=') && !arg.starts_with('-'))
            .take(2)
            .collect();
        if !task.is_empty() {
            return Some(format!("{} {}", command, task.join(" ")));
        }
    }

    Some(command)
}

fn process_title_from_user_var(command: &str) -> Option<String> {
    let tokens = shlex::split(command).unwrap_or_else(|| {
        command
            .split_whitespace()
            .map(ToString::to_string)
            .collect()
    });
    process_title_from_argv(&tokens)
}

/// Title for a stateful foreground process ("claude", "vim", "npm run dev").
/// Returns None for idle shells and ssh so path/remote titles keep priority.
///
/// Shell integration keeps WEZTERM_PROG current: the typed command line while
/// one runs, an empty string at an idle prompt. Prefer it over process
/// inspection; it costs no syscalls and names wrapper scripts ("npm run dev")
/// rather than their interpreter ("node").
fn foreground_process_title(pane: &dyn mux::pane::Pane) -> Option<String> {
    if let Some(prog) = pane.copy_user_vars().get("WEZTERM_PROG") {
        return if prog.is_empty() {
            None
        } else {
            process_title_from_user_var(prog)
        };
    }

    if let Some(info) = pane.get_foreground_process_info(CachePolicy::AllowStale) {
        if let Some(title) = process_title_from_argv(&info.argv) {
            return Some(title);
        }
    }

    let proc_name = pane.get_foreground_process_name(CachePolicy::AllowStale)?;
    process_title_from_program(&proc_name)
}

fn foreground_process_title_for_pane_info(pane: &PaneInformation) -> Option<String> {
    if let Some(prog) = pane.user_vars.get("WEZTERM_PROG") {
        return if prog.is_empty() {
            None
        } else {
            process_title_from_user_var(prog)
        };
    }
    let mux = Mux::try_get()?;
    let real_pane = mux.get_pane(pane.pane_id)?;
    foreground_process_title(&*real_pane)
}

fn normalize_ssh_target(target: &str) -> Option<String> {
    let mut host = target.trim();
    if host.is_empty() {
        return None;
    }

    if let Some(rest) = host.rsplit_once('@').map(|(_, rhs)| rhs) {
        host = rest;
    }

    if let Some(without_open) = host.strip_prefix('[') {
        if let Some(end) = without_open.find(']') {
            return Some(without_open[..end].to_string());
        }
    }

    if host.matches(':').count() == 1 {
        if let Some((h, port)) = host.rsplit_once(':') {
            if !h.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                host = h;
            }
        }
    }

    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn is_tab_hover(mouse_x: Option<usize>, x: usize, tab_title_len: usize) -> bool {
    mouse_x
        .map(|mouse_x| mouse_x >= x && mouse_x < x + tab_title_len)
        .unwrap_or(false)
}

/// Maximum width, in cell columns, that each tab title may occupy on the
/// non-fancy tab bar. Titles wider than this are truncated, so this is the
/// budget that governs the "tab gets cut off / squeezed to nothing" class of
/// layout bugs. Returns `usize::MAX` when no per-tab cap applies (the fancy
/// bar lays itself out and is never truncated here).
fn tab_width_budget(
    title_width: usize,
    controls_width: usize,
    number_of_tabs: usize,
    tab_max_width: usize,
    use_fancy_tab_bar: bool,
) -> usize {
    let tab_max_width = tab_max_width.max(1);
    if number_of_tabs == 0 {
        tab_max_width
    } else if use_fancy_tab_bar {
        usize::MAX
    } else {
        let available_cells = title_width.saturating_sub(controls_width);
        // Floor at 1 so a tab is never squeezed to zero width (it would vanish
        // and become unclickable); cap at the configured per-tab maximum.
        let per_tab = (available_cells / number_of_tabs).max(1);
        per_tab.min(tab_max_width)
    }
}

impl TabBarState {
    pub fn default() -> Self {
        Self {
            line: Line::with_width(1, SEQ_ZERO),
            items: vec![TabEntry {
                item: TabBarItem::None,
                title: Line::from_text(" ", &CellAttributes::blank(), 1, None),
                progress: Progress::None,
                x: 1,
                width: 1,
            }],
        }
    }

    pub fn line(&self) -> &Line {
        &self.line
    }

    pub fn items(&self) -> &[TabEntry] {
        &self.items
    }

    fn integrated_title_buttons(
        mouse_x: Option<usize>,
        x: &mut usize,
        config: &ConfigHandle,
        items: &mut Vec<TabEntry>,
        line: &mut Line,
        colors: &TabBarColors,
    ) {
        let default_cell = if config.use_fancy_tab_bar {
            CellAttributes::default()
        } else {
            colors.new_tab().as_cell_attributes()
        };

        let default_cell_hover = if config.use_fancy_tab_bar {
            CellAttributes::default()
        } else {
            colors.new_tab_hover().as_cell_attributes()
        };

        let window_hide =
            parse_status_text(&config.tab_bar_style.window_hide, default_cell.clone());
        let window_hide_hover = parse_status_text(
            &config.tab_bar_style.window_hide_hover,
            default_cell_hover.clone(),
        );

        let window_maximize =
            parse_status_text(&config.tab_bar_style.window_maximize, default_cell.clone());
        let window_maximize_hover = parse_status_text(
            &config.tab_bar_style.window_maximize_hover,
            default_cell_hover.clone(),
        );

        let window_close =
            parse_status_text(&config.tab_bar_style.window_close, default_cell.clone());
        let window_close_hover = parse_status_text(
            &config.tab_bar_style.window_close_hover,
            default_cell_hover.clone(),
        );

        for button in &config.integrated_title_buttons {
            use IntegratedTitleButton as Button;
            let title = match button {
                Button::Hide => {
                    let hover = is_tab_hover(mouse_x, *x, window_hide_hover.len());

                    if hover {
                        &window_hide_hover
                    } else {
                        &window_hide
                    }
                }
                Button::Maximize => {
                    let hover = is_tab_hover(mouse_x, *x, window_maximize_hover.len());

                    if hover {
                        &window_maximize_hover
                    } else {
                        &window_maximize
                    }
                }
                Button::Close => {
                    let hover = is_tab_hover(mouse_x, *x, window_close_hover.len());

                    if hover {
                        &window_close_hover
                    } else {
                        &window_close
                    }
                }
            };

            line.append_line(title.to_owned(), SEQ_ZERO);

            let width = title.len();
            items.push(TabEntry {
                item: TabBarItem::WindowButton(*button),
                title: title.to_owned(),
                progress: Progress::None,
                x: *x,
                width,
            });

            *x += width;
        }
    }

    /// Build a new tab bar from the current state
    /// mouse_x is some if the mouse is on the same row as the tab bar.
    /// title_width is the total number of cell columns in the window.
    /// window allows access to the tabs associated with the window.
    pub fn new(
        title_width: usize,
        mouse_x: Option<usize>,
        tab_info: &[TabInformation],
        pane_info: &[PaneInformation],
        is_fullscreen: bool,
        colors: Option<&TabBarColors>,
        config: &ConfigHandle,
        left_status: &str,
        right_status: &str,
    ) -> Self {
        let colors = colors.cloned().unwrap_or_else(TabBarColors::default);

        let active_cell_attrs = colors.active_tab().as_cell_attributes();
        let inactive_hover_attrs = colors.inactive_tab_hover().as_cell_attributes();
        let inactive_cell_attrs = colors.inactive_tab().as_cell_attributes();
        let new_tab_hover_attrs = colors.new_tab_hover().as_cell_attributes();
        let new_tab_attrs = colors.new_tab().as_cell_attributes();

        let new_tab = parse_status_text(
            &config.tab_bar_style.new_tab,
            if config.use_fancy_tab_bar {
                CellAttributes::default()
            } else {
                new_tab_attrs.clone()
            },
        );
        let new_tab_hover = parse_status_text(
            &config.tab_bar_style.new_tab_hover,
            if config.use_fancy_tab_bar {
                CellAttributes::default()
            } else {
                new_tab_hover_attrs.clone()
            },
        );

        let use_integrated_title_buttons = config
            .window_decorations
            .contains(window::WindowDecorations::INTEGRATED_BUTTONS);

        // We ultimately want to produce a line looking like this:
        // ` | tab1-title x | tab2-title x |  +      . - X `
        // Where the `+` sign will spawn a new tab (or show a context
        // menu with tab creation options) and the other three chars
        // are symbols representing minimize, maximize and close.

        let mut active_tab_no = 0;
        if config.show_tabs_in_tab_bar {
            for tab in tab_info {
                if tab.is_active {
                    active_tab_no = tab.tab_index;
                }
            }
        }
        let number_of_tabs = if config.show_tabs_in_tab_bar {
            tab_info.len()
        } else {
            0
        };

        // Tab titles are rendered contiguously; only reserve width for controls
        // that are actually shown.
        let controls_width = if config.show_new_tab_button_in_tab_bar {
            new_tab.len()
        } else {
            0
        };
        let tab_width_max = tab_width_budget(
            title_width,
            controls_width,
            number_of_tabs,
            config.tab_max_width,
            config.use_fancy_tab_bar,
        );
        let tab_title_max_width_for_callback = if tab_width_max == usize::MAX {
            config.tab_max_width.max(1)
        } else {
            tab_width_max
        };

        let mut line = Line::with_width(0, SEQ_ZERO);

        let mut x = 0;
        let mut items = vec![];

        let black_cell = Cell::blank_with_attrs(
            CellAttributes::default()
                .set_background(ColorSpec::TrueColor(*colors.background()))
                .clone(),
        );

        if use_integrated_title_buttons
            && config.integrated_title_button_style == IntegratedTitleButtonStyle::MacOsNative
            && !config.use_fancy_tab_bar
            && !config.tab_bar_at_bottom
            && !is_fullscreen
        {
            for _ in 0..10_usize {
                line.insert_cell(0, black_cell.clone(), title_width, SEQ_ZERO);
                x += 1;
            }
        }

        if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Left
        {
            Self::integrated_title_buttons(mouse_x, &mut x, config, &mut items, &mut line, &colors);
        }

        let left_status_line = parse_status_text(left_status, black_cell.attrs().clone());
        if left_status_line.len() > 0 {
            items.push(TabEntry {
                item: TabBarItem::LeftStatus,
                title: left_status_line.clone(),
                progress: Progress::None,
                x,
                width: left_status_line.len(),
            });
            x += left_status_line.len();
            line.append_line(left_status_line, SEQ_ZERO);
        }

        // Pre-compute all tab titles in a single Lua scope to avoid serializing
        // Config, tabs, and panes sequences once per tab.
        let precomputed_titles = if number_of_tabs > 0 {
            call_format_tab_titles_batch(
                tab_info,
                pane_info,
                config,
                tab_title_max_width_for_callback,
            )
        } else {
            BatchTabTitles::without_callback(0)
        };

        for tab_idx in 0..number_of_tabs {
            let active = tab_idx == active_tab_no;

            let precomputed = precomputed_titles
                .titles
                .get(tab_idx)
                .and_then(|t| t.clone());

            let mut tab_title =
                compute_tab_title_from_precomputed(&tab_info[tab_idx], config, precomputed.clone());
            #[cfg(feature = "remote")]
            kaku_remote::publish_tab_title(
                tab_info[tab_idx].tab_id.into(),
                &title_plain_text(&tab_title),
            );
            let mut cell_attrs = if active {
                &active_cell_attrs
            } else {
                &inactive_cell_attrs
            };

            let tab_start_idx = x;

            let mut esc =
                format_as_escapes(tab_title.items.clone()).expect("already parsed ok above");
            let mut tab_line = parse_status_text(
                &esc,
                if config.use_fancy_tab_bar {
                    CellAttributes::default()
                } else {
                    cell_attrs.clone()
                },
            );
            if tab_line.len() > tab_width_max {
                tab_line.resize(tab_width_max, SEQ_ZERO);
            }
            let mut width = tab_line.len();
            let hover = is_tab_hover(mouse_x, x, width);
            if hover {
                // The normal callback may return nil to opt into the default
                // title while still customizing the hover state.
                // Use the same SSH fallback policy as the normal title.
                let is_ssh_tab =
                    ssh_destination_for_default_title(&tab_info[tab_idx], config).is_some();
                let hover_precomputed = if precomputed_titles.callback_present && !is_ssh_tab {
                    call_format_tab_title_hover(
                        &tab_info[tab_idx],
                        tab_info,
                        pane_info,
                        config,
                        tab_title_max_width_for_callback,
                    )
                } else {
                    None
                };
                tab_title = compute_tab_title_from_precomputed(
                    &tab_info[tab_idx],
                    config,
                    hover_precomputed,
                );
                cell_attrs = if active {
                    &active_cell_attrs
                } else {
                    &inactive_hover_attrs
                };
                esc = format_as_escapes(tab_title.items.clone()).expect("already parsed ok above");
                tab_line = parse_status_text(
                    &esc,
                    if config.use_fancy_tab_bar {
                        CellAttributes::default()
                    } else {
                        cell_attrs.clone()
                    },
                );
                if tab_line.len() > tab_width_max {
                    tab_line.resize(tab_width_max, SEQ_ZERO);
                }
                width = tab_line.len();
            }
            let title = tab_line.clone();

            items.push(TabEntry {
                item: TabBarItem::Tab { tab_idx, active },
                title,
                progress: tab_info[tab_idx].progress.clone(),
                x: tab_start_idx,
                width,
            });

            line.append_line(tab_line, SEQ_ZERO);
            x += width;
        }

        // New tab button
        if config.show_new_tab_button_in_tab_bar {
            let hover = is_tab_hover(mouse_x, x, new_tab_hover.len());

            let new_tab_button = if hover { &new_tab_hover } else { &new_tab };

            let button_start = x;
            let width = new_tab_button.len();

            line.append_line(new_tab_button.clone(), SEQ_ZERO);

            items.push(TabEntry {
                item: TabBarItem::NewTabButton,
                title: new_tab_button.clone(),
                progress: Progress::None,
                x: button_start,
                width,
            });

            x += width;
        }

        // Reserve place for integrated title buttons
        let title_width = if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Right
        {
            let window_hide =
                parse_status_text(&config.tab_bar_style.window_hide, CellAttributes::default());
            let window_hide_hover = parse_status_text(
                &config.tab_bar_style.window_hide_hover,
                CellAttributes::default(),
            );

            let window_maximize = parse_status_text(
                &config.tab_bar_style.window_maximize,
                CellAttributes::default(),
            );
            let window_maximize_hover = parse_status_text(
                &config.tab_bar_style.window_maximize_hover,
                CellAttributes::default(),
            );
            let window_close = parse_status_text(
                &config.tab_bar_style.window_close,
                CellAttributes::default(),
            );
            let window_close_hover = parse_status_text(
                &config.tab_bar_style.window_close_hover,
                CellAttributes::default(),
            );

            let hide_len = window_hide.len().max(window_hide_hover.len());
            let maximize_len = window_maximize.len().max(window_maximize_hover.len());
            let close_len = window_close.len().max(window_close_hover.len());

            let mut width_to_reserve = 0;
            for button in &config.integrated_title_buttons {
                use IntegratedTitleButton as Button;
                let button_len = match button {
                    Button::Hide => hide_len,
                    Button::Maximize => maximize_len,
                    Button::Close => close_len,
                };
                width_to_reserve += button_len;
            }

            title_width.saturating_sub(width_to_reserve)
        } else {
            title_width
        };

        let status_space_available = title_width.saturating_sub(x);

        let mut right_status_line = parse_status_text(right_status, black_cell.attrs().clone());
        items.push(TabEntry {
            item: TabBarItem::RightStatus,
            title: right_status_line.clone(),
            progress: Progress::None,
            x,
            width: status_space_available,
        });

        if right_status_line.len() > status_space_available {
            let excess = right_status_line.len() - status_space_available;
            right_status_line = right_status_line.split_off(excess, SEQ_ZERO);
        }

        line.append_line(right_status_line, SEQ_ZERO);
        while line.len() < title_width {
            line.insert_cell(x, black_cell.clone(), title_width, SEQ_ZERO);
        }

        if use_integrated_title_buttons
            && config.integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative
            && config.integrated_title_button_alignment == IntegratedTitleButtonAlignment::Right
        {
            x = title_width;
            Self::integrated_title_buttons(mouse_x, &mut x, config, &mut items, &mut line, &colors);
        }

        Self { line, items }
    }

    pub fn compute_ui_items(&self, y: usize, cell_height: usize, cell_width: usize) -> Vec<UIItem> {
        let mut items = vec![];

        for entry in self.items.iter() {
            items.push(UIItem {
                x: entry.x * cell_width,
                width: entry.width * cell_width,
                y,
                height: cell_height,
                item_type: UIItemType::TabBar(entry.item),
            });
        }

        items
    }
}

pub fn parse_status_text(text: &str, default_cell: CellAttributes) -> Line {
    let mut pen = default_cell.clone();
    let mut cells = vec![];
    let mut ignoring = false;
    let mut print_buffer = String::new();

    fn flush_print(buf: &mut String, cells: &mut Vec<Cell>, pen: &CellAttributes) {
        for g in Graphemes::new(buf.as_str()) {
            let cell = Cell::new_grapheme(g, pen.clone(), None);
            let width = cell.width();
            cells.push(cell);
            for _ in 1..width {
                // Line/Screen expect double wide graphemes to be followed by a blank in
                // the next column position, otherwise we'll render incorrectly
                cells.push(Cell::blank_with_attrs(pen.clone()));
            }
        }
        buf.clear();
    }

    let mut parser = Parser::new();
    parser.parse(text.as_bytes(), |action| {
        if ignoring {
            return;
        }
        match action {
            Action::Print(c) => print_buffer.push(c),
            Action::PrintString(s) => print_buffer.push_str(&s),
            Action::Control(c) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
                match c {
                    ControlCode::CarriageReturn | ControlCode::LineFeed => {
                        ignoring = true;
                    }
                    _ => {}
                }
            }
            Action::CSI(csi) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
                match csi {
                    CSI::Sgr(sgr) => match sgr {
                        Sgr::Reset => pen = default_cell.clone(),
                        Sgr::Intensity(i) => {
                            pen.set_intensity(i);
                        }
                        Sgr::Underline(u) => {
                            pen.set_underline(u);
                        }
                        Sgr::Overline(o) => {
                            pen.set_overline(o);
                        }
                        Sgr::VerticalAlign(o) => {
                            pen.set_vertical_align(o);
                        }
                        Sgr::Blink(b) => {
                            pen.set_blink(b);
                        }
                        Sgr::Italic(i) => {
                            pen.set_italic(i);
                        }
                        Sgr::Inverse(inverse) => {
                            pen.set_reverse(inverse);
                        }
                        Sgr::Invisible(invis) => {
                            pen.set_invisible(invis);
                        }
                        Sgr::StrikeThrough(strike) => {
                            pen.set_strikethrough(strike);
                        }
                        Sgr::Foreground(col) => {
                            if let ColorSpec::Default = col {
                                pen.set_foreground(default_cell.foreground());
                            } else {
                                pen.set_foreground(col);
                            }
                        }
                        Sgr::Background(col) => {
                            if let ColorSpec::Default = col {
                                pen.set_background(default_cell.background());
                            } else {
                                pen.set_background(col);
                            }
                        }
                        Sgr::UnderlineColor(col) => {
                            pen.set_underline_color(col);
                        }
                        Sgr::Font(_) => {}
                    },
                    _ => {}
                }
            }
            Action::OperatingSystemCommand(_)
            | Action::DeviceControl(_)
            | Action::Esc(_)
            | Action::KittyImage(_)
            | Action::XtGetTcap(_)
            | Action::Sixel(_) => {
                flush_print(&mut print_buffer, &mut cells, &pen);
            }
        }
    });
    flush_print(&mut print_buffer, &mut cells, &pen);
    Line::from_cells(cells, SEQ_ZERO)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ssh_pane_titles_keep_the_formatter_on_hover() -> anyhow::Result<()> {
        config::designate_this_as_the_main_thread();
        let config_file = tempfile::NamedTempFile::new()?;
        let mut tab = make_tab(0, 0, false, "");
        let mut user_vars = std::collections::HashMap::new();
        user_vars.insert("WEZTERM_PROG".to_string(), "ssh test-host".to_string());
        let pane = PaneInformation {
            pane_id: 10000.into(),
            pane_index: 0,
            is_active: true,
            is_zoomed: false,
            has_unseen_output: false,
            left: 0,
            top: 0,
            width: 80,
            height: 24,
            pixel_width: 0,
            pixel_height: 0,
            title: "application-title".to_string(),
            user_vars,
            progress: Progress::None,
        };
        for enabled in [false, true] {
            std::fs::write(
                config_file.path(),
                format!(
                    r#"
local wezterm = require 'wezterm'
wezterm.on('format-tab-title', function() return ' Lua title ' end)
return {{ use_fancy_tab_bar = false, show_new_tab_button_in_tab_bar = false,
          tab_title_use_pane_title = {} }}
"#,
                    enabled
                ),
            )?;
            config::set_config_file_override(config_file.path());
            config::reload();
            let config = config::configuration_result()?;
            assert_eq!(config.tab_title_use_pane_title, enabled);
            // The second title is the terminal default: no program has set one
            // yet, so the opt-in has nothing to show and SSH stays in charge (#566).
            for (pane_title, app_title_set) in
                [("application-title", true), (DEFAULT_PANE_TITLE, false)]
            {
                let mut pane = pane.clone();
                pane.title = pane_title.to_string();
                tab.active_pane = Some(pane.clone());
                let tabs = [tab.clone()];
                let panes = [pane.clone()];
                let normal =
                    TabBarState::new(80, None, &tabs, &panes, false, None, &config, "", "");
                let normal_tab = normal
                    .items
                    .iter()
                    .find(|entry| matches!(entry.item, TabBarItem::Tab { .. }))
                    .expect("tab entry");
                let hovered = TabBarState::new(
                    80,
                    Some(normal_tab.x),
                    &tabs,
                    &panes,
                    false,
                    None,
                    &config,
                    "",
                    "",
                );
                let hovered_tab = hovered
                    .items
                    .iter()
                    .find(|entry| matches!(entry.item, TabBarItem::Tab { .. }))
                    .expect("hovered tab entry");
                if enabled && app_title_set {
                    assert_eq!(normal_tab.title.as_str(), " Lua title ");
                } else {
                    assert!(normal_tab.title.as_str().contains("test-host"));
                }
                assert_eq!(normal_tab.title.as_str(), hovered_tab.title.as_str());
                assert_eq!(normal_tab.width, hovered_tab.width);
            }
        }
        Ok(())
    }

    fn plain_text(title: &TitleText) -> String {
        title
            .items
            .iter()
            .filter_map(|item| match item {
                FormatItem::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn make_tab(tab_id: usize, tab_index: usize, is_active: bool, title: &str) -> TabInformation {
        TabInformation {
            tab_id: tab_id.into(),
            tab_index,
            is_active,
            is_last_active: false,
            active_pane: None,
            progress: Progress::None,
            has_unread_bell: false,
            window_id: 0,
            tab_title: title.to_string(),
        }
    }

    #[test]
    fn default_tab_title_uses_one_trailing_status_cell() {
        let config = ConfigHandle::default_config();
        let mut tab = make_tab(0, 0, false, "claude");

        let idle = build_default_title(&tab, &config, "claude", false, false);
        assert!(plain_text(&idle).ends_with(' '));

        // A running pane deliberately shows nothing: only attention gets a dot.
        tab.progress = Progress::Indeterminate;
        let running = build_default_title(&tab, &config, "claude", false, false);
        assert!(plain_text(&running).ends_with(' '));

        tab.progress = Progress::Paused;
        let attention = build_default_title(&tab, &config, "claude", false, false);
        assert!(plain_text(&attention).ends_with('\u{2022}'));
    }

    #[test]
    fn unread_bell_uses_the_same_trailing_status_cell() {
        let config = ConfigHandle::default_config();
        let mut tab = make_tab(0, 0, false, "shell");
        tab.has_unread_bell = true;

        let title = build_default_title(&tab, &config, "shell", false, false);
        assert_eq!(plain_text(&title).matches('\u{2022}').count(), 1);
        assert!(plain_text(&title).ends_with('\u{2022}'));
    }

    #[test]
    fn tab_width_budget_caps_single_tab_to_max() {
        // One tab in a wide window is capped at tab_max_width, not the window.
        assert_eq!(tab_width_budget(200, 0, 1, 25, false), 25);
    }

    #[test]
    fn tab_width_budget_divides_narrow_window_evenly() {
        // 6 tabs sharing 60 cells -> 10 each, under the 25 cap.
        assert_eq!(tab_width_budget(60, 0, 6, 25, false), 10);
    }

    #[test]
    fn tab_width_budget_reserves_controls() {
        // The new-tab button width is taken off the top before dividing.
        assert_eq!(tab_width_budget(64, 4, 6, 25, false), 10);
    }

    #[test]
    fn tab_width_budget_never_zero_when_crowded() {
        // Many tabs in a tiny window must still leave each tab at least 1 cell,
        // otherwise a tab renders to nothing and becomes unclickable (#439/#445).
        assert_eq!(tab_width_budget(10, 0, 40, 25, false), 1);
        // Controls wider than the window saturate to 0 available, still >= 1.
        assert_eq!(tab_width_budget(3, 8, 5, 25, false), 1);
    }

    #[test]
    fn tab_width_budget_zero_tabs_uses_max() {
        assert_eq!(tab_width_budget(80, 0, 0, 25, false), 25);
        // tab_max_width is clamped to at least 1.
        assert_eq!(tab_width_budget(80, 0, 0, 0, false), 1);
    }

    #[test]
    fn tab_width_budget_fancy_bar_is_uncapped() {
        assert_eq!(tab_width_budget(80, 0, 6, 25, true), usize::MAX);
    }

    #[test]
    fn hover_hits_only_within_tab_span() {
        // Tab occupies columns [10, 15): inclusive start, exclusive end.
        assert!(!is_tab_hover(Some(9), 10, 5));
        assert!(is_tab_hover(Some(10), 10, 5));
        assert!(is_tab_hover(Some(14), 10, 5));
        assert!(!is_tab_hover(Some(15), 10, 5));
        // No mouse, or a zero-width tab, never registers a hover.
        assert!(!is_tab_hover(None, 10, 5));
        assert!(!is_tab_hover(Some(10), 10, 0));
    }

    #[test]
    fn parse_plain_ssh_target() {
        assert_eq!(
            ssh_target_from_command("ssh root@10.0.0.8").as_deref(),
            Some("10.0.0.8")
        );
    }

    #[test]
    fn parse_ssh_target_with_options() {
        assert_eq!(
            ssh_target_from_command("ssh -p 2222 -i ~/.ssh/id user@build-host").as_deref(),
            Some("build-host")
        );
    }

    #[test]
    fn ignore_non_ssh_command() {
        assert!(ssh_target_from_command("ls -la").is_none());
        assert!(ssh_target_from_command("ssh-keygen -t ed25519").is_none());
    }

    #[test]
    fn parse_other_remote_shell_targets() {
        assert_eq!(
            ssh_target_from_command("mosh alice@edge.example").as_deref(),
            Some("edge.example")
        );
        assert_eq!(
            ssh_target_from_command("mosh --ssh=ssh -p 60001 edge").as_deref(),
            Some("edge")
        );
        assert_eq!(
            ssh_target_from_command("autossh -M 20000 build@ci-box").as_deref(),
            Some("ci-box")
        );
        assert_eq!(ssh_target_from_command("et devbox:8080").as_deref(), {
            Some("devbox")
        });
    }

    #[test]
    fn process_title_ignores_shells_and_ssh() {
        assert_eq!(process_title_from_argv(&["/bin/zsh".to_string()]), None);
        assert_eq!(process_title_from_argv(&["-bash".to_string()]), None);
        assert_eq!(
            process_title_from_argv(&["ssh".to_string(), "host".to_string()]),
            None
        );
    }

    #[test]
    fn process_title_uses_command_basename() {
        assert_eq!(
            process_title_from_argv(&["/usr/local/bin/claude".to_string()]).as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn process_title_hides_macos_target_triple_suffixes() {
        assert_eq!(
            process_title_from_argv(&["/usr/local/bin/codex-aarch64-apple-darwin".to_string()])
                .as_deref(),
            Some("codex")
        );
        assert_eq!(
            process_title_from_user_var("codex-x86_64-apple-darwin --resume").as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn process_title_maps_claude_versioned_executable_to_claude() {
        assert_eq!(
            process_title_from_argv(&[
                "/Users/test/.local/share/claude/versions/2.1.201".to_string(),
                "--session-id".to_string(),
                "abc".to_string(),
            ])
            .as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn process_title_ignores_bare_version_like_executable_names() {
        assert_eq!(
            process_title_from_argv(&["2.1.201".to_string(), "--session-id".to_string()]),
            None
        );
        assert_eq!(
            process_title_from_argv(&["/tmp/versions/2.1.201".to_string()]),
            None
        );
    }

    #[test]
    fn context_process_title_keeps_cwd_primary_without_spaces() {
        assert_eq!(
            context_process_title(Some("kaku"), Some("codex")).as_deref(),
            Some("kaku\u{00b7}codex")
        );
        assert_eq!(
            context_process_title(Some("kaku"), None).as_deref(),
            Some("kaku")
        );
        assert_eq!(
            context_process_title(None, Some("codex")).as_deref(),
            Some("codex")
        );
        assert_eq!(
            context_process_title(Some("codex"), Some("codex")).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn multi_pane_separator_is_distinct_from_context_process_separator() {
        assert_eq!(
            ["www/kaku\u{00b7}codex", "www/kaku"].join(MULTI_PANE_TITLE_SEPARATOR),
            "www/kaku\u{00b7}codex\u{2219}www/kaku"
        );
        assert_ne!(CONTEXT_PROCESS_SEPARATOR, MULTI_PANE_TITLE_SEPARATOR.trim());
    }

    #[test]
    fn plain_tab_title_keeps_multi_pane_context_for_rename() {
        assert_eq!(
            choose_plain_tab_title(
                None,
                Some("src/main.rs∙src/app.rs".to_string()),
                "src/main.rs".to_string(),
            ),
            "src/main.rs∙src/app.rs"
        );
        assert_eq!(
            choose_plain_tab_title(
                Some("server.example".to_string()),
                Some("src/main.rs∙src/app.rs".to_string()),
                "src/main.rs".to_string(),
            ),
            "server.example"
        );
    }

    #[test]
    fn process_title_keeps_package_manager_task() {
        assert_eq!(
            process_title_from_argv(&[
                "/opt/homebrew/bin/npm".to_string(),
                "run".to_string(),
                "dev".to_string(),
                "--".to_string(),
            ])
            .as_deref(),
            Some("npm run dev")
        );
    }

    #[test]
    fn process_title_never_joins_env_tokens() {
        // A process that rewrites its title (node does) squashes argv into one
        // string and KERN_PROCARGS2 parsing then leaks environment strings
        // into the remaining argv slots.
        assert_eq!(
            process_title_from_argv(&[
                "yarn".to_string(),
                "COLORFGBG=15;0".to_string(),
                "COLORTERM=truecolor".to_string(),
            ])
            .as_deref(),
            Some("yarn")
        );
        assert_eq!(
            process_title_from_argv(&["npm run dev".to_string(), "COLORFGBG=15;0".to_string()])
                .as_deref(),
            Some("npm run dev")
        );
    }

    #[test]
    fn process_title_from_user_var_command_line() {
        assert_eq!(
            process_title_from_user_var("npm run dev -- --port 3000").as_deref(),
            Some("npm run dev")
        );
        assert_eq!(
            process_title_from_user_var("claude --resume").as_deref(),
            Some("claude")
        );
        assert_eq!(
            process_title_from_user_var("vim notes.md").as_deref(),
            Some("vim")
        );
        assert_eq!(process_title_from_user_var("ssh example.com"), None);
        assert_eq!(process_title_from_user_var("zsh"), None);
        assert_eq!(
            process_title_from_user_var("pnpm install typescript").as_deref(),
            Some("pnpm install typescript")
        );
    }

    #[test]
    fn hover_title_callback_runs_even_when_normal_returns_nil() -> anyhow::Result<()> {
        let lua = mlua::Lua::new();
        let callback = lua.create_function(
            |lua,
             (_tab, _tabs, _panes, _config, hover, _max_width): (
                mlua::Value,
                mlua::Value,
                mlua::Value,
                mlua::Value,
                bool,
                usize,
            )| {
                if hover {
                    Ok(mlua::Value::String(lua.create_string("HOVER")?))
                } else {
                    Ok(mlua::Value::Nil)
                }
            },
        )?;
        config::lua::register_event(&lua, ("format-tab-title".to_string(), callback))?;

        let config = ConfigHandle::default_config();
        let tab_info = vec![make_tab(0, 0, true, "tab-0")];
        let pane_info = vec![];

        let batch =
            call_format_tab_titles_batch_with_lua(&lua, &tab_info, &pane_info, &config, 32)?;
        assert!(batch.callback_present);
        assert_eq!(batch.titles.len(), 1);
        assert!(batch.titles[0].is_none());

        let hover = call_format_tab_title_hover_with_lua(
            &lua,
            &tab_info[0],
            &tab_info,
            &pane_info,
            &config,
            32,
        )?
        .expect("hover callback should produce a title");

        assert_eq!(plain_text(&hover), "HOVER");
        Ok(())
    }
}
