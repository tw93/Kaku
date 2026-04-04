mod ui;

use crate::assistant_config;
use crate::utils::open_path_in_editor;
use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::{Path, PathBuf};

const KAKU_AUTO_COLOR_SCHEME_EXPR: &str =
    "(wezterm.gui and wezterm.gui.get_appearance() or 'Dark'):find('Dark') and 'Kaku Dark' or 'Kaku Light'";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NormalModeAction {
    ExitAndSave,
    ExitDiscard,
    OpenEditor,
    MoveUp,
    MoveDown,
    StartEdit,
    Noop,
}

pub fn run(config_path: PathBuf) -> anyhow::Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    stdout
        .execute(EnterAlternateScreen)
        .context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let mut app = App::new(config_path);
    app.load_config();

    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode().context("disable raw mode")?;
    terminal
        .backend_mut()
        .execute(LeaveAlternateScreen)
        .context("leave alternate screen")?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        if let Err(e) = terminal.draw(|f| ui::ui(f, app)) {
            return Err(e.into());
        }

        let event = match event::read() {
            Ok(e) => e,
            Err(e) => return Err(e.into()),
        };

        let Event::Key(key) = event else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            app.finalize_active_input();
            if let Err(e) = app.save_if_dirty() {
                return Err(e);
            }
            return Ok(());
        }

        match app.mode {
            Mode::Normal => match normal_mode_action(key.code) {
                NormalModeAction::ExitAndSave => {
                    if let Err(e) = app.save_if_dirty() {
                        return Err(e);
                    }
                    return Ok(());
                }
                NormalModeAction::ExitDiscard => {
                    return Ok(());
                }
                NormalModeAction::OpenEditor => {
                    if let Err(e) = app.save_if_dirty() {
                        return Err(e);
                    }
                    let config_path = app.config_path();
                    if let Err(e) =
                        with_terminal_suspended(terminal, || open_config_in_editor(&config_path))
                    {
                        return Err(e);
                    }
                    return Ok(());
                }
                NormalModeAction::MoveUp => {
                    app.move_up();
                }
                NormalModeAction::MoveDown => {
                    app.move_down();
                }
                NormalModeAction::StartEdit => {
                    app.start_edit();
                }
                NormalModeAction::Noop => {}
            },
            Mode::Editing => match key.code {
                KeyCode::Esc => {
                    app.cancel_edit();
                }
                KeyCode::Enter => {
                    app.confirm_edit();
                }
                KeyCode::Backspace => {
                    app.edit_backspace();
                }
                KeyCode::Left => {
                    app.edit_cursor_left();
                }
                KeyCode::Right => {
                    app.edit_cursor_right();
                }
                KeyCode::Char(c) => {
                    // Ignore characters with Ctrl/Cmd modifiers to avoid inserting escape sequences
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::SUPER)
                    {
                        app.edit_insert(c);
                    }
                }
                _ => {}
            },
            Mode::Selecting => match key.code {
                KeyCode::Esc => {
                    // ESC in selector = confirm the highlighted option and exit,
                    // matching the "ESC saves and exits" mental model of Normal mode.
                    app.confirm_select();
                    if let Err(e) = app.save_if_dirty() {
                        return Err(e);
                    }
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.select_up();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.select_down();
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    app.confirm_select();
                }
                _ => {}
            },
        }
    }
}

fn normal_mode_action(key: KeyCode) -> NormalModeAction {
    match key {
        KeyCode::Esc => NormalModeAction::ExitAndSave,
        KeyCode::Char('q') | KeyCode::Char('Q') => NormalModeAction::ExitDiscard,
        KeyCode::Char('e') | KeyCode::Char('E') => NormalModeAction::OpenEditor,
        KeyCode::Up | KeyCode::Char('k') => NormalModeAction::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => NormalModeAction::MoveDown,
        KeyCode::Enter | KeyCode::Char(' ') => NormalModeAction::StartEdit,
        _ => NormalModeAction::Noop,
    }
}

fn with_terminal_suspended<F>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    func: F,
) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    disable_raw_mode().context("disable raw mode")?;
    let mut stdout = io::stdout();
    stdout
        .execute(LeaveAlternateScreen)
        .context("leave alternate screen")?;

    let action_result = func();

    let restore_result = (|| -> anyhow::Result<()> {
        enable_raw_mode().context("enable raw mode")?;
        let mut stdout = io::stdout();
        stdout
            .execute(EnterAlternateScreen)
            .context("enter alternate screen")?;
        terminal.clear().context("clear terminal")?;
        Ok(())
    })();

    action_result.and(restore_result)
}

pub(crate) fn ensure_editable_config_exists(config_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = config_path {
        return config::ensure_config_exists_at_path(path);
    }

    config::ensure_user_config_exists()
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Editing,
    Selecting,
}

#[derive(Clone)]
struct ConfigField {
    section: &'static str,
    key: &'static str,
    lua_key: &'static str,
    value: String,
    default: String,
    options: Vec<&'static str>,
    /// If true, the field's config line exists but could not be fully parsed.
    /// save_config will leave the line untouched to avoid corrupting user config.
    skip_write: bool,
}

impl ConfigField {
    fn has_options(&self) -> bool {
        !self.options.is_empty()
    }
}

struct App {
    config_path: PathBuf,
    fields: Vec<ConfigField>,
    selected: usize,
    mode: Mode,
    edit_buffer: String,
    edit_cursor: usize,
    /// Original value before editing, used to revert on invalid input.
    edit_original: String,
    select_index: usize,
    dirty: bool,
    /// True if save_config() was called at least once (for signaling on exit)
    has_saved: bool,
    /// Preserve whether the current window_decorations state keeps resize edges.
    window_decorations_resize: bool,
}

impl App {
    fn new(config_path: PathBuf) -> Self {
        let fields = vec![
            // Appearance
            ConfigField {
                section: "Appearance",
                key: "Theme",
                lua_key: "color_scheme",
                value: String::new(),
                default: "Auto".into(),
                options: vec!["Auto", "Kaku Dark", "Kaku Light"],
                skip_write: false,
            },
            ConfigField {
                section: "Appearance",
                key: "Font",
                lua_key: "font",
                value: String::new(),
                default: "JetBrains Mono".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                section: "Appearance",
                key: "Font Size",
                lua_key: "font_size",
                value: String::new(),
                default: "17".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                section: "Appearance",
                key: "Line Height",
                lua_key: "line_height",
                value: String::new(),
                default: "1.28".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                section: "Integrations",
                key: "Global Hotkey",
                lua_key: "macos_global_hotkey",
                value: String::new(),
                default: "Ctrl+Alt+Cmd+K".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                section: "Integrations",
                key: "Kaku Assistant",
                lua_key: "__assistant_enabled__",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Tab Bar Position",
                lua_key: "tab_bar_at_bottom",
                value: String::new(),
                default: "Bottom".into(),
                options: vec!["Bottom", "Top"],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Short Tab Titles",
                lua_key: "tab_title_show_basename_only",
                value: String::new(),
                default: "Off".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Scrollbar",
                lua_key: "enable_scroll_bar",
                value: String::new(),
                default: "Off".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Traffic Lights",
                lua_key: "__wdeco_traffic_lights__",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Shadow",
                lua_key: "__wdeco_shadow__",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Background Opacity",
                lua_key: "window_background_opacity",
                value: String::new(),
                default: "1.0".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                section: "Window",
                key: "Background Blur",
                lua_key: "macos_window_background_blur",
                value: String::new(),
                default: "0".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                section: "Behavior",
                key: "Copy on Select",
                lua_key: "copy_on_select",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Behavior",
                key: "Confirm Tab Close",
                lua_key: "tab_close_confirmation",
                value: String::new(),
                default: "Off".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Behavior",
                key: "Confirm Pane Close",
                lua_key: "pane_close_confirmation",
                value: String::new(),
                default: "Off".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Behavior",
                key: "Bell Tab Prefix",
                lua_key: "bell_tab_indicator",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Behavior",
                key: "Bell Dock Badge",
                lua_key: "bell_dock_badge",
                value: String::new(),
                default: "Off".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                section: "Behavior",
                key: "Remember Last Directory",
                lua_key: "remember_last_cwd",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
        ];

        Self {
            config_path,
            fields,
            selected: 0,
            mode: Mode::Normal,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_original: String::new(),
            select_index: 0,
            dirty: false,
            has_saved: false,
            window_decorations_resize: true,
        }
    }

    fn load_config(&mut self) {
        if let Some(field) = self
            .fields
            .iter_mut()
            .find(|field| field.lua_key == "__assistant_enabled__")
        {
            field.value = match assistant_config::read_enabled() {
                Ok(true) => "On".into(),
                Ok(false) => "Off".into(),
                Err(_) => field.default.clone(),
            };
        }

        let config_path = self.config_path();
        if !config_path.exists() {
            return;
        }

        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        for i in 0..self.fields.len() {
            let lua_key = self.fields[i].lua_key;
            match Self::extract_lua_value(&content, lua_key) {
                Some(val) => match Self::normalize_value(lua_key, &val) {
                    Some(normalized) => self.fields[i].value = normalized,
                    // Recognized key, but value format is unsupported.
                    // Mark skip_write so save never corrupts this line.
                    None => self.fields[i].skip_write = true,
                },
                None => {
                    // extract_lua_value returns None when the wezterm.* guard fires
                    // (line exists but value is an unsupported API call).
                    // Only set skip_write when a config line actually exists for this key.
                    if Self::has_config_line(&content, lua_key) {
                        self.fields[i].skip_write = true;
                    }
                }
            }
        }

        // Load window_decorations into the Traffic Lights / Shadow pseudo-fields.
        self.load_window_decorations(&content);
    }

    /// Returns true if a non-commented `config.<key>` assignment exists in content.
    fn has_config_line(content: &str, key: &str) -> bool {
        let pattern = format!("config.{}", key);
        content.lines().any(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("--") {
                return false;
            }
            if !trimmed.starts_with(&pattern) {
                return false;
            }
            let after = &trimmed[pattern.len()..];
            after.starts_with(|c: char| c.is_whitespace() || c == '=')
        })
    }

    /// Parse the raw `window_decorations` value from the Lua file and populate
    /// the two pseudo-fields (`__wdeco_traffic_lights__` and `__wdeco_shadow__`).
    fn load_window_decorations(&mut self, content: &str) {
        let raw = match Self::extract_lua_value(content, "window_decorations") {
            Some(val) => val,
            None => {
                // Line exists but value is unparseable (e.g. wezterm.* API call).
                if Self::has_config_line(content, "window_decorations") {
                    for f in self.fields.iter_mut() {
                        if f.lua_key == "__wdeco_traffic_lights__"
                            || f.lua_key == "__wdeco_shadow__"
                        {
                            f.skip_write = true;
                        }
                    }
                }
                return;
            }
        };

        let is_supported = Self::parse_window_decorations(&raw).is_some();
        // Always extract the recognizable bits so that toggling one field
        // does not silently flip the other when the value is unsupported.
        let (traffic_lights, shadow, resize) = Self::extract_window_decoration_state(&raw);
        self.window_decorations_resize = resize;

        for f in self.fields.iter_mut() {
            if f.lua_key == "__wdeco_traffic_lights__" {
                f.value = if traffic_lights { "On" } else { "Off" }.into();
                if !is_supported {
                    f.skip_write = true;
                }
            } else if f.lua_key == "__wdeco_shadow__" {
                f.value = if shadow { "On" } else { "Off" }.into();
                if !is_supported {
                    f.skip_write = true;
                }
            }
        }
    }

    /// Decompose a raw `window_decorations` string into
    /// (traffic_lights_on, shadow_on, resize_on).
    /// Returns `None` for unsupported flag combinations.
    fn parse_window_decorations(raw: &str) -> Option<(bool, bool, bool)> {
        let value = raw.trim().trim_matches('\'').trim_matches('"');
        let flags: Vec<&str> = value.split('|').map(|s| s.trim()).collect();

        let has_ib = flags.contains(&"INTEGRATED_BUTTONS");
        let has_resize = flags.contains(&"RESIZE");
        let has_shadow_off = flags.contains(&"MACOS_FORCE_DISABLE_SHADOW");

        if !has_ib && !has_resize {
            return None;
        }

        let expected_count = has_ib as usize + has_resize as usize + has_shadow_off as usize;
        if flags.len() != expected_count {
            return None;
        }

        Some((has_ib, !has_shadow_off, has_resize))
    }

    /// Best-effort extraction of the modeled bits from any `window_decorations`
    /// string, including unsupported combinations. Used so that toggling one field
    /// does not silently flip the other when the original value had extra flags.
    fn extract_window_decoration_state(raw: &str) -> (bool, bool, bool) {
        let value = raw.trim().trim_matches('\'').trim_matches('"');
        let flags: Vec<&str> = value.split('|').map(|s| s.trim()).collect();
        let has_ib = flags.contains(&"INTEGRATED_BUTTONS");
        let has_resize = flags.contains(&"RESIZE");
        let has_shadow_off = flags.contains(&"MACOS_FORCE_DISABLE_SHADOW");
        (has_ib, !has_shadow_off, has_resize)
    }

    /// Build the Lua-ready `window_decorations` value (with quotes) from the two
    /// boolean states.
    fn compose_window_decorations(
        traffic_lights: bool,
        shadow: bool,
        resize: bool,
    ) -> &'static str {
        match (traffic_lights, shadow, resize) {
            (true, true, true) => "'INTEGRATED_BUTTONS|RESIZE'",
            (true, true, false) => "'INTEGRATED_BUTTONS'",
            (true, false, true) => "'INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW'",
            (true, false, false) => "'INTEGRATED_BUTTONS|MACOS_FORCE_DISABLE_SHADOW'",
            (false, true, true) | (false, true, false) => "'RESIZE'",
            (false, false, true) | (false, false, false) => "'RESIZE|MACOS_FORCE_DISABLE_SHADOW'",
        }
    }

    fn config_path(&self) -> PathBuf {
        self.config_path.clone()
    }

    fn extract_lua_value(content: &str, key: &str) -> Option<String> {
        let pattern = format!("config.{}", key);
        for line in content.lines() {
            let trimmed = line.trim();
            // Skip comments
            if trimmed.starts_with("--") {
                continue;
            }
            if !trimmed.starts_with(&pattern) {
                continue;
            }
            // Ensure exact key match (not prefix like font vs font_size)
            let after_pattern = &trimmed[pattern.len()..];
            if !after_pattern.starts_with(|c: char| c.is_whitespace() || c == '=') {
                continue;
            }
            let eq_pos = trimmed.find('=')?;
            let value_part = trimmed[eq_pos + 1..].trim();

            // Handle different value types
            if value_part.starts_with("wezterm.font(") {
                // Extract font name from wezterm.font('Name') or wezterm.font("Name")
                return Self::extract_quoted_arg(value_part, "wezterm.font(");
            }
            // Unknown wezterm API call (e.g. wezterm.font_with_fallback): skip to
            // avoid corrupting the value on write-back via to_lua_value.
            if value_part.starts_with("wezterm.") {
                return None;
            }
            if value_part.starts_with('{') {
                // Table value - return as-is up to end or comment
                return Some(Self::strip_trailing_comment(value_part));
            }
            if value_part.starts_with('\'') || value_part.starts_with('"') {
                // Quoted string
                let quote = value_part.chars().next().unwrap();
                if let Some(end) = value_part[1..].find(quote) {
                    return Some(value_part[1..1 + end].to_string());
                }
            }
            let value = Self::strip_trailing_comment(value_part);
            if key == "color_scheme" && Self::is_kaku_auto_color_scheme_expr(&value) {
                return Some("Auto".to_string());
            }
            // Number, boolean, or identifier
            if Self::is_scalar_literal(&value) {
                return Some(value);
            }
            return None;
        }
        None
    }

    fn is_kaku_auto_color_scheme_expr(raw: &str) -> bool {
        raw.trim() == KAKU_AUTO_COLOR_SCHEME_EXPR
    }

    fn extract_quoted_arg(s: &str, prefix: &str) -> Option<String> {
        let rest = s.strip_prefix(prefix)?;
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let inner = &rest[1..];
        let end = inner.find(quote)?;
        Some(inner[..end].to_string())
    }

    fn strip_trailing_comment(s: &str) -> String {
        // Remove Lua line comment (--) but be careful with strings
        let mut in_string = false;
        let mut quote_char = ' ';
        let mut result_end = s.len();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == quote_char && (i == 0 || chars[i - 1] != '\\') {
                    in_string = false;
                }
            } else if c == '\'' || c == '"' {
                in_string = true;
                quote_char = c;
            } else if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
                result_end = i;
                break;
            }
            i += 1;
        }
        s[..result_end].trim().to_string()
    }

    fn extract_table_quoted_value(raw: &str, key: &str) -> Option<String> {
        let needle = format!("{key} = ");
        let start = raw.find(&needle)? + needle.len();
        let rest = raw[start..].trim_start();
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let inner = &rest[1..];
        let end = inner.find(quote)?;
        Some(inner[..end].to_string())
    }

    fn normalize_hotkey_table(raw: &str) -> Option<String> {
        let key = Self::extract_table_quoted_value(raw, "key")?;
        let mods = Self::extract_table_quoted_value(raw, "mods").unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        for token in mods.split('|') {
            match token.trim().to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => parts.push("Ctrl".to_string()),
                "ALT" | "OPT" | "OPTION" => parts.push("Alt".to_string()),
                "SUPER" | "CMD" | "COMMAND" => parts.push("Cmd".to_string()),
                "SHIFT" => parts.push("Shift".to_string()),
                _ => {}
            }
        }
        parts.push(key.to_ascii_uppercase());
        Some(parts.join("+"))
    }

    fn hotkey_to_lua(value: &str) -> Option<String> {
        let parts: Vec<&str> = value
            .split('+')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return None;
        }

        let key = parts.last()?.to_ascii_uppercase();
        let mut mods: Vec<&str> = Vec::new();
        for token in &parts[..parts.len() - 1] {
            match token.to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => mods.push("CTRL"),
                "ALT" | "OPT" | "OPTION" => mods.push("ALT"),
                "CMD" | "SUPER" | "COMMAND" => mods.push("SUPER"),
                "SHIFT" => mods.push("SHIFT"),
                _ => {}
            }
        }
        if mods.is_empty() {
            return None;
        }

        Some(format!(
            "{{ key = '{}', mods = '{}' }}",
            key,
            mods.join("|")
        ))
    }

    /// Converts a raw Lua value string into the TUI's internal display format.
    /// Returns None when the value exists but cannot be parsed into a supported
    /// format; the caller should set skip_write=true to protect the original line.
    fn normalize_value(lua_key: &str, raw: &str) -> Option<String> {
        match lua_key {
            "color_scheme" | "font" => {
                if raw.is_empty()
                    || raw.eq_ignore_ascii_case("nil")
                    || raw.eq_ignore_ascii_case("true")
                    || raw.eq_ignore_ascii_case("false")
                    || Self::is_number_literal(raw)
                {
                    None
                } else {
                    Some(raw.to_string())
                }
            }
            "font_size"
            | "line_height"
            | "window_background_opacity"
            | "macos_window_background_blur" => {
                if Self::is_number_literal(raw) {
                    Some(raw.to_string())
                } else {
                    None
                }
            }
            "copy_on_select"
            | "enable_scroll_bar"
            | "tab_close_confirmation"
            | "pane_close_confirmation"
            | "bell_tab_indicator"
            | "bell_dock_badge"
            | "remember_last_cwd"
            | "tab_title_show_basename_only" => {
                if raw == "true" {
                    Some("On".into())
                } else if raw == "false" {
                    Some("Off".into())
                } else {
                    None
                }
            }
            "hide_tab_bar_if_only_one_tab" => {
                if raw == "true" {
                    Some("Auto".into())
                } else if raw == "false" {
                    Some("Always".into())
                } else {
                    None
                }
            }
            "tab_bar_at_bottom" => {
                if raw == "true" {
                    Some("Bottom".into())
                } else if raw == "false" {
                    Some("Top".into())
                } else {
                    None
                }
            }
            "harfbuzz_features" => {
                let stripped = raw.replace([' ', '\'', '"'], "");
                if stripped == "{calt=0,clig=0,liga=0}" {
                    Some("Off".into())
                } else if stripped == "{}"
                    || stripped.is_empty()
                    || stripped.eq_ignore_ascii_case("nil")
                {
                    Some("On".into())
                } else {
                    None
                }
            }
            "macos_global_hotkey" => {
                let value = raw.trim();
                if value.eq_ignore_ascii_case("nil") {
                    Some(String::new())
                } else if value.starts_with('{') {
                    Self::normalize_hotkey_table(value)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_number_literal(raw: &str) -> bool {
        let value = raw.trim();
        !value.is_empty() && (value.parse::<i64>().is_ok() || value.parse::<f64>().is_ok())
    }

    fn is_scalar_literal(raw: &str) -> bool {
        let value = raw.trim();
        value.eq_ignore_ascii_case("true")
            || value.eq_ignore_ascii_case("false")
            || value.eq_ignore_ascii_case("nil")
            || Self::is_number_literal(value)
    }

    fn display_value<'a>(&'a self, field: &'a ConfigField) -> &'a str {
        if field.value.is_empty() {
            &field.default
        } else {
            &field.value
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.item_count() {
            self.selected += 1;
        }
    }

    fn item_count(&self) -> usize {
        self.fields.len()
    }

    /// Save config if there are pending changes. Returns Err on save failure.
    /// Also signals kaku-gui immediately after a successful write so it reloads
    /// without waiting for the file-watcher grace period.
    fn save_if_dirty(&mut self) -> anyhow::Result<()> {
        if self.dirty {
            self.save_config()?;
            self.dirty = false;
            self.has_saved = true;
            // Signal immediately while the pane's stdout is still being read by
            // kaku-gui. Sending after LeaveAlternateScreen is unreliable because
            // the terminal may have already closed the child's output stream.
            signal_config_changed();
        }
        Ok(())
    }

    fn finalize_active_input(&mut self) {
        match self.mode {
            Mode::Editing => self.confirm_edit(),
            Mode::Selecting => self.confirm_select(),
            Mode::Normal => {}
        }
    }

    fn start_edit(&mut self) {
        let field = &self.fields[self.selected];
        if field.has_options() {
            if field.options.len() == 2 {
                // Binary field: toggle directly without a popup.
                let current = self.display_value(field);
                let current_idx = field
                    .options
                    .iter()
                    .position(|&o| o == current)
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % 2;
                let next_value = field.options[next_idx].to_string();
                self.fields[self.selected].value = next_value;
                self.fields[self.selected].skip_write = false;
                self.dirty = true;
            } else {
                self.mode = Mode::Selecting;
                let current = self.display_value(field);
                self.select_index = field
                    .options
                    .iter()
                    .position(|&o| o == current)
                    .unwrap_or(0);
            }
        } else {
            self.mode = Mode::Editing;
            // Remember original value to revert on invalid input
            self.edit_original = field.value.clone();
            self.edit_buffer = if field.value.is_empty() {
                field.default.clone()
            } else {
                field.value.clone()
            };
            self.edit_cursor = self.edit_buffer.chars().count();
        }
    }

    fn cancel_edit(&mut self) {
        self.mode = Mode::Normal;
        self.edit_buffer.clear();
    }

    fn select_up(&mut self) {
        if self.select_index > 0 {
            self.select_index -= 1;
        }
    }

    fn select_down(&mut self) {
        let field = &self.fields[self.selected];
        if self.select_index < field.options.len() - 1 {
            self.select_index += 1;
        }
    }

    fn confirm_edit(&mut self) {
        let mut new_value = self.edit_buffer.clone();
        let field = &self.fields[self.selected];

        if Self::expects_numeric_input(field.lua_key)
            && !new_value.is_empty()
            && !Self::is_number_literal(&new_value)
        {
            new_value = self.edit_original.clone();
        }

        // Validate hotkey input: if invalid, revert to original value
        // so UI display matches what will be saved to file.
        if field.lua_key == "macos_global_hotkey"
            && !new_value.is_empty()
            && Self::hotkey_to_lua(&new_value).is_none()
        {
            new_value = self.edit_original.clone();
        }

        self.fields[self.selected].value = new_value;
        // User explicitly set a value: allow it to be written even if the field
        // was previously marked unwritable due to an unrecognized format.
        self.fields[self.selected].skip_write = false;
        self.mode = Mode::Normal;
        self.edit_buffer.clear();
        self.dirty = true;
    }

    fn confirm_select(&mut self) {
        let selected_option = self.fields[self.selected].options[self.select_index];
        let current_value = self.display_value(&self.fields[self.selected]).to_string();
        if current_value == selected_option {
            self.mode = Mode::Normal;
            return;
        }

        self.fields[self.selected].value = selected_option.to_string();
        // Same: explicit user choice overrides the skip_write protection.
        self.fields[self.selected].skip_write = false;
        self.mode = Mode::Normal;
        self.dirty = true;
    }

    fn edit_backspace(&mut self) {
        if self.edit_cursor > 0 {
            // Convert char index to byte index
            let byte_idx = self
                .edit_buffer
                .char_indices()
                .nth(self.edit_cursor - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.edit_buffer.remove(byte_idx);
            self.edit_cursor -= 1;
        }
    }

    fn edit_cursor_left(&mut self) {
        if self.edit_cursor > 0 {
            self.edit_cursor -= 1;
        }
    }

    fn edit_cursor_right(&mut self) {
        if self.edit_cursor < self.edit_buffer.chars().count() {
            self.edit_cursor += 1;
        }
    }

    fn edit_insert(&mut self, c: char) {
        // Convert char index to byte index for insertion
        let byte_idx = self
            .edit_buffer
            .char_indices()
            .nth(self.edit_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.edit_buffer.len());
        self.edit_buffer.insert(byte_idx, c);
        self.edit_cursor += 1;
    }

    fn save_config(&self) -> anyhow::Result<()> {
        // Ensure config file exists with proper structure first
        ensure_editable_config_exists(Some(&self.config_path))?;

        let config_path = self.config_path();
        let original_content = std::fs::read_to_string(&config_path).unwrap_or_default();
        let mut content = original_content.clone();
        let assistant_enabled = self
            .fields
            .iter()
            .find(|field| field.lua_key == "__assistant_enabled__")
            .map(|field| self.display_value(field) == "On");

        for field in &self.fields {
            if field.lua_key == "__assistant_enabled__" || field.lua_key.starts_with("__wdeco_") {
                continue;
            }

            // Never touch lines we couldn't fully parse — preserve user's original.
            if field.skip_write {
                continue;
            }
            let is_default = field.value.is_empty() || field.value == field.default;
            // Keep tab bar position explicit so switching back to Bottom
            // does not depend on removing a line and inheriting bundled defaults.
            let always_write = field.lua_key == "tab_bar_at_bottom";
            if is_default && !always_write {
                // Remove the config line if it exists
                content = self.remove_lua_config(&content, field.lua_key);
            } else {
                // Update or add the config line
                content = self.update_lua_config(&content, field);
            }
        }

        // Compose window_decorations from the two pseudo-fields.
        let tl_field = self
            .fields
            .iter()
            .find(|f| f.lua_key == "__wdeco_traffic_lights__");
        let sh_field = self.fields.iter().find(|f| f.lua_key == "__wdeco_shadow__");
        if let (Some(tl), Some(sh)) = (tl_field, sh_field) {
            if !tl.skip_write || !sh.skip_write {
                let tl_on = self.display_value(tl) == "On";
                let sh_on = self.display_value(sh) == "On";
                let resize = if tl_on {
                    self.window_decorations_resize
                } else {
                    true
                };
                if tl_on && sh_on && resize {
                    // Default state: remove explicit override so bundled default applies.
                    content = self.remove_lua_config(&content, "window_decorations");
                } else {
                    let lua_value = Self::compose_window_decorations(tl_on, sh_on, resize);
                    content = self.set_lua_config(&content, "window_decorations", lua_value);
                }
            }
        }

        // Atomic write: write to a temp file then rename so the file watcher
        // always sees a fully-written config (never a truncated intermediate).
        //
        // Resolve symlinks so we write through to the real file rather than
        // replacing the symlink itself (which would break dotfile workflows).
        let real_path = std::fs::canonicalize(&config_path).unwrap_or(config_path);
        // Preserve the original file's permissions on the replacement.
        let original_perms = std::fs::metadata(&real_path).ok().map(|m| m.permissions());
        let temp_path = real_path.with_extension("lua.tmp");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            // Set permissions after writing to avoid failure if original was read-only.
            if let Some(perms) = original_perms {
                let _ = file.set_permissions(perms);
            }
        }
        std::fs::rename(&temp_path, &real_path)?;

        if let Some(enabled) = assistant_enabled {
            if let Err(err) = assistant_config::write_enabled(enabled) {
                if let Err(rollback_err) =
                    crate::utils::write_atomic(&real_path, original_content.as_bytes())
                {
                    return Err(err.context(format!(
                        "assistant setting save failed and Lua rollback also failed: {}",
                        rollback_err
                    )));
                }
                return Err(err);
            }
        }

        Ok(())
    }

    fn remove_lua_config(&self, content: &str, lua_key: &str) -> String {
        let pattern = format!("config.{}", lua_key);
        let lines: Vec<&str> = content.lines().collect();
        let mut result: Vec<&str> = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            // Keep comment lines
            if trimmed.starts_with("--") {
                result.push(line);
                i += 1;
                continue;
            }

            // Check if this line starts our target config
            if trimmed.starts_with(&pattern) {
                let after_pattern = &trimmed[pattern.len()..];
                if after_pattern.starts_with(|c: char| c.is_whitespace() || c == '=') {
                    // Found the config line to remove
                    // Check if value contains an unclosed brace (multi-line table)
                    if let Some(eq_pos) = trimmed.find('=') {
                        let value_part = trimmed[eq_pos + 1..].trim();
                        let mut brace_depth = Self::count_brace_depth(value_part);

                        // Skip additional lines if brace is unclosed
                        while brace_depth > 0 && i + 1 < lines.len() {
                            i += 1;
                            brace_depth += Self::count_brace_depth(lines[i]);
                        }
                    }
                    i += 1;
                    continue;
                }
            }

            result.push(line);
            i += 1;
        }

        // POSIX: text files end with a newline. join() strips the trailing one
        // that lines() removed, so we restore it here.
        if result.is_empty() {
            String::new()
        } else {
            result.join("\n") + "\n"
        }
    }

    fn count_brace_depth(s: &str) -> i32 {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut quote_char = ' ';
        let chars: Vec<char> = s.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];

            // Handle Lua comments
            if !in_string && c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
                break;
            }

            if in_string {
                if c == quote_char && (i == 0 || chars[i - 1] != '\\') {
                    in_string = false;
                }
            } else if c == '\'' || c == '"' {
                in_string = true;
                quote_char = c;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
            }
            i += 1;
        }
        depth
    }

    fn update_lua_config(&self, content: &str, field: &ConfigField) -> String {
        let lua_value = self.to_lua_value(field);
        self.set_lua_config(content, field.lua_key, &lua_value)
    }

    /// Insert or replace a `config.<lua_key> = <lua_value>` line in the config.
    fn set_lua_config(&self, content: &str, lua_key: &str, lua_value: &str) -> String {
        let config_line = format!("config.{} = {}", lua_key, lua_value);
        let pattern = format!("config.{}", lua_key);

        let lines: Vec<&str> = content.lines().collect();
        let mut result: Vec<String> = Vec::new();
        let mut found = false;
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            // Keep comment lines
            if trimmed.starts_with("--") {
                result.push(line.to_string());
                i += 1;
                continue;
            }

            // Check if this line starts our target config
            if trimmed.starts_with(&pattern) {
                let after_pattern = &trimmed[pattern.len()..];
                if after_pattern.starts_with(|c: char| c.is_whitespace() || c == '=') {
                    // Found the config line to replace
                    found = true;
                    result.push(config_line.clone());

                    // Skip continuation lines if multi-line table
                    if let Some(eq_pos) = trimmed.find('=') {
                        let value_part = trimmed[eq_pos + 1..].trim();
                        let mut brace_depth = Self::count_brace_depth(value_part);

                        while brace_depth > 0 && i + 1 < lines.len() {
                            i += 1;
                            brace_depth += Self::count_brace_depth(lines[i]);
                        }
                    }
                    i += 1;
                    continue;
                }
            }

            result.push(line.to_string());
            i += 1;
        }

        if !found {
            // Find "return config" and insert before it
            if let Some(pos) = result.iter().position(|l| l.trim() == "return config") {
                result.insert(pos, config_line);
            } else {
                result.push(config_line);
            }
        }

        // POSIX: text files end with a newline. join() strips the trailing one
        // that lines() removed, so we restore it here.
        if result.is_empty() {
            String::new()
        } else {
            result.join("\n") + "\n"
        }
    }

    fn to_lua_value(&self, field: &ConfigField) -> String {
        match field.lua_key {
            "color_scheme" => {
                if field.value == "Auto" {
                    KAKU_AUTO_COLOR_SCHEME_EXPR.into()
                } else {
                    format!("'{}'", field.value)
                }
            }
            "font" => format!("wezterm.font('{}')", field.value),
            "font_size"
            | "line_height"
            | "window_background_opacity"
            | "macos_window_background_blur"
            | "split_pane_gap" => field.value.clone(),
            "copy_on_select"
            | "enable_scroll_bar"
            | "tab_close_confirmation"
            | "pane_close_confirmation"
            | "bell_tab_indicator"
            | "bell_dock_badge"
            | "remember_last_cwd"
            | "tab_title_show_basename_only" => {
                if field.value == "On" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            "hide_tab_bar_if_only_one_tab" => {
                if field.value == "Auto" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            "tab_bar_at_bottom" => {
                let effective = if field.value.is_empty() {
                    &field.default
                } else {
                    &field.value
                };
                if effective == "Bottom" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            "harfbuzz_features" => {
                if field.value == "On" {
                    "{}".into()
                } else {
                    "{ 'calt=0', 'clig=0', 'liga=0' }".into()
                }
            }
            "macos_global_hotkey" => {
                if field.value.is_empty() {
                    "nil".into()
                } else {
                    // confirm_edit() already validated; nil is a defensive fallback.
                    Self::hotkey_to_lua(&field.value).unwrap_or_else(|| "nil".into())
                }
            }
            _ => format!("'{}'", field.value),
        }
    }

    fn expects_numeric_input(lua_key: &str) -> bool {
        matches!(
            lua_key,
            "font_size"
                | "line_height"
                | "window_background_opacity"
                | "macos_window_background_blur"
        )
    }

    fn selecting_view(&self) -> Option<(&ConfigField, usize)> {
        if self.mode == Mode::Selecting {
            Some((&self.fields[self.selected], self.select_index))
        } else {
            None
        }
    }

    fn editing_view(&self) -> Option<(&ConfigField, &str, usize)> {
        if self.mode == Mode::Editing {
            Some((
                &self.fields[self.selected],
                &self.edit_buffer,
                self.edit_cursor,
            ))
        } else {
            None
        }
    }
}

fn open_config_in_editor(config_path: &Path) -> anyhow::Result<()> {
    open_path_in_editor(&config_path)
}

/// Send an OSC 1337 SetUserVar to signal kaku-gui that config has changed.
/// This triggers an immediate config reload instead of waiting for the file watcher.
fn signal_config_changed() {
    use std::io::Write;
    // OSC 1337 ; SetUserVar=name=base64(value) ST
    // name: KAKU_CONFIG_CHANGED, value: "1" -> base64 "MQ=="
    let seq = if std::env::var("TMUX").is_ok() {
        // tmux passthrough: wrap OSC in DCS tmux; ... ST
        b"\x1bPtmux;\x1b\x1b]1337;SetUserVar=KAKU_CONFIG_CHANGED=MQ==\x07\x1b\\" as &[u8]
    } else {
        b"\x1b]1337;SetUserVar=KAKU_CONFIG_CHANGED=MQ==\x07" as &[u8]
    };
    let _ = std::io::stdout().write_all(seq);
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_editable_config_exists, normal_mode_action, App, Mode, NormalModeAction,
        KAKU_AUTO_COLOR_SCHEME_EXPR,
    };
    use crossterm::event::KeyCode;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_app() -> App {
        App::new(PathBuf::from("/tmp/kaku-config-tui-test.lua"))
    }

    #[test]
    fn tab_bar_at_bottom_uses_default_when_value_is_empty() {
        let app = test_app();
        let field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "tab_bar_at_bottom")
            .expect("tab_bar_at_bottom field to exist");

        assert_eq!(app.to_lua_value(field), "true");
    }

    #[test]
    fn tab_bar_at_bottom_respects_explicit_top_selection() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "tab_bar_at_bottom")
            .expect("tab_bar_at_bottom field to exist");
        app.fields[idx].value = "Top".to_string();

        assert_eq!(app.to_lua_value(&app.fields[idx]), "false");
    }

    #[test]
    fn color_scheme_defaults_to_auto() {
        let app = test_app();
        let field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "color_scheme")
            .expect("color_scheme field to exist");

        assert_eq!(field.default, "Auto");
    }

    #[test]
    fn color_scheme_auto_serializes_to_dynamic_expression() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "color_scheme")
            .expect("color_scheme field to exist");
        app.fields[idx].value = "Auto".to_string();

        assert_eq!(
            app.to_lua_value(&app.fields[idx]),
            KAKU_AUTO_COLOR_SCHEME_EXPR
        );
    }

    #[test]
    fn load_config_round_trips_serialized_auto_theme_expression() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        std::fs::write(
            &config_path,
            format!("config.color_scheme = {KAKU_AUTO_COLOR_SCHEME_EXPR}\n"),
        )
        .expect("write config");

        let mut app = App::new(config_path);
        app.load_config();

        let field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "color_scheme")
            .expect("color_scheme field to exist");

        assert_eq!(field.value, "Auto");
        assert!(!field.skip_write);
    }

    #[test]
    fn scrollbar_field_defaults_to_off() {
        let app = test_app();
        let field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "enable_scroll_bar")
            .expect("enable_scroll_bar field to exist");

        assert_eq!(field.default, "Off");
        assert_eq!(app.to_lua_value(field), "false");
    }

    #[test]
    fn normalize_scrollbar_bool_values() {
        assert_eq!(
            App::normalize_value("enable_scroll_bar", "true"),
            Some("On".into())
        );
        assert_eq!(
            App::normalize_value("enable_scroll_bar", "false"),
            Some("Off".into())
        );
    }

    #[test]
    fn basename_only_tab_titles_default_to_off() {
        let app = test_app();
        let field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "tab_title_show_basename_only")
            .expect("tab_title_show_basename_only field to exist");

        assert_eq!(field.default, "Off");
        assert_eq!(app.to_lua_value(field), "false");
    }

    #[test]
    fn normalize_basename_only_tab_titles_bool_values() {
        assert_eq!(
            App::normalize_value("tab_title_show_basename_only", "true"),
            Some("On".into())
        );
        assert_eq!(
            App::normalize_value("tab_title_show_basename_only", "false"),
            Some("Off".into())
        );
    }

    #[test]
    fn close_confirmation_fields_default_to_off() {
        let app = test_app();
        let tab_field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "tab_close_confirmation")
            .expect("tab_close_confirmation field to exist");
        let pane_field = app
            .fields
            .iter()
            .find(|f| f.lua_key == "pane_close_confirmation")
            .expect("pane_close_confirmation field to exist");

        assert_eq!(tab_field.default, "Off");
        assert_eq!(pane_field.default, "Off");
        assert_eq!(app.to_lua_value(tab_field), "false");
        assert_eq!(app.to_lua_value(pane_field), "false");
    }

    #[test]
    fn normalize_close_confirmation_bool_values() {
        assert_eq!(
            App::normalize_value("tab_close_confirmation", "true"),
            Some("On".into())
        );
        assert_eq!(
            App::normalize_value("tab_close_confirmation", "false"),
            Some("Off".into())
        );
        assert_eq!(
            App::normalize_value("pane_close_confirmation", "true"),
            Some("On".into())
        );
        assert_eq!(
            App::normalize_value("pane_close_confirmation", "false"),
            Some("Off".into())
        );
    }

    #[test]
    fn finalize_active_input_commits_edit_buffer() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "font")
            .expect("font field to exist");
        app.selected = idx;

        app.start_edit();
        app.edit_insert('X');
        app.finalize_active_input();

        assert_eq!(app.fields[idx].value, "JetBrains MonoX");
        assert!(app.dirty);
    }

    #[test]
    fn start_edit_toggles_binary_option_fields() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "copy_on_select")
            .expect("copy_on_select field to exist");
        app.selected = idx;

        app.start_edit();

        assert_eq!(app.fields[idx].value, "Off");
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.dirty);
    }

    #[test]
    fn start_edit_opens_selector_for_multi_option_fields() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "color_scheme")
            .expect("color_scheme field to exist");
        app.selected = idx;

        app.start_edit();

        assert!(matches!(app.mode, Mode::Selecting));
        assert_eq!(app.select_index, 0);
        assert!(!app.dirty);
    }

    #[test]
    fn dynamic_color_scheme_expression_is_not_parsed_as_writable_value() {
        let content =
            "config.color_scheme = appearance == 'Dark' and 'Kaku Dark' or 'Kaku Light'\n";

        assert_eq!(App::extract_lua_value(content, "color_scheme"), None);
        assert!(App::has_config_line(content, "color_scheme"));
    }

    #[test]
    fn color_scheme_rejects_nil_literal() {
        assert_eq!(App::normalize_value("color_scheme", "nil"), None);
    }

    #[test]
    fn scientific_notation_numbers_are_supported() {
        assert_eq!(
            App::normalize_value("font_size", "1.0e2"),
            Some("1.0e2".into())
        );
        assert_eq!(
            App::extract_lua_value("config.font_size = 1.0e2\n", "font_size"),
            Some("1.0e2".into())
        );
    }

    #[test]
    fn ensure_editable_config_creates_missing_custom_path() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("nested").join("custom-kaku.lua");

        let ensured = ensure_editable_config_exists(Some(&config_path)).expect("ensure config");

        assert_eq!(ensured, config_path);
        assert!(config_path.is_file());
        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("local wezterm = require 'wezterm'"));
    }

    #[test]
    fn selecting_esc_confirms_and_marks_dirty() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "color_scheme")
            .expect("color_scheme field to exist");
        app.selected = idx;

        // Enter Selecting mode and move to a different option
        app.start_edit();
        assert!(matches!(app.mode, Mode::Selecting));
        app.select_down(); // move off default (index 0 → 1)

        // Simulate ESC: confirm_select() is called, which should set dirty
        app.confirm_select();

        assert!(matches!(app.mode, Mode::Normal));
        assert!(
            app.dirty,
            "ESC in Selecting mode should commit the selection"
        );
        assert_eq!(app.fields[idx].value, "Kaku Dark");
    }

    #[test]
    fn selecting_esc_without_value_change_does_not_mark_dirty() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "color_scheme")
            .expect("color_scheme field to exist");
        app.selected = idx;

        app.start_edit();
        assert!(matches!(app.mode, Mode::Selecting));

        app.confirm_select();

        assert!(matches!(app.mode, Mode::Normal));
        assert!(
            !app.dirty,
            "confirming the existing selection should not mark the app dirty"
        );
        assert_eq!(app.fields[idx].value, "");
    }

    #[test]
    fn normal_mode_maps_q_to_discard_and_escape_to_save() {
        assert_eq!(
            normal_mode_action(KeyCode::Char('q')),
            NormalModeAction::ExitDiscard
        );
        assert_eq!(
            normal_mode_action(KeyCode::Char('Q')),
            NormalModeAction::ExitDiscard
        );
        assert_eq!(
            normal_mode_action(KeyCode::Esc),
            NormalModeAction::ExitAndSave
        );
    }

    #[test]
    fn numeric_fields_accept_opacity_and_blur_values() {
        assert_eq!(
            App::normalize_value("window_background_opacity", "0.95"),
            Some("0.95".into())
        );
        assert_eq!(
            App::normalize_value("macos_window_background_blur", "20"),
            Some("20".into())
        );
    }

    #[test]
    fn invalid_numeric_edit_reverts_to_original_value() {
        let mut app = test_app();
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "window_background_opacity")
            .expect("window_background_opacity field to exist");
        app.selected = idx;
        app.fields[idx].value = "0.9".into();

        app.start_edit();
        app.edit_buffer = "not-a-number".into();
        app.edit_cursor = app.edit_buffer.chars().count();
        app.confirm_edit();

        assert_eq!(app.fields[idx].value, "0.9");
    }

    #[test]
    fn save_config_produces_trailing_newline() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        // Write a minimal valid config so save_config has something to update.
        std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\nlocal config = {}\nreturn config\n",
        )
        .expect("write config");

        let mut app = App::new(config_path.clone());
        app.load_config();

        // Toggle copy_on_select (a binary field) to make the state dirty.
        let idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "copy_on_select")
            .expect("copy_on_select field to exist");
        app.selected = idx;
        app.start_edit(); // toggles On → Off, sets dirty=true

        app.save_config().expect("save_config");

        let written = std::fs::read_to_string(&config_path).expect("read back");
        assert!(
            written.ends_with('\n'),
            "saved config must end with a newline, got: {:?}",
            &written[written.len().saturating_sub(10)..]
        );
    }

    // ── window_decorations: Traffic Lights + Shadow ───────────────────

    #[test]
    fn parse_window_decorations_all_six_supported_values() {
        assert_eq!(
            App::parse_window_decorations("INTEGRATED_BUTTONS"),
            Some((true, true, false))
        );
        assert_eq!(
            App::parse_window_decorations("INTEGRATED_BUTTONS|RESIZE"),
            Some((true, true, true))
        );
        assert_eq!(
            App::parse_window_decorations("RESIZE"),
            Some((false, true, true))
        );
        assert_eq!(
            App::parse_window_decorations("INTEGRATED_BUTTONS|MACOS_FORCE_DISABLE_SHADOW"),
            Some((true, false, false))
        );
        assert_eq!(
            App::parse_window_decorations("INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW"),
            Some((true, false, true))
        );
        assert_eq!(
            App::parse_window_decorations("RESIZE|MACOS_FORCE_DISABLE_SHADOW"),
            Some((false, false, true))
        );
    }

    #[test]
    fn parse_window_decorations_rejects_unsupported_flags() {
        assert_eq!(App::parse_window_decorations("TITLE|RESIZE"), None);
        assert_eq!(App::parse_window_decorations("NONE"), None);
        assert_eq!(
            App::parse_window_decorations("TITLE|INTEGRATED_BUTTONS|RESIZE"),
            None
        );
    }

    #[test]
    fn window_decorations_load_all_six_supported_values() {
        let cases = [
            ("INTEGRATED_BUTTONS", "On", "On", false),
            ("INTEGRATED_BUTTONS|RESIZE", "On", "On", true),
            (
                "INTEGRATED_BUTTONS|MACOS_FORCE_DISABLE_SHADOW",
                "On",
                "Off",
                false,
            ),
            (
                "INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW",
                "On",
                "Off",
                true,
            ),
            ("RESIZE", "Off", "On", true),
            ("RESIZE|MACOS_FORCE_DISABLE_SHADOW", "Off", "Off", true),
        ];
        for (lua_val, expected_tl, expected_shadow, expected_resize) in &cases {
            let dir = tempdir().expect("tempdir");
            let config_path = dir.path().join("kaku.lua");
            std::fs::write(
                &config_path,
                format!("config.window_decorations = \"{}\"\n", lua_val),
            )
            .expect("write config");

            let mut app = App::new(config_path);
            app.load_config();

            let tl = app
                .fields
                .iter()
                .find(|f| f.lua_key == "__wdeco_traffic_lights__")
                .unwrap();
            let sh = app
                .fields
                .iter()
                .find(|f| f.lua_key == "__wdeco_shadow__")
                .unwrap();

            assert_eq!(
                tl.value.as_str(),
                *expected_tl,
                "traffic lights for {}",
                lua_val
            );
            assert_eq!(
                sh.value.as_str(),
                *expected_shadow,
                "shadow for {}",
                lua_val
            );
            assert_eq!(
                app.window_decorations_resize, *expected_resize,
                "resize for {}",
                lua_val
            );
            assert!(!tl.skip_write, "tl skip_write for {}", lua_val);
            assert!(!sh.skip_write, "sh skip_write for {}", lua_val);
        }
    }

    #[test]
    fn window_decorations_save_all_four_combinations() {
        let cases: &[(&str, &str, Option<&str>)] = &[
            ("On", "On", None), // default: line removed
            ("Off", "On", Some("config.window_decorations = 'RESIZE'")),
            (
                "On",
                "Off",
                Some(
                    "config.window_decorations = 'INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW'",
                ),
            ),
            (
                "Off",
                "Off",
                Some("config.window_decorations = 'RESIZE|MACOS_FORCE_DISABLE_SHADOW'"),
            ),
        ];
        for (tl_val, sh_val, expected_line) in cases {
            let dir = tempdir().expect("tempdir");
            let config_path = dir.path().join("kaku.lua");
            std::fs::write(
                &config_path,
                "local wezterm = require 'wezterm'\nlocal config = {}\nreturn config\n",
            )
            .expect("write config");

            let mut app = App::new(config_path.clone());
            app.load_config();

            for f in app.fields.iter_mut() {
                if f.lua_key == "__wdeco_traffic_lights__" {
                    f.value = tl_val.to_string();
                    f.skip_write = false;
                } else if f.lua_key == "__wdeco_shadow__" {
                    f.value = sh_val.to_string();
                    f.skip_write = false;
                }
            }

            app.save_config().expect("save_config");
            let content = std::fs::read_to_string(&config_path).expect("read config");

            match expected_line {
                Some(line) => assert!(
                    content.contains(line),
                    "expected '{}' in:\n{}",
                    line,
                    content
                ),
                None => assert!(
                    !content.contains("window_decorations"),
                    "default should remove line, got:\n{}",
                    content
                ),
            }
        }
    }

    #[test]
    fn window_decorations_integrated_buttons_no_resize_preserved_on_save() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\nlocal config = {}\nconfig.window_decorations = \"INTEGRATED_BUTTONS\"\nreturn config\n",
        )
        .expect("write config");

        let mut app = App::new(config_path.clone());
        app.load_config();
        app.save_config().expect("save_config");

        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(
            content.contains("config.window_decorations = 'INTEGRATED_BUTTONS'"),
            "expected INTEGRATED_BUTTONS to be preserved, got:\n{}",
            content
        );
    }

    #[test]
    fn window_decorations_integrated_buttons_no_resize_preserved_on_shadow_toggle() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\nlocal config = {}\nconfig.window_decorations = \"INTEGRATED_BUTTONS\"\nreturn config\n",
        )
        .expect("write config");

        let mut app = App::new(config_path.clone());
        app.load_config();

        let shadow_idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "__wdeco_shadow__")
            .unwrap();
        app.selected = shadow_idx;
        app.start_edit();

        app.save_config().expect("save_config");

        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(
            content.contains(
                "config.window_decorations = 'INTEGRATED_BUTTONS|MACOS_FORCE_DISABLE_SHADOW'"
            ),
            "expected shadow toggle to preserve no-resize integrated buttons, got:\n{}",
            content
        );
    }

    #[test]
    fn window_decorations_unsupported_preserved_on_noop_save() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        let original = "local wezterm = require 'wezterm'\nlocal config = {}\nconfig.window_decorations = \"TITLE|RESIZE\"\nreturn config\n";
        std::fs::write(&config_path, original).expect("write");

        let mut app = App::new(config_path.clone());
        app.load_config();

        // Verify skip_write is set and bits are extracted correctly
        let tl = app
            .fields
            .iter()
            .find(|f| f.lua_key == "__wdeco_traffic_lights__")
            .unwrap();
        let sh = app
            .fields
            .iter()
            .find(|f| f.lua_key == "__wdeco_shadow__")
            .unwrap();
        assert!(tl.skip_write);
        assert!(sh.skip_write);
        // TITLE|RESIZE has no INTEGRATED_BUTTONS → TL=Off, no SHADOW flag → Shadow=On
        assert_eq!(tl.value, "Off");
        assert_eq!(sh.value, "On");

        // Save without touching either toggle
        app.save_config().expect("save_config");

        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(
            content.contains("\"TITLE|RESIZE\""),
            "unsupported value should be preserved, got:\n{}",
            content
        );
    }

    #[test]
    fn window_decorations_unsupported_converted_after_explicit_toggle() {
        // TITLE|RESIZE: bits → traffic_lights=Off, shadow=On.
        // Toggling traffic lights (Off→On) should produce INTEGRATED_BUTTONS|RESIZE
        // while preserving the shadow state.
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\nlocal config = {}\nconfig.window_decorations = \"TITLE|RESIZE\"\nreturn config\n",
        )
        .expect("write");

        let mut app = App::new(config_path.clone());
        app.load_config();

        // Verify extracted bits: TL=Off (no INTEGRATED_BUTTONS), Shadow=On
        let tl_idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "__wdeco_traffic_lights__")
            .unwrap();
        assert_eq!(app.fields[tl_idx].value, "Off");

        // Toggle traffic lights (Off → On), clears skip_write
        app.selected = tl_idx;
        app.start_edit();

        app.save_config().expect("save_config");
        let content = std::fs::read_to_string(&config_path).expect("read config");

        assert!(
            !content.contains("window_decorations"),
            "TL=On + Shadow=On is default, line should be removed, got:\n{}",
            content
        );
    }

    #[test]
    fn window_decorations_unsupported_with_shadow_off_preserves_shadow_on_toggle() {
        // TITLE|RESIZE|MACOS_FORCE_DISABLE_SHADOW: bits → TL=Off, Shadow=Off.
        // Toggling only traffic lights should preserve Shadow=Off.
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\nlocal config = {}\nconfig.window_decorations = \"TITLE|RESIZE|MACOS_FORCE_DISABLE_SHADOW\"\nreturn config\n",
        )
        .expect("write");

        let mut app = App::new(config_path.clone());
        app.load_config();

        // Verify extracted bits: TL=Off, Shadow=Off
        let tl = app
            .fields
            .iter()
            .find(|f| f.lua_key == "__wdeco_traffic_lights__")
            .unwrap();
        let sh = app
            .fields
            .iter()
            .find(|f| f.lua_key == "__wdeco_shadow__")
            .unwrap();
        assert_eq!(tl.value, "Off");
        assert_eq!(sh.value, "Off");
        assert!(tl.skip_write);
        assert!(sh.skip_write);

        // Toggle only traffic lights (Off → On)
        let tl_idx = app
            .fields
            .iter()
            .position(|f| f.lua_key == "__wdeco_traffic_lights__")
            .unwrap();
        app.selected = tl_idx;
        app.start_edit();

        app.save_config().expect("save_config");
        let content = std::fs::read_to_string(&config_path).expect("read config");

        // TL=On + Shadow=Off → INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW
        assert!(
            content.contains("config.window_decorations = 'INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW'"),
            "shadow should stay off when only TL was toggled, got:\n{}",
            content
        );
    }

    #[test]
    fn window_decorations_default_state_removes_explicit_line() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("kaku.lua");
        std::fs::write(
            &config_path,
            "local wezterm = require 'wezterm'\nlocal config = {}\nconfig.window_decorations = \"INTEGRATED_BUTTONS|RESIZE\"\nreturn config\n",
        )
        .expect("write");

        let mut app = App::new(config_path.clone());
        app.load_config();

        // Both loaded as "On" (default state). Save should remove the line.
        app.save_config().expect("save_config");
        let content = std::fs::read_to_string(&config_path).expect("read config");
        assert!(
            !content.contains("window_decorations"),
            "default state should remove explicit override, got:\n{}",
            content
        );
    }
}
