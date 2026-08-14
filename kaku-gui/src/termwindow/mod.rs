#![allow(clippy::range_plus_one)]
use super::renderstate::*;
use super::utilsprites::RenderMetrics;
use crate::colorease::ColorEase;
use crate::frontend::{front_end, refresh_fast_config_snapshot, try_front_end};
use crate::inputmap::InputMap;
use crate::overlay::launcher::{LauncherAction, LauncherTabEntry};
use crate::overlay::{
    confirm_close_pane, confirm_close_tab, confirm_quit_program, launcher, start_overlay,
    start_overlay_pane, CopyModeParams, CopyOverlay, LauncherArgs, LauncherFlags,
    QuickSelectOverlay,
};
use crate::resize_increment_calculator::ResizeIncrementCalculator;
use crate::scripting::guiwin::GuiWin;
use crate::scrollbar::*;
use crate::selection::Selection;
use crate::shapecache::*;
use crate::tabbar::{TabBarItem, TabBarState};
use crate::termwindow::background::{
    load_background_image, reload_background_image, LoadedBackgroundLayer,
};
use crate::termwindow::keyevent::{KeyTableArgs, KeyTableState, KeyboardInputState};
use crate::termwindow::modal::Modal;
use crate::termwindow::mouseevent::WindowDragState;
use crate::termwindow::render::paint::AllowImage;
use crate::termwindow::render::{
    CachedLineState, LineQuadCacheKey, LineQuadCacheValue, LineToEleShapeCacheKey,
    LineToElementShapeItem,
};
use crate::termwindow::webgpu::WebGpuState;
use ::wezterm_term::input::{ClickPosition, MouseButton as TMB};
use ::window::*;
use anyhow::{anyhow, ensure, Context};
use config::keyassignment::{
    Confirmation, KeyAssignment, LauncherActionArgs, PaneDirection, PaneEncoding, Pattern,
    PromptInputLine, QuickSelectArguments, RotationDirection, ScrollbackEraseMode, SpawnCommand,
    SplitSize,
};
use config::window::WindowLevel;
use config::{
    configuration, AudibleBell, ConfigHandle, Dimension, DimensionContext, FrontEndSelection,
    GeometryOrigin, GuiPosition, TermConfig, WindowCloseConfirmation,
};
use lfucache::*;
use mlua::{FromLua, LuaSerdeExt, UserData, UserDataFields};
use mux::pane::{
    CachePolicy, CloseReason, Pane, PaneId, Pattern as MuxPattern, PerformAssignmentResult,
};
use mux::renderable::RenderableDimensions;
use mux::tab::{
    PositionedPane, PositionedSplit, SplitDirection, SplitRequest, SplitSize as MuxSplitSize, Tab,
    TabId,
};
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use mux_lua::MuxPane;
use smol::channel::Sender;
use smol::Timer;
use std::cell::{RefCell, RefMut};
use std::collections::{HashMap, LinkedList};
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::SequenceNo;
use wezterm_dynamic::Value;
use wezterm_font::units::PixelLength;
use wezterm_font::FontConfiguration;
use wezterm_term::color::ColorPalette;
use wezterm_term::input::LastMouseClick;
use wezterm_term::{Alert, Progress, StableRowIndex, TerminalConfiguration, TerminalSize};
use wezterm_toast_notification::ToastNotification;

mod ai_chat;
pub mod background;
pub mod box_model;
pub mod charselect;
pub mod clipboard;
pub mod keyevent;
pub mod modal;
mod mouseevent;
pub mod palette;
pub mod paneselect;
mod prevcursor;
pub mod render;
pub mod resize;
mod selection;
pub mod spawn;
pub mod tab_rename;
pub mod webgpu;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputBroadcastMode {
    Off,
    CurrentTab,
    AllTabs,
}

fn scrollback_erase_mode_for_pane(
    requested: ScrollbackEraseMode,
    alternate_screen_active: bool,
) -> ScrollbackEraseMode {
    if requested == ScrollbackEraseMode::ScrollbackAndViewport && alternate_screen_active {
        ScrollbackEraseMode::ScrollbackOnly
    } else {
        requested
    }
}

impl InputBroadcastMode {
    fn applies_to_active_tab(
        self,
        target_tab_id: Option<TabId>,
        active_tab_id: Option<TabId>,
    ) -> bool {
        match self {
            Self::Off => false,
            Self::CurrentTab => target_tab_id.is_some() && target_tab_id == active_tab_id,
            Self::AllTabs => true,
        }
    }

    fn should_broadcast_input(
        self,
        target_tab_id: Option<TabId>,
        active_tab_id: Option<TabId>,
        is_active_mux_pane: bool,
        target_count: usize,
    ) -> bool {
        is_active_mux_pane
            && target_count >= 2
            && self.applies_to_active_tab(target_tab_id, active_tab_id)
    }

    fn title_prefix(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::CurrentTab => Some("[Broadcast: Tab]"),
            Self::AllTabs => Some("[Broadcast: All Tabs]"),
        }
    }

    fn disabled_toast(self) -> &'static str {
        match self {
            Self::Off => "Broadcast input off",
            Self::CurrentTab => "Current tab broadcast input off",
            Self::AllTabs => "All tabs broadcast input off",
        }
    }

    fn unavailable_toast(self) -> &'static str {
        match self {
            Self::Off => "Broadcast input needs at least 2 panes",
            Self::CurrentTab => "Current tab broadcast input needs at least 2 panes",
            Self::AllTabs => "All tabs broadcast input needs at least 2 panes",
        }
    }

    fn enabled_toast(self, target_count: usize) -> String {
        match self {
            Self::Off => format!("Broadcast input on ({target_count} panes)"),
            Self::CurrentTab => {
                format!("Current tab broadcast input on ({target_count} panes)")
            }
            Self::AllTabs => format!("All tabs broadcast input on ({target_count} panes)"),
        }
    }
}

use crate::spawn::SpawnWhere;
use prevcursor::PrevCursorPos;

const ATLAS_SIZE: usize = 128;
const VSCODE_OPEN_CANDIDATES: &[&str] = &[
    "code",
    "/usr/local/bin/code",
    "/opt/homebrew/bin/code",
    "/opt/local/bin/code",
    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
];

const TOP_TAB_LAYOUT_FULLSCREEN_STICKY_MS: u64 = 160;
/// Consecutive surface acquire timeouts before forcing a swapchain
/// reconfigure; the Metal backend reports a sleep/wake-invalidated layer
/// as Timeout (nil nextDrawable), see #458.
const CONSECUTIVE_TIMEOUTS_BEFORE_RECONFIGURE: u32 = 3;
/// Stop self-scheduling recovery repaints after this many consecutive
/// surface acquire failures; user input and PTY output still trigger
/// paint attempts beyond it.
const WEBGPU_SURFACE_RECOVERY_SELF_INVALIDATE_LIMIT: u32 = 120;

#[derive(Clone, Debug)]
struct FileLinkTarget {
    path: PathBuf,
    line: Option<usize>,
    col: Option<usize>,
}

fn decode_hex_event_payload(payload: &str) -> Option<String> {
    if payload.is_empty() || payload.len() % 2 != 0 {
        return None;
    }

    let mut bytes = Vec::with_capacity(payload.len() / 2);
    let chars: Vec<char> = payload.chars().collect();
    for i in (0..chars.len()).step_by(2) {
        let hi = chars[i].to_digit(16)?;
        let lo = chars[i + 1].to_digit(16)?;
        bytes.push(((hi << 4) | lo) as u8);
    }

    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub use config::is_light_color;

fn ai_toast_lifetime_ms(message: &str) -> u64 {
    let lower = message.to_ascii_lowercase();
    if lower.contains("checking")
        || lower.contains("analy")
        || lower.contains("fail")
        || lower.contains("error")
        || lower.contains("missing")
        || lower.contains("unavailable")
        || lower.contains("not found")
    {
        3000
    } else {
        2000
    }
}

fn normalize_bell_notification_source(value: &str) -> Option<String> {
    let first_line = value
        .lines()
        .next()?
        .trim()
        .trim_start_matches("Bell from ")
        .trim();
    if first_line.is_empty()
        || first_line.eq_ignore_ascii_case("kaku")
        || first_line.eq_ignore_ascii_case("wezterm")
        || first_line.eq_ignore_ascii_case("bell")
    {
        return None;
    }

    let normalized = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }

    let normalized_lower = normalized.to_ascii_lowercase();
    let normalized_len = normalized.chars().count();
    const SHORT_OK: &[&str] = &[
        "ls", "vi", "go", "rg", "fd", "gh", "jq", "hx", "cp", "mv", "rm", "sh", "nu",
    ];
    if normalized_len < 3 && !SHORT_OK.contains(&normalized_lower.as_str()) {
        return None;
    }

    const MAX_LEN: usize = 120;
    let needs_truncation = normalized.chars().count() > MAX_LEN;
    let mut text = normalized.chars().take(MAX_LEN).collect::<String>();
    if needs_truncation {
        text.push_str("...");
    }
    Some(text)
}

fn summarize_bell_source(value: &str) -> Option<String> {
    let normalized = normalize_bell_notification_source(value)?;
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    summarize_bell_tokens(&tokens).or(Some(normalized))
}

/// Number of tokens to skip past `sudo` and its option-arguments.
/// Returns the count of tokens consumed (not counting sudo itself).
fn sudo_args_skip_count(tokens: &[&str]) -> usize {
    const SUDO_OPT_WITH_ARG: &[&str] = &[
        "-u", "-g", "-C", "-D", "-R", "-T", "-h", "--user", "--group",
    ];
    let mut count = 0;
    while count < tokens.len() {
        let t = tokens[count];
        if SUDO_OPT_WITH_ARG.contains(&t) {
            if count + 1 < tokens.len() {
                count += 2; // skip flag + its argument
            } else {
                break;
            }
        } else if t.starts_with('-') {
            count += 1; // skip standalone flag
        } else {
            break;
        }
    }
    count
}

/// For commands that take subcommands (git, cargo, docker, ...), build the
/// display label. Returns `"{cmd} {sub} {sub2}"` for two-level subcommands
/// (e.g. `docker compose up`), `"{cmd} {sub}"` otherwise.
fn subcommand_label(command: &str, tokens_after_cmd: &[&str]) -> Option<String> {
    const TWO_LEVEL_PREFIX: &[&str] = &[
        "run", "exec", "compose", "get", "describe", "apply", "delete", "create",
    ];
    let next = tokens_after_cmd.first()?;
    if next.starts_with('-') || next.contains('=') {
        return None;
    }
    if TWO_LEVEL_PREFIX.contains(next) {
        if let Some(next2) = tokens_after_cmd.get(1) {
            if !next2.starts_with('-') && !next2.contains('=') {
                return Some(format!("{command} {next} {next2}"));
            }
        }
    }
    Some(format!("{command} {next}"))
}

fn summarize_bell_tokens(tokens: &[&str]) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }

    let mut idx = 0;
    while idx < tokens.len() {
        let token = tokens[idx];
        let token_lower = token.to_ascii_lowercase();

        if token_lower == "env"
            || token_lower == "command"
            || token_lower == "nohup"
            || token_lower == "time"
            || token_lower == "exec"
            || token_lower == "builtin"
        {
            idx += 1;
            continue;
        }

        if token_lower == "sudo" {
            idx += 1 + sudo_args_skip_count(&tokens[idx + 1..]);
            continue;
        }

        let looks_like_env_assignment = token.contains('=')
            && !token.starts_with('=')
            && !token.starts_with('-')
            && !token.contains('/');
        if looks_like_env_assignment {
            idx += 1;
            continue;
        }

        let is_shell = matches!(token_lower.as_str(), "bash" | "zsh" | "sh" | "fish" | "nu");
        if is_shell && idx + 2 < tokens.len() && matches!(tokens[idx + 1], "-c" | "-lc" | "-cl") {
            return summarize_bell_tokens(&tokens[idx + 2..]);
        }

        if token.starts_with('-') {
            idx += 1;
            continue;
        }

        let command = Path::new(token)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| token.to_string());

        // Skip decorative symbols (e.g. ✻ from Claude Code pane titles)
        if !command.chars().any(|c| c.is_alphanumeric()) {
            idx += 1;
            continue;
        }

        let command_lower = command.to_ascii_lowercase();
        let include_subcommand = matches!(
            command_lower.as_str(),
            "cargo"
                | "git"
                | "npm"
                | "pnpm"
                | "yarn"
                | "go"
                | "uv"
                | "python"
                | "python3"
                | "pip"
                | "brew"
                | "make"
                | "just"
                | "docker"
                | "kubectl"
        );
        if include_subcommand {
            if let Some(label) = subcommand_label(&command, &tokens[idx + 1..]) {
                return Some(label);
            }
        }
        return Some(command);
    }

    None
}

fn bell_notification_message(
    last_command: Option<&str>,
    reported_program: Option<&str>,
    pane_title: &str,
    foreground_process: Option<&str>,
) -> String {
    if let Some(command) = last_command.and_then(summarize_bell_source) {
        return format!("Task complete: {command}");
    }

    if let Some(program) = reported_program.and_then(summarize_bell_source) {
        return format!("Task complete: {program}");
    }

    if let Some(title) = summarize_bell_source(pane_title) {
        return format!("Task complete: {title}");
    }

    if let Some(process) = foreground_process
        .and_then(|value| {
            Path::new(value)
                .file_name()
                .map(|name| name.to_string_lossy().into())
        })
        .and_then(|value: String| summarize_bell_source(&value))
    {
        return format!("Task complete: {process}");
    }

    "Background task complete".to_string()
}

/// Lookup table for simple lazygit/yazi toast messages dispatched via EmitEvent.
fn lookup_kaku_toast(event_name: &str) -> Option<&'static str> {
    const KAKU_TOAST_MAP: &[(&str, &str)] = &[
        ("kaku-toast-try-lazygit", "Try Lazygit: Cmd+Shift+G"),
        ("kaku-toast-lazygit-no-pane", "Lazygit: No active pane"),
        (
            "kaku-toast-lazygit-no-cwd",
            "Lazygit: Cannot detect current directory",
        ),
        (
            "kaku-toast-lazygit-not-git",
            "Lazygit: Not a git repository",
        ),
        (
            "kaku-toast-lazygit-missing",
            "Lazygit not found. Run kaku init",
        ),
        (
            "kaku-toast-lazygit-dispatch-failed",
            "Lazygit: Dispatch failed",
        ),
        ("kaku-toast-yazi-no-pane", "Yazi: No active pane"),
        ("kaku-toast-yazi-missing", "Yazi not found. Run kaku init"),
        ("kaku-toast-yazi-dispatch-failed", "Yazi: Dispatch failed"),
    ];
    KAKU_TOAST_MAP
        .iter()
        .find(|(k, _)| *k == event_name)
        .map(|(_, v)| *v)
}

/// Lookup table for AI result-notice toast messages dispatched via EmitEvent.
fn lookup_ai_toast(event_name: &str) -> Option<&'static str> {
    const AI_TOAST_MAP: &[(&str, &str)] = &[
        (
            "kaku-toast-ai-ready",
            "Kaku Assistant suggestion ready. Press Cmd+Shift+E",
        ),
        ("kaku-toast-ai-unavailable", "Kaku Assistant unavailable"),
        (
            "kaku-toast-ai-missing-key",
            "Run kaku ai to set up Kaku Assistant.",
        ),
        ("kaku-toast-ai-no-pane", "No active pane"),
        ("kaku-toast-ai-no-suggestion", "No executable suggestion"),
        ("kaku-toast-ai-send-failed", "Failed to apply suggestion"),
        ("kaku-toast-ai-info", "Kaku Assistant update"),
    ];
    AI_TOAST_MAP
        .iter()
        .find(|(k, _)| *k == event_name)
        .map(|(_, v)| *v)
}

lazy_static::lazy_static! {
    static ref WINDOW_CLASS: Mutex<String> = Mutex::new(wezterm_gui_subcommands::DEFAULT_WINDOW_CLASS.to_owned());
    static ref POSITION: Mutex<Option<GuiPosition>> = Mutex::new(None);
    static ref RENDER_METRICS_CACHE: Mutex<Option<Vec<RenderMetricsCacheEntry>>> = Mutex::new(None);
}

/// Bounds both the in-memory entries and the on-disk cache file so that
/// zoom steps and multi-monitor DPI switches stay warm without letting the
/// cache grow without bound.
const RENDER_METRICS_CACHE_CAP: usize = 8;

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct RenderMetricsCacheKey {
    dpi: usize,
    font_scale_bits: u64,
    font_size_bits: u64,
    line_height_bits: u64,
    cell_width_bits: u64,
    font_fingerprint: u64,
}

/// Cached metrics are only valid for the font that produced them. Hashing
/// the configured text style (plus font_dirs, which changes how a family
/// name resolves) into the key means a `font` change invalidates instead of
/// serving stale cell geometry. The app version is included because shaping
/// or metrics behavior can change between releases.
fn font_config_fingerprint(config: &ConfigHandle) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    format!("{:?}", config.font).hash(&mut hasher);
    format!("{:?}", config.font_dirs).hash(&mut hasher);
    hasher.finish()
}

#[derive(Copy, Clone, Debug)]
struct RenderMetricsCacheEntry {
    key: RenderMetricsCacheKey,
    metrics: RenderMetrics,
}

#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RenderMetricsDiskEntry {
    key: RenderMetricsCacheKey,
    #[serde(default)]
    cap_height: Option<f64>,
    descender: f64,
    descender_row: isize,
    descender_plus_two: isize,
    underline_height: isize,
    strike_row: isize,
    cell_width: isize,
    cell_height: isize,
    #[serde(default)]
    line_height_y_adjust: f32,
    #[serde(default)]
    natural_cell_height: isize,
}

fn render_metrics_cache_file() -> PathBuf {
    config::DATA_DIR.join("render_metrics_cache_v2.json")
}

fn disk_entry_to_metrics(entry: &RenderMetricsDiskEntry) -> Option<RenderMetrics> {
    if !entry.descender.is_finite()
        || entry.cell_width <= 0
        || entry.cell_height <= 0
        || entry.underline_height <= 0
    {
        return None;
    }

    Some(RenderMetrics {
        cap_height: entry.cap_height.map(PixelLength::new),
        descender: PixelLength::new(entry.descender),
        descender_row: entry.descender_row,
        descender_plus_two: entry.descender_plus_two,
        underline_height: entry.underline_height,
        strike_row: entry.strike_row,
        cell_size: Size::new(entry.cell_width, entry.cell_height),
        line_height_y_adjust: entry.line_height_y_adjust,
        natural_cell_height: if entry.natural_cell_height > 0 {
            entry.natural_cell_height
        } else {
            entry.cell_height
        },
    })
}

fn load_render_metrics_entries_from_disk() -> Vec<RenderMetricsCacheEntry> {
    let file_name = render_metrics_cache_file();
    let data = match std::fs::read(&file_name) {
        Ok(data) => data,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::debug!(
                    "Failed to read render metrics cache {}: {}",
                    file_name.display(),
                    err
                );
            }
            return Vec::new();
        }
    };
    let entries: Vec<RenderMetricsDiskEntry> = match serde_json::from_slice(&data) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                "Failed to parse render metrics cache {}: {}",
                file_name.display(),
                err
            );
            return Vec::new();
        }
    };
    entries
        .iter()
        .filter_map(|entry| {
            disk_entry_to_metrics(entry).map(|metrics| RenderMetricsCacheEntry {
                key: entry.key,
                metrics,
            })
        })
        .take(RENDER_METRICS_CACHE_CAP)
        .collect()
}

fn persist_render_metrics_to_disk(entries: &[RenderMetricsCacheEntry]) {
    let file_name = render_metrics_cache_file();
    if let Some(parent) = file_name.parent() {
        if let Err(err) = config::create_user_owned_dirs(parent) {
            log::debug!(
                "Failed to create render metrics cache directory {}: {:#}",
                parent.display(),
                err
            );
        }
    }

    let disk_entries: Vec<RenderMetricsDiskEntry> = entries
        .iter()
        .map(|entry| RenderMetricsDiskEntry {
            key: entry.key,
            cap_height: entry.metrics.cap_height.map(|value| value.get()),
            descender: entry.metrics.descender.get(),
            descender_row: entry.metrics.descender_row,
            descender_plus_two: entry.metrics.descender_plus_two,
            underline_height: entry.metrics.underline_height,
            strike_row: entry.metrics.strike_row,
            cell_width: entry.metrics.cell_size.width,
            cell_height: entry.metrics.cell_size.height,
            line_height_y_adjust: entry.metrics.line_height_y_adjust,
            natural_cell_height: entry.metrics.natural_cell_height,
        })
        .collect();

    match serde_json::to_vec(&disk_entries) {
        Ok(data) => {
            if let Err(err) = std::fs::write(&file_name, data) {
                log::debug!(
                    "Failed to write render metrics cache {}: {}",
                    file_name.display(),
                    err
                );
            }
        }
        Err(err) => {
            log::debug!("Failed to serialize render metrics cache entries: {}", err);
        }
    }
}

fn render_metrics_from_cache_or_compute(
    fonts: &Rc<FontConfiguration>,
    config: &ConfigHandle,
    dpi: usize,
    font_scale: f64,
) -> anyhow::Result<(RenderMetrics, bool)> {
    let key = RenderMetricsCacheKey {
        dpi,
        font_scale_bits: font_scale.to_bits(),
        font_size_bits: config.font_size.to_bits(),
        line_height_bits: config.line_height.to_bits(),
        cell_width_bits: config.cell_width.to_bits(),
        font_fingerprint: font_config_fingerprint(config),
    };

    {
        let mut cache = RENDER_METRICS_CACHE.lock().unwrap();
        let entries = cache.get_or_insert_with(load_render_metrics_entries_from_disk);
        if let Some(idx) = entries.iter().position(|entry| entry.key == key) {
            // Keep most-recently-used first so cap eviction drops stale keys.
            let entry = entries.remove(idx);
            entries.insert(0, entry);
            return Ok((entry.metrics, true));
        }
    }

    let metrics = RenderMetrics::new(fonts)?;

    let mut cache = RENDER_METRICS_CACHE.lock().unwrap();
    let entries = cache.get_or_insert_with(Vec::new);
    entries.retain(|entry| entry.key != key);
    entries.insert(0, RenderMetricsCacheEntry { key, metrics });
    entries.truncate(RENDER_METRICS_CACHE_CAP);
    persist_render_metrics_to_disk(entries);
    Ok((metrics, false))
}

pub const ICON_DATA: &'static [u8] = include_bytes!("../../../assets/logo.png");

pub fn set_window_position(pos: GuiPosition) {
    POSITION.lock().unwrap().replace(pos);
}

pub fn set_window_class(cls: &str) {
    *WINDOW_CLASS.lock().unwrap() = cls.to_owned();
}

pub fn get_window_class() -> String {
    WINDOW_CLASS.lock().unwrap().clone()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MouseCapture {
    UI,
    TerminalPane(PaneId),
}

/// Type used together with Window::notify to do something in the
/// context of the window-specific event loop
pub enum TermWindowNotif {
    InvalidateShapeCache,
    PerformAssignment {
        pane_id: PaneId,
        assignment: KeyAssignment,
        tx: Option<Sender<anyhow::Result<()>>>,
    },
    SetLeftStatus(String),
    SetRightStatus(String),
    GetDimensions(Sender<(Dimensions, WindowState)>),
    GetSelectionForPane {
        pane_id: PaneId,
        tx: Sender<String>,
    },
    GetEffectiveConfig(Sender<ConfigHandle>),
    FinishWindowEvent {
        name: String,
        again: bool,
    },
    GetConfigOverrides(Sender<wezterm_dynamic::Value>),
    SetConfigOverrides(wezterm_dynamic::Value),
    CancelOverlayForPane(PaneId),
    CancelOverlayForTab {
        tab_id: TabId,
        pane_id: Option<PaneId>,
    },
    MuxNotification(MuxNotification),
    EmitStatusUpdate,
    EmitTitleUpdate,
    Apply(Box<dyn FnOnce(&mut TermWindow) + Send + Sync>),
    SwitchToMuxWindow(MuxWindowId),
    SetInnerSize {
        width: usize,
        height: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UIItemType {
    TabBar(TabBarItem),
    CloseTab(usize),
    AboveScrollThumb,
    ScrollThumb,
    BelowScrollThumb,
    Split(PositionedSplit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UIItem {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub item_type: UIItemType,
}

impl UIItem {
    pub fn hit_test(&self, x: isize, y: isize) -> bool {
        x >= self.x as isize
            && x <= (self.x + self.width) as isize
            && y >= self.y as isize
            && y <= (self.y + self.height) as isize
    }
}

#[derive(Clone, Default)]
pub struct SemanticZoneCache {
    seqno: SequenceNo,
    zones: Vec<StableRowIndex>,
}

pub struct OverlayState {
    pub pane: Arc<dyn Pane>,
    pub key_table_state: KeyTableState,
}

#[derive(Default)]
pub struct PaneState {
    /// If is_some(), the top row of the visible screen.
    /// Otherwise, the viewport is at the bottom of the
    /// scrollback.
    viewport: Option<StableRowIndex>,
    was_primary_peek: bool,
    selection: Selection,
    /// If is_some(), rather than display the actual tab
    /// contents, we're overlaying a little internal application
    /// tab.  We'll also route input to it.
    pub overlay: Option<OverlayState>,

    bell_start: Option<Instant>,
    pub has_unread_bell: bool,
    pub mouse_terminal_coords: Option<(ClickPosition, StableRowIndex)>,
}

/// Data used when synchronously formatting pane and window titles
#[derive(Debug, Clone)]
pub struct TabInformation {
    pub tab_id: TabId,
    pub tab_index: usize,
    pub is_active: bool,
    pub is_last_active: bool,
    pub active_pane: Option<PaneInformation>,
    pub window_id: MuxWindowId,
    pub tab_title: String,
}

impl UserData for TabInformation {
    fn add_fields<'lua, F: UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("tab_id", |_, this| Ok(this.tab_id.as_usize()));
        fields.add_field_method_get("tab_index", |_, this| Ok(this.tab_index));
        fields.add_field_method_get("is_active", |_, this| Ok(this.is_active));
        fields.add_field_method_get("is_last_active", |_, this| Ok(this.is_last_active));
        fields.add_field_method_get("active_pane", |_, this| {
            if let Some(pane) = &this.active_pane {
                Ok(Some(pane.clone()))
            } else {
                Ok(None)
            }
        });
        fields.add_field_method_get("panes", |_, this| {
            let mux = Mux::get();
            let mut panes = vec![];
            if let Some(tab) = mux.get_tab(this.tab_id) {
                panes = tab
                    .iter_panes()
                    .iter()
                    .map(TermWindow::pos_pane_to_pane_info)
                    .collect();
            }
            Ok(panes)
        });
        fields.add_field_method_get("window_id", |_, this| Ok(this.window_id));
        fields.add_field_method_get("tab_title", |_, this| Ok(this.tab_title.clone()));
        fields.add_field_method_get("window_title", |_, this| {
            let mux = Mux::get();
            let window = mux.get_window(this.window_id).ok_or_else(|| {
                mlua::Error::external(format!("window {} not found", this.window_id))
            })?;
            Ok(window.get_title().to_string())
        });
    }
}

/// Data used when synchronously formatting pane and window titles
#[derive(Debug, Clone)]
pub struct PaneInformation {
    pub pane_id: PaneId,
    pub pane_index: usize,
    pub is_active: bool,
    pub is_zoomed: bool,
    pub has_unseen_output: bool,
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
    pub pixel_width: usize,
    pub pixel_height: usize,
    pub title: String,
    pub user_vars: HashMap<String, String>,
    pub progress: Progress,
}

impl UserData for PaneInformation {
    fn add_fields<'lua, F: UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("pane_id", |_, this| Ok(this.pane_id.as_usize()));
        fields.add_field_method_get("pane_index", |_, this| Ok(this.pane_index));
        fields.add_field_method_get("is_active", |_, this| Ok(this.is_active));
        fields.add_field_method_get("is_zoomed", |_, this| Ok(this.is_zoomed));
        fields.add_field_method_get("has_unseen_output", |_, this| Ok(this.has_unseen_output));
        fields.add_field_method_get("left", |_, this| Ok(this.left));
        fields.add_field_method_get("top", |_, this| Ok(this.top));
        fields.add_field_method_get("width", |_, this| Ok(this.width));
        fields.add_field_method_get("height", |_, this| Ok(this.height));
        fields.add_field_method_get("pixel_width", |_, this| Ok(this.pixel_width));
        fields.add_field_method_get("pixel_height", |_, this| Ok(this.pixel_height));
        fields.add_field_method_get("progress", |lua, this| lua.to_value(&this.progress));
        fields.add_field_method_get("title", |_, this| Ok(this.title.clone()));
        fields.add_field_method_get("user_vars", |_, this| Ok(this.user_vars.clone()));
        fields.add_field_method_get("foreground_process_name", |_, this| {
            let mut name = None;
            if let Some(mux) = Mux::try_get() {
                if let Some(pane) = mux.get_pane(this.pane_id) {
                    name = pane.get_foreground_process_name(CachePolicy::AllowStale);
                }
            }
            match name {
                Some(name) => Ok(name),
                None => Ok("".to_string()),
            }
        });
        fields.add_field_method_get("tty_name", |_, this| {
            let mut name = None;
            if let Some(mux) = Mux::try_get() {
                if let Some(pane) = mux.get_pane(this.pane_id) {
                    name = pane.tty_name();
                }
            }
            Ok(name)
        });
        fields.add_field_method_get("current_working_dir", |_, this| {
            if let Some(mux) = Mux::try_get() {
                if let Some(pane) = mux.get_pane(this.pane_id) {
                    return Ok(pane
                        .get_current_working_dir(CachePolicy::AllowStale)
                        .map(|url| url_funcs::Url { url }));
                }
            }
            Ok(None)
        });
        fields.add_field_method_get("domain_name", |_, this| {
            let mut name = None;
            if let Some(mux) = Mux::try_get() {
                if let Some(pane) = mux.get_pane(this.pane_id) {
                    let domain_id = pane.domain_id();
                    name = mux
                        .get_domain(domain_id)
                        .map(|dom| dom.domain_name().to_string());
                }
            }
            match name {
                Some(name) => Ok(name),
                None => Ok("".to_string()),
            }
        });
    }
}

#[derive(Default)]
pub struct TabState {
    /// If is_some(), rather than display the actual tab
    /// contents, we're overlaying a little internal application
    /// tab.  We'll also route input to it.
    pub overlay: Option<OverlayState>,
}

/// Manages the state/queue of lua based event handlers.
/// We don't want to queue more than 1 event at a time,
/// so we use this enum to allow for at most 1 executing
/// and 1 pending event.
#[derive(Copy, Clone, Debug)]
enum EventState {
    /// The event is not running
    None,
    /// The event is running
    InProgress,
    /// The event is running, and we have another one ready to
    /// run once it completes
    InProgressWithQueued(Option<PaneId>),
}

/// State tracked during a live split-divider drag.
struct SplitDragState {
    tab_id: TabId,
}

/// State tracked during a live tab drag-reorder gesture.
struct TabDragState {
    tab_idx: usize,
    start_event: MouseEvent,
    has_dragged: bool,
    drag_offset_x: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineEditorSelectionDirection {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineEditorSelectionState {
    None,
    Charwise {
        direction: LineEditorSelectionDirection,
        count: usize,
    },
    ToStart,
    ToEnd,
    All,
    Unknown,
}

/// Discriminated union over the two GPU backends. Replaces the previous
/// `gl: Option<_>` + `webgpu: Option<_>` pair so the "exactly one alive"
/// invariant is enforced by the type system.
pub enum RenderBackend {
    OpenGl(Rc<glium::backend::Context>),
    WebGpu(Rc<WebGpuState>),
}

pub struct TermWindow {
    pub window: Option<Window>,
    pub config: ConfigHandle,
    pub config_overrides: wezterm_dynamic::Value,
    os_parameters: Option<parameters::Parameters>,
    /// When we most recently received keyboard focus
    pub focused: Option<Instant>,
    fonts: Rc<FontConfiguration>,
    /// Window dimensions and dpi
    pub dimensions: Dimensions,
    pub window_state: WindowState,
    pub resizes_pending: usize,
    is_repaint_pending: bool,
    pending_scale_changes: LinkedList<resize::ScaleChange>,
    /// Terminal dimensions
    terminal_size: TerminalSize,
    pub mux_window_id: MuxWindowId,
    pub mux_window_id_for_subscriptions: Arc<Mutex<MuxWindowId>>,
    pub render_metrics: RenderMetrics,
    render_state: Option<RenderState>,
    /// All keyboard-input state (input map, key tables, leader, dead keys).
    /// Bundled so the flat `TermWindow` field list stays smaller; see
    /// `KeyboardInputState` in `keyevent.rs`.
    keyboard: KeyboardInputState,
    show_tab_bar: bool,
    show_scroll_bar: bool,
    tab_bar: TabBarState,
    fancy_tab_bar: Option<box_model::ComputedElement>,
    pub right_status: String,
    pub left_status: String,
    last_ui_item: Option<UIItem>,
    last_mouse_coords: (usize, i64),
    /// All window-level drag / focus suppression flags (manual title-bar
    /// drag, OS edge-resize, click-to-focus). See `WindowDragState`.
    window_drag: WindowDragState,
    current_mouse_event: Option<MouseEvent>,
    prev_cursor: PrevCursorPos,
    /// Scrollbar UI state (last-snapshot, hover, fade-out timer).
    /// Bundled to keep the `TermWindow` field list shorter.
    scrollbar: ScrollbarState,
    line_editor_selection: LineEditorSelectionState,
    line_editor_selection_owner: Option<PaneId>,
    input_broadcast_mode: InputBroadcastMode,
    input_broadcast_tab_id: Option<TabId>,

    tab_state: RefCell<HashMap<TabId, TabState>>,
    pane_state: RefCell<HashMap<PaneId, PaneState>>,
    semantic_zones: HashMap<PaneId, SemanticZoneCache>,

    window_background: Vec<LoadedBackgroundLayer>,

    current_modifier_and_leds: (Modifiers, KeyboardLedStatus),
    current_mouse_buttons: Vec<MousePress>,
    current_mouse_capture: Option<MouseCapture>,
    /// True while the held left button is driving a terminal text selection,
    /// i.e. the press/drag actually resolved to a SelectTextAtMouseCursor or
    /// ExtendSelectionToMouseCursor assignment. A left press forwarded to a
    /// mouse-reporting application (claude code, vim, tmux with mouse on)
    /// leaves this false so wheel events are not hijacked into
    /// selection-extension (#455).
    selection_drag_active: bool,

    opengl_info: Option<String>,

    /// Keeps track of double and triple clicks
    last_mouse_click: Option<LastMouseClick>,

    /// The URL over which we are currently hovering
    current_highlight: Option<Arc<Hyperlink>>,

    quad_generation: usize,
    shape_generation: usize,
    shape_cache: RefCell<LfuCache<ShapeCacheKey, anyhow::Result<Rc<Vec<ShapedInfo>>>>>,
    line_to_ele_shape_cache: RefCell<LfuCache<LineToEleShapeCacheKey, LineToElementShapeItem>>,

    line_state_cache: RefCell<LfuCacheU64<Arc<CachedLineState>>>,
    next_line_state_id: u64,

    line_quad_cache: RefCell<LfuCache<LineQuadCacheKey, LineQuadCacheValue>>,

    /// Caches font.shape() results for box model rendering (tab bar, palette,
    /// notifications). Palette command text is stable across keystrokes, so
    /// shaping it once per unique string eliminates the dominant cost in
    /// compute_element. Keyed by font instance, presentation, and text;
    /// cleared on shape invalidation, config reload, and scale/DPI changes.
    box_text_shape_cache: RefCell<LfuCache<String, Vec<wezterm_font::GlyphInfo>>>,

    last_status_call: Instant,
    last_pane_output_invalidate: Option<Instant>,
    /// True when an output invalidation was suppressed by the 8ms coalescing
    /// window and a deferred follow-up has been scheduled.
    pending_output_invalidate: bool,
    status_update_queued: bool,
    title_update_queued: bool,
    cursor_blink_state: RefCell<ColorEase>,
    blink_state: RefCell<ColorEase>,
    rapid_blink_state: RefCell<ColorEase>,

    palette: Option<ColorPalette>,

    ui_items: Vec<UIItem>,
    dragging: Option<(UIItem, MouseEvent)>,
    split_drag_state: Option<SplitDragState>,
    tab_drag_state: Option<TabDragState>,
    /// Tab render offset animations: tab_idx -> (start_offset, ease)
    /// start_offset is the pixel distance from which the tab animates back to 0.
    tab_position_animations: HashMap<usize, (f32, Rc<RefCell<ColorEase>>)>,

    modal: RefCell<Option<Rc<dyn Modal>>>,

    event_states: HashMap<String, EventState>,
    pub current_event: Option<Value>,
    has_animation: RefCell<Option<Instant>>,
    /// We use this to attempt to do something reasonable
    /// if we run out of texture space
    allow_images: AllowImage,
    scheduled_animation: RefCell<Option<Instant>>,

    created: Instant,

    pub last_frame_duration: Duration,
    last_fps_check_time: Instant,
    num_frames: usize,
    pub first_paint_logged: bool,
    pub fps: f32,

    connection_name: String,

    /// Tracks whether we are currently in a live resize operation
    live_resizing: bool,
    pending_screen_change_resize: bool,
    pending_pty_flush_after_resize: bool,

    /// The active GPU rendering backend. Exactly one variant is alive for
    /// the lifetime of the window; `None` only while the window is still
    /// initializing. Lifts the previous `gl: Option<_> + webgpu: Option<_>`
    /// pair into the type system so "both Some" is no longer representable.
    render_backend: Option<RenderBackend>,
    config_subscription: Option<config::ConfigSubscription>,
    pending_config_reload_after_resize: bool,
    silent_reload_queued: bool,
    last_handled_appearance: Option<Appearance>,
    deferred_layout_relayout_epoch: usize,
    layout_sticky_fullscreen_until: Option<Instant>,
    closed_tab_history: std::collections::VecDeque<PathBuf>,

    /// Toast notification: (start_time, message, lifetime)
    toast: Option<(Instant, String, Duration)>,
    /// Shaped pixel width of the current toast message, keyed by
    /// (message, dpi, config generation). The toast repaints every frame
    /// while fading, and the message never changes mid-toast, so shaping it
    /// once is enough; the generation guards against a config reload
    /// changing the font under a repeated message.
    toast_shaped_width: Option<(String, usize, usize, f32)>,
    selection_copy_disabled_hint_shown: bool,
    last_window_title: String,

    /// Panes that currently have an ai_chat overlay open. The sender keeps each
    /// running overlay synchronized with config-driven palette changes.
    ai_chat_overlay_panes:
        HashMap<PaneId, std::sync::mpsc::Sender<crate::overlay::ai_chat::ChatPalette>>,
}

impl TermWindow {
    /// Accessor for the OpenGL backend, if active.
    pub(crate) fn opengl(&self) -> Option<&Rc<glium::backend::Context>> {
        match &self.render_backend {
            Some(RenderBackend::OpenGl(gl)) => Some(gl),
            _ => None,
        }
    }

    /// Accessor for the WebGpu backend, if active.
    pub(crate) fn webgpu(&self) -> Option<&Rc<WebGpuState>> {
        match &self.render_backend {
            Some(RenderBackend::WebGpu(w)) => Some(w),
            _ => None,
        }
    }

    fn should_reload_config_for_user_var(name: &str, _window_contains_pane: bool) -> bool {
        // We used to require `window_contains_pane && name == "KAKU_CONFIG_CHANGED"`
        // to avoid duplicate reloads when multiple windows are open. However, when
        // a user exits `kaku config` very quickly (e.g. by hitting ESC), the pane
        // might already be removed from Mux before the OSC 1337 is processed by
        // `emit_user_var_event`, meaning `window_contains_pane` evaluates to false.
        // This caused the config reload to be dropped. By returning true globally
        // for `KAKU_CONFIG_CHANGED`, we ensure the reload happens. The `config::reload()`
        // function has its own debouncing mechanism (reload_epoch) to prevent
        // duplicate reloads across windows.
        name == "KAKU_CONFIG_CHANGED"
    }

    fn load_os_parameters(&mut self) {
        if let Some(ref window) = self.window {
            self.os_parameters = match window
                .get_os_parameters(&self.config, self.effective_layout_window_state())
            {
                Ok(os_parameters) => os_parameters,
                Err(err) => {
                    log::warn!("Error while getting OS parameters: {:#}", err);
                    None
                }
            };
        }
    }

    fn close_requested(&mut self, _window: &Window) {
        // AppKit sends `windowShouldClose:` to every NSWindow during Cmd+Q as
        // well as for plain Cmd+W. `on_app_terminating` sets the global flag
        // before those quit-time `windowShouldClose:` calls fan out, so this
        // check reliably tells the two paths apart on macOS. Treating quit-
        // time closes as "user closed a window" would push every live id into
        // LOGICALLY_CLOSED_WINDOWS and starve the session save at main.rs:940.
        #[cfg(target_os = "macos")]
        let is_app_quitting = ::window::is_app_terminating();
        #[cfg(not(target_os = "macos"))]
        let is_app_quitting = false;

        if !is_app_quitting {
            if self.config.restore_previous_session {
                let _ = crate::session_restore::save_closed_window_snapshot(self.mux_window_id);
            }
            crate::session_restore::mark_window_logically_closed(self.mux_window_id);
            crate::session_restore::mark_dirty();
        }
        #[cfg(target_os = "macos")]
        {
            // On macOS, hide the window instead of destroying it so that tabs,
            // panes, and child processes are preserved. Clicking the Dock icon
            // calls application_open_untitled_file which finds the window in
            // conn.windows and focuses it back.
            _window.order_out();
            return;
        }

        #[cfg(not(target_os = "macos"))]
        {
            let mux = Mux::get();
            match self.config.window_close_confirmation {
                WindowCloseConfirmation::NeverPrompt => {
                    mux.kill_window(self.mux_window_id);
                    _window.close();
                    front_end().forget_known_window(_window);
                }
                // SmartPrompt and AlwaysPrompt share the same close path:
                // both already skip the prompt when the window has no
                // stateful process via can_close_without_prompting() below.
                WindowCloseConfirmation::AlwaysPrompt | WindowCloseConfirmation::SmartPrompt => {
                    let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                        Some(tab) => tab,
                        None => {
                            mux.kill_window(self.mux_window_id);
                            _window.close();
                            front_end().forget_known_window(_window);
                            return;
                        }
                    };

                    let mux_window_id = self.mux_window_id;

                    let can_close = mux
                        .get_window(mux_window_id)
                        .map_or(false, |w| w.can_close_without_prompting());
                    if can_close {
                        mux.kill_window(self.mux_window_id);
                        _window.close();
                        front_end().forget_known_window(_window);
                        return;
                    }
                    if let Some(window) = self.window.clone() {
                        let (overlay, future) = start_overlay(self, &tab, move |tab_id, term| {
                            confirm_close_window(term, mux_window_id, window, tab_id)
                        });
                        self.assign_overlay(tab.tab_id(), overlay);
                        promise::spawn::spawn(future).detach();
                    }

                    // Don't close right now; let the close happen from
                    // the confirmation overlay
                }
            }
        }
    }

    fn focus_changed(&mut self, focused: bool, window: &Window) {
        log::trace!("Setting focus to {:?}", focused);
        if focused {
            // On macOS a closed window is only hidden (see close_requested) and a
            // Dock click brings it back via makeKeyAndOrderFront, which lands here.
            // The window is in use again, so drop its logically-closed marker;
            // otherwise the exit-time session save skips it forever and the stale
            // last_session.json resurrects tabs the user already closed.
            crate::session_restore::forget_logically_closed(self.mux_window_id);
        }
        if !self.config.tab_bar_at_bottom && self.layout_is_effective_fullscreen() {
            self.arm_layout_sticky_fullscreen();
        }
        self.focused = if focused { Some(Instant::now()) } else { None };
        self.quad_generation += 1;
        self.load_os_parameters();
        self.invalidate_fancy_tab_bar();

        if let Some(modal) = self.get_modal() {
            modal.focus_changed(focused, self);
        }

        if self.focused.is_none() {
            self.last_mouse_click = None;
            self.current_mouse_buttons.clear();
            self.current_mouse_capture = None;
            self.selection_drag_active = false;
            self.window_drag.is_click_to_focus = false;

            for state in self.pane_state.borrow_mut().values_mut() {
                state.mouse_terminal_coords.take();
            }
        }

        // Reset the cursor blink phase
        self.prev_cursor.bump();

        let immediate_relayout_needed = self.config.tab_bar_at_bottom; // Only immediate for bottom-tab

        if focused && immediate_relayout_needed {
            // Some macOS Space switches do not reliably emit a visibility-change
            // callback. Re-apply layout on focus gain for bottom-tab layouts,
            // where stale tab-bar visibility is most noticeable.
            self.sync_tab_bar_visibility_for_window_state("focus_changed:sync_tab_bar");
            let dimensions = self.dimensions;
            self.apply_dimensions(&dimensions, None, window, true);
            self.schedule_deferred_layout_relayout(window);
        } else if focused && !self.config.tab_bar_at_bottom {
            // Top-tab (both fullscreen and non-fullscreen): ONLY run a deferred
            // relayout. Lua config overrides (window_padding) take time to arrive
            // after we gain focus. If we `apply_dimensions` immediately, we will
            // draw 1 frame with the old padding, causing a visible flicker.
            // Deferring gives Lua's `window-focus-changed` handler time to push
            // the new override.
            self.schedule_deferred_layout_relayout(window);
        }

        // force cursor to be repainted
        window.invalidate();

        if let Some(pane) = self.get_active_pane_or_overlay() {
            pane.focus_changed(focused);
            if focused {
                let mut state = self.pane_state(pane.pane_id());
                if state.has_unread_bell {
                    state.has_unread_bell = false;
                    drop(state);
                    front_end().adjust_unread_bell_count(-1);
                }
            }
        }

        self.update_title();
        self.emit_window_event("window-focus-changed", None);
    }

    fn visibility_changed(&mut self, visible: bool, window: &Window) {
        log::trace!("Setting visibility to {:?}", visible);
        if !self.config.tab_bar_at_bottom && self.layout_is_effective_fullscreen() {
            self.arm_layout_sticky_fullscreen();
        }
        self.quad_generation += 1;

        if visible {
            let immediate_relayout_needed = self.config.tab_bar_at_bottom;

            if immediate_relayout_needed {
                self.load_os_parameters();
                self.invalidate_fancy_tab_bar();
                self.sync_tab_bar_visibility_for_window_state("visibility_changed:sync_tab_bar");
                let dimensions = self.dimensions;
                self.apply_dimensions(&dimensions, None, window, true);
                self.schedule_deferred_layout_relayout(window);
            } else if !self.config.tab_bar_at_bottom {
                // Top-tab: ONLY run deferred relayout.
                // Prevent 1-frame flicker while waiting for Lua padding overrides
                // just like in `focus_changed`.
                self.load_os_parameters();
                self.invalidate_fancy_tab_bar();
                self.schedule_deferred_layout_relayout(window);
            }
        } else {
            self.invalidate_fancy_tab_bar();
        }

        // Only repaint when becoming visible; an occluded window doesn't need a
        // compositor frame and the extra invalidate burns power for no visual gain.
        if visible {
            window.invalidate();
        }
    }

    fn created(&mut self, ctx: RenderContext) -> anyhow::Result<()> {
        self.render_state = None;

        let render_info = ctx.renderer_info();
        self.opengl_info.replace(render_info.clone());

        match RenderState::new(ctx, &self.fonts, &self.render_metrics, ATLAS_SIZE) {
            Ok(render_state) => {
                log::debug!(
                    "OpenGL initialized! {} Kaku version: {}",
                    render_info,
                    config::wezterm_version(),
                );
                self.render_state.replace(render_state);
            }
            Err(err) => {
                log::error!("failed to create RenderState: {}", err);
            }
        }

        if self.render_state.is_none() {
            panic!("No OpenGL");
        }

        Ok(())
    }
}

impl TermWindow {
    fn arm_layout_sticky_fullscreen(&mut self) {
        if self.config.tab_bar_at_bottom {
            return;
        }

        self.layout_sticky_fullscreen_until =
            Some(Instant::now() + Duration::from_millis(TOP_TAB_LAYOUT_FULLSCREEN_STICKY_MS));
    }

    fn layout_sticky_fullscreen_active(&self) -> bool {
        !self.config.tab_bar_at_bottom
            && self
                .layout_sticky_fullscreen_until
                .map(|until| Instant::now() < until)
                .unwrap_or(false)
    }

    pub(crate) fn effective_layout_window_state(&self) -> WindowState {
        let mut state = self.window_state;
        if self.layout_sticky_fullscreen_active() {
            state |= WindowState::FULL_SCREEN;
        }
        state
    }

    pub(crate) fn layout_is_effective_fullscreen(&self) -> bool {
        self.effective_layout_window_state()
            .contains(WindowState::FULL_SCREEN)
    }

    /// Returns true when the currently focused pane has the AI chat overlay
    /// active. Used by the resize path to temporarily shrink the bottom
    /// padding so the chat box sits closer to the window / tab-bar edge.
    fn has_ai_chat_overlay_on_active_pane(&self) -> bool {
        let mux = mux::Mux::try_get();
        let pane_id = mux
            .and_then(|m| m.get_active_tab_for_window(self.mux_window_id))
            .and_then(|tab| tab.get_active_pane().map(|p| p.pane_id()));
        match pane_id {
            Some(id) => self.ai_chat_overlay_panes.contains_key(&id),
            None => false,
        }
    }

    /// Fullscreen should keep the historical V0.7.1 padding behavior.
    /// Only maximized windows opt into the newer edge-to-edge padding path.
    pub(crate) fn layout_uses_edge_to_edge_padding(&self) -> bool {
        self.window_state.contains(WindowState::MAXIMIZED)
    }

    fn schedule_deferred_layout_relayout(&mut self, window: &Window) {
        // Deferred relayout runs for all layout modes (bottom-tab, top-tab, fullscreen).
        // Top-tab is included because Lua's config override (window_padding /
        // hide_tab_bar_if_only_one_tab) may arrive after focus/visibility callbacks.

        // Give top-tab fullscreen extra time to let Lua's update-right-status
        // config override settle before we recompute dimensions.  For all
        // other cases the original 16 ms frame-skip is sufficient.
        let delay_ms: u64 =
            if !self.config.tab_bar_at_bottom && self.layout_is_effective_fullscreen() {
                80
            } else {
                16
            };
        self.deferred_layout_relayout_epoch = self.deferred_layout_relayout_epoch.wrapping_add(1);
        let epoch = self.deferred_layout_relayout_epoch;

        let window = window.clone();
        promise::spawn::spawn_into_main_thread(async move {
            // Defer one frame so macOS can settle fullscreen/Space visibility state.
            Timer::after(Duration::from_millis(delay_ms)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.deferred_layout_relayout_epoch != epoch {
                    return;
                }
                if let Some(owned_window) = tw.window.as_ref().cloned() {
                    tw.load_os_parameters();
                    tw.invalidate_fancy_tab_bar();
                    tw.sync_tab_bar_visibility_for_window_state(
                        "deferred_layout_relayout:sync_tab_bar",
                    );
                    let dimensions = tw.dimensions;
                    tw.apply_dimensions(&dimensions, None, &owned_window, true);
                    owned_window.invalidate();
                }
            })));
        })
        .detach();
    }

    fn schedule_silent_config_reload(&mut self, window: &Window) {
        if self.silent_reload_queued {
            return;
        }
        self.silent_reload_queued = true;
        let window = window.clone();
        promise::spawn::spawn_into_main_thread(async move {
            // Coalesce rapid override updates and run after current event dispatch.
            Timer::after(Duration::from_millis(1)).await;
            window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                tw.config_was_reloaded_silently();
                tw.silent_reload_queued = false;
            })));
        })
        .detach();
    }

    pub async fn new_window(mux_window_id: MuxWindowId) -> anyhow::Result<()> {
        crate::startup_trace::mark("TermWindow::new_window ENTER");
        let config = configuration();
        let dpi = config.dpi.unwrap_or_else(|| ::window::default_dpi()) as usize;
        crate::startup_trace::mark("  FontConfiguration#2 start");
        let fontconfig = Rc::new(FontConfiguration::new(Some(config.clone()), dpi)?);
        crate::startup_trace::mark("  FontConfiguration#2 done");
        let persisted_font_scale = resize::load_persisted_font_scale(&config);
        if let Some(font_scale) = persisted_font_scale {
            fontconfig.change_scaling(font_scale, dpi);
        }

        let mux = Mux::get();
        crate::startup_trace::mark("    mux.get_active_tab_for_window start");
        let size = match mux.get_active_tab_for_window(mux_window_id) {
            Some(tab) => tab.get_size(),
            None => {
                log::debug!("new_window has no tabs... yet?");
                Default::default()
            }
        };
        crate::startup_trace::mark("    mux.get_active_tab_for_window done");
        let physical_rows = size.rows as usize;
        let physical_cols = size.cols as usize;

        crate::startup_trace::mark("    render_metrics start");
        let (render_metrics, _metrics_cache_hit) = render_metrics_from_cache_or_compute(
            &fontconfig,
            &config,
            dpi,
            persisted_font_scale.unwrap_or(1.0),
        )?;
        crate::startup_trace::mark("    render_metrics done");
        log::trace!("using render_metrics {:#?}", render_metrics);

        // Initially we have only a single tab, so take that into account
        // for the tab bar state.
        let show_tab_bar = config.enable_tab_bar && !config.hide_tab_bar_if_only_one_tab;
        crate::startup_trace::mark("    tab_bar_pixel_height start");
        // Use a cheap estimate based on terminal cell metrics to avoid paying
        // the title-font resolution cost (~485ms on macOS cold start) during
        // window creation. The real height is computed by the instance method
        // tab_bar_pixel_height() on first render.
        let tab_bar_height = if show_tab_bar {
            Self::estimated_tab_bar_pixel_height(&config, &render_metrics) as usize
        } else {
            0
        };
        crate::startup_trace::mark("    tab_bar_pixel_height done");

        let terminal_size = TerminalSize {
            rows: physical_rows,
            cols: physical_cols,
            pixel_width: (render_metrics.cell_size.width as usize * physical_cols),
            pixel_height: (render_metrics.cell_size.height as usize * physical_rows),
            dpi: dpi as u32,
        };

        if terminal_size != size {
            // DPI is different from the default assumed DPI when the mux
            // created the pty. We need to inform the kernel of the revised
            // pixel geometry now
            log::trace!(
                "Initial geometry was {:?} but dpi-adjusted geometry \
                        is {:?}; update the kernel pixel geometry for the ptys!",
                size,
                terminal_size,
            );
            if let Some(window) = mux.get_window(mux_window_id) {
                for tab in window.iter() {
                    tab.resize(terminal_size);
                }
            };
        }

        let h_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: terminal_size.pixel_width as f32,
            pixel_cell: render_metrics.cell_size.width as f32,
        };
        let padding_left = config.window_padding.left.evaluate_as_pixels(h_context) as usize;
        let padding_right = resize::effective_right_padding(&config, h_context) as usize;
        let v_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: terminal_size.pixel_height as f32,
            pixel_cell: render_metrics.cell_size.height as f32,
        };
        let (padding_top, padding_bottom) = resize::effective_vertical_padding(
            &config,
            v_context,
            show_tab_bar,
            config.tab_bar_at_bottom,
            tab_bar_height,
            false,
        );

        let mut dimensions = Dimensions {
            pixel_width: (terminal_size.pixel_width + padding_left + padding_right) as usize,
            pixel_height: ((terminal_size.rows * render_metrics.cell_size.height as usize)
                + padding_top
                + padding_bottom) as usize
                + tab_bar_height,
            dpi,
        };

        let mut border = Self::get_os_border_impl(&None, &config, &dimensions, &render_metrics);

        // Mirror get_os_border() for non-fullscreen startup windows.
        let integrated_top_inset = crate::termwindow::render::borders::integrated_buttons_top_inset(
            &config,
            false,
            show_tab_bar && !config.tab_bar_at_bottom,
            dpi as f32,
        );
        if integrated_top_inset > 0 {
            border.top += ULength::new(integrated_top_inset);
        }

        dimensions.pixel_height += (border.top + border.bottom).get() as usize;
        dimensions.pixel_width += (border.left + border.right).get() as usize;

        crate::startup_trace::mark("    load_background_image start");
        let window_background = load_background_image(&config, &dimensions, &render_metrics);
        crate::startup_trace::mark("    load_background_image done");

        log::trace!(
            "TermWindow::new_window called with mux_window_id {} {:?} {:?}",
            mux_window_id,
            terminal_size,
            dimensions
        );

        let render_state = None;

        let connection_name = Connection::get().map_or_else(
            || {
                log::warn!(
                    "window connection is not initialized while creating TermWindow; using placeholder"
                );
                "uninitialized".to_string()
            },
            |conn| conn.name(),
        );

        let myself = Self {
            created: Instant::now(),
            connection_name,
            last_fps_check_time: Instant::now(),
            num_frames: 0,
            first_paint_logged: false,
            last_frame_duration: Duration::ZERO,
            fps: 0.,
            config_subscription: None,
            pending_config_reload_after_resize: false,
            silent_reload_queued: false,
            last_handled_appearance: None,
            deferred_layout_relayout_epoch: 0,
            layout_sticky_fullscreen_until: None,
            closed_tab_history: std::collections::VecDeque::new(),
            os_parameters: None,
            render_backend: None,
            window: None,
            window_background,
            config: config.clone(),
            config_overrides: wezterm_dynamic::Value::default(),
            palette: None,
            focused: None,
            mux_window_id,
            mux_window_id_for_subscriptions: Arc::new(Mutex::new(mux_window_id)),
            fonts: Rc::clone(&fontconfig),
            render_metrics,
            dimensions,
            window_state: WindowState::default(),
            resizes_pending: 0,
            is_repaint_pending: false,
            pending_scale_changes: LinkedList::new(),
            terminal_size,
            render_state,
            keyboard: KeyboardInputState::new(InputMap::new(&config)),
            show_tab_bar,
            show_scroll_bar: config.enable_scroll_bar,
            tab_bar: TabBarState::default(),
            fancy_tab_bar: None,
            right_status: String::new(),
            left_status: String::new(),
            last_mouse_coords: (0, -1),
            window_drag: WindowDragState::default(),
            current_mouse_event: None,
            current_modifier_and_leds: Default::default(),
            prev_cursor: PrevCursorPos::new(),
            scrollbar: ScrollbarState::default(),
            line_editor_selection: LineEditorSelectionState::None,
            line_editor_selection_owner: None,
            input_broadcast_mode: InputBroadcastMode::Off,
            input_broadcast_tab_id: None,
            tab_state: RefCell::new(HashMap::new()),
            pane_state: RefCell::new(HashMap::new()),
            current_mouse_buttons: vec![],
            current_mouse_capture: None,
            selection_drag_active: false,
            last_mouse_click: None,
            current_highlight: None,
            quad_generation: 0,
            shape_generation: 0,
            shape_cache: RefCell::new(LfuCache::new(
                "shape_cache.hit.rate",
                "shape_cache.miss.rate",
                |config| config.shape_cache_size,
                &config,
            )),
            line_state_cache: RefCell::new(LfuCacheU64::new(
                "line_state_cache.hit.rate",
                "line_state_cache.miss.rate",
                |config| config.line_state_cache_size,
                &config,
            )),
            next_line_state_id: 0,
            line_quad_cache: RefCell::new(LfuCache::new(
                "line_quad_cache.hit.rate",
                "line_quad_cache.miss.rate",
                |config| config.line_quad_cache_size,
                &config,
            )),
            line_to_ele_shape_cache: RefCell::new(LfuCache::new(
                "line_to_ele_shape_cache.hit.rate",
                "line_to_ele_shape_cache.miss.rate",
                |config| config.line_to_ele_shape_cache_size,
                &config,
            )),
            box_text_shape_cache: RefCell::new(LfuCache::new(
                "box_text_shape_cache.hit.rate",
                "box_text_shape_cache.miss.rate",
                |_| 1024,
                &config,
            )),
            last_status_call: Instant::now(),
            last_pane_output_invalidate: None,
            pending_output_invalidate: false,
            status_update_queued: false,
            title_update_queued: false,
            cursor_blink_state: RefCell::new(ColorEase::new(
                config.cursor_blink_rate,
                config.cursor_blink_ease_in,
                config.cursor_blink_rate,
                config.cursor_blink_ease_out,
                None,
            )),
            blink_state: RefCell::new(ColorEase::new(
                config.text_blink_rate,
                config.text_blink_ease_in,
                config.text_blink_rate,
                config.text_blink_ease_out,
                None,
            )),
            rapid_blink_state: RefCell::new(ColorEase::new(
                config.text_blink_rate_rapid,
                config.text_blink_rapid_ease_in,
                config.text_blink_rate_rapid,
                config.text_blink_rapid_ease_out,
                None,
            )),
            event_states: HashMap::new(),
            current_event: None,
            has_animation: RefCell::new(None),
            scheduled_animation: RefCell::new(None),
            allow_images: AllowImage::Yes,
            semantic_zones: HashMap::new(),
            ui_items: vec![],
            dragging: None,
            split_drag_state: None,
            tab_drag_state: None,
            tab_position_animations: HashMap::new(),
            last_ui_item: None,
            modal: RefCell::new(None),
            opengl_info: None,
            toast: None,
            toast_shaped_width: None,
            selection_copy_disabled_hint_shown: false,
            last_window_title: String::new(),
            ai_chat_overlay_panes: HashMap::new(),
            live_resizing: false,
            pending_screen_change_resize: false,
            pending_pty_flush_after_resize: false,
        };

        let tw = Rc::new(RefCell::new(myself));
        let tw_event = Rc::clone(&tw);

        let mut x = None;
        let mut y = None;
        let mut origin = GeometryOrigin::default();

        if let Some(position) = mux
            .get_window(mux_window_id)
            .and_then(|window| window.get_initial_position().clone())
            .or_else(|| POSITION.lock().unwrap().take())
        {
            x.replace(position.x);
            y.replace(position.y);
            origin = position.origin;
        }

        let geometry = RequestedWindowGeometry {
            width: Dimension::Pixels(dimensions.pixel_width as f32),
            height: Dimension::Pixels(dimensions.pixel_height as f32),
            x,
            y,
            origin,
        };
        log::trace!("{:?}", geometry);

        crate::startup_trace::mark("  Window::new_window start");
        let window = Window::new_window(
            &get_window_class(),
            "kaku",
            geometry,
            Some(&config),
            Rc::clone(&fontconfig),
            move |event, window| {
                let mut tw = tw_event.borrow_mut();
                if let Err(err) = tw.dispatch_window_event(event, window) {
                    log::error!("dispatch_window_event: {:#}", err);
                }
            },
        )
        .await?;
        crate::startup_trace::mark("  Window::new_window done");
        tw.borrow_mut().window.replace(window.clone());

        {
            let mut myself = tw.borrow_mut();
            myself.load_os_parameters();
        }

        // These don't depend on the window being visible.
        Self::apply_icon(&window)?;

        let config_subscription = config::subscribe_to_config_reload({
            let window = window.clone();
            move || {
                window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                    tw.config_was_reloaded()
                })));
                true
            }
        });
        config::enable_deferred_watchers();

        // Show the window before GPU initialization so it appears immediately,
        // matching pre-0.10 startup feel. The dark-theme titlebar vibrancy
        // flash is already prevented by apply_window_appearance, which pins the
        // NSWindow's NSAppearance to the resolved theme at window-creation time
        // (see apply_window_appearance in window/src/os/macos/window.rs).
        crate::startup_trace::mark("  window.show() start");
        window.show();
        crate::startup_trace::mark("  window.show() done");

        crate::startup_trace::mark("  GPU init start");
        let (gl, webgpu) = match config.front_end {
            FrontEndSelection::WebGpu => match WebGpuState::new(&window, dimensions, &config).await
            {
                Ok(state) => (None, Some(Rc::new(state))),
                Err(err) => {
                    log::error!(
                        "WebGpu initialization failed; falling back to OpenGL. Error: {:#}",
                        err
                    );
                    let gl = window.enable_opengl().await.with_context(|| {
                        "WebGpu initialization failed and OpenGL fallback also failed"
                    })?;
                    (Some(gl), None)
                }
            },
            _ => (Some(window.enable_opengl().await?), None),
        };
        crate::startup_trace::mark("  GPU init done");

        {
            let mut myself = tw.borrow_mut();
            myself.config_subscription.replace(config_subscription);
            if config.use_resize_increments {
                window.set_resize_increments(
                    ResizeIncrementCalculator {
                        x: myself.render_metrics.cell_size.width as u16,
                        y: myself.render_metrics.cell_size.height as u16,
                        padding_left: padding_left,
                        padding_top: padding_top,
                        padding_right: padding_right,
                        padding_bottom: padding_bottom,
                        border: border,
                        tab_bar_height: tab_bar_height,
                    }
                    .into(),
                );
            }

            crate::startup_trace::mark("  TermWindow::created start");
            // The backend init above always yields exactly one of (gl, webgpu).
            // The `if let` ladder collapses to a single assignment + one call.
            if let Some(gl) = gl {
                myself.render_backend = Some(RenderBackend::OpenGl(Rc::clone(&gl)));
                myself.created(RenderContext::Glium(gl))?;
            } else if let Some(webgpu) = webgpu {
                myself.render_backend = Some(RenderBackend::WebGpu(Rc::clone(&webgpu)));
                myself.created(RenderContext::WebGpu(webgpu))?;
            }
            crate::startup_trace::mark("  TermWindow::created done");
            myself.subscribe_to_pane_updates();
            crate::startup_trace::mark("  emit window-config-reloaded start");
            myself.emit_window_event("window-config-reloaded", None);
            crate::startup_trace::mark("  emit window-config-reloaded done");
            myself.emit_status_event();
            crate::startup_trace::mark("  emit_status_event done");
        }

        // The update checker (notification-center init + marker-file IO) is
        // deferred to just after the first paint; see paint_impl.
        front_end().record_known_window(window, mux_window_id);
        crate::startup_trace::mark("TermWindow::new_window EXIT");

        Ok(())
    }

    fn dispatch_window_event(
        &mut self,
        event: WindowEvent,
        window: &Window,
    ) -> anyhow::Result<bool> {
        log::trace!("{event:?}");
        match event {
            WindowEvent::Destroyed => {
                self.window.take();
                self.event_states.clear();
                // Ensure that we cancel any overlays we had running, so
                // that the mux can empty out, otherwise the mux keeps
                // the TermWindow alive via the frontend even though
                // the window is gone and we'll linger forever.
                // <https://github.com/wezterm/wezterm/issues/3522>
                self.clear_all_overlays();
                front_end().forget_known_window(window);
                Ok(false)
            }
            WindowEvent::CloseRequested => {
                self.close_requested(window);
                Ok(true)
            }
            WindowEvent::AppearanceChanged(appearance) => {
                if self.last_handled_appearance == Some(appearance) {
                    return Ok(true);
                }
                self.last_handled_appearance = Some(appearance);

                // Coalesce config reloads across all open windows. One system
                // Light/Dark flip dispatches AppearanceChanged to every
                // window, but config::reload() reloads from disk and fans its
                // result out to all windows via subscribers, so a single
                // reload already refreshes every window. Let only the first
                // window to observe a given appearance run the reload; the
                // rest skip the redundant disk load + subscriber fanout and
                // still repaint from that reload's subscription.
                use std::sync::atomic::{AtomicU8, Ordering};
                static LAST_RELOADED_APPEARANCE: AtomicU8 = AtomicU8::new(0);
                let code = match appearance {
                    Appearance::Light => 1u8,
                    Appearance::Dark => 2,
                    Appearance::LightHighContrast => 3,
                    Appearance::DarkHighContrast => 4,
                };
                if LAST_RELOADED_APPEARANCE.swap(code, Ordering::Relaxed) != code {
                    config::reload();
                }
                Ok(true)
            }
            WindowEvent::PerformKeyAssignment(action) => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    self.perform_key_assignment(&pane, &action)?;
                    window.invalidate();
                }
                Ok(true)
            }
            WindowEvent::FocusChanged(focused) => {
                self.focus_changed(focused, window);
                Ok(true)
            }
            WindowEvent::VisibilityChanged(visible) => {
                self.visibility_changed(visible, window);
                Ok(true)
            }
            WindowEvent::MouseEvent(event) => {
                self.mouse_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::MouseLeave => {
                self.mouse_leave_impl(window);
                Ok(true)
            }
            WindowEvent::Resized {
                dimensions,
                window_state,
                live_resizing,
                screen_changed,
            } => {
                self.resize(
                    dimensions,
                    window_state,
                    window,
                    live_resizing,
                    screen_changed,
                );
                Ok(true)
            }
            WindowEvent::SetInnerSizeCompleted => {
                self.resizes_pending -= 1;
                if self.is_repaint_pending {
                    self.is_repaint_pending = false;
                    if self.webgpu().is_some() {
                        self.do_paint_webgpu()?;
                    } else {
                        self.do_paint(window);
                    }
                }
                self.apply_pending_scale_changes();
                Ok(true)
            }
            WindowEvent::AdviseModifiersLedStatus(modifiers, leds) => {
                self.current_modifier_and_leds = (modifiers, leds);
                self.update_title();
                window.invalidate();
                Ok(true)
            }
            WindowEvent::RawKeyEvent(event) => {
                self.raw_key_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::KeyEvent(event) => {
                self.key_event_impl(event, window);
                Ok(true)
            }
            WindowEvent::AdviseDeadKeyStatus(status) => {
                if self.config.debug_key_events {
                    log::info!("DeadKeyStatus now: {:?}", status);
                } else {
                    log::trace!("DeadKeyStatus now: {:?}", status);
                }
                self.keyboard.dead_key_status = status;
                self.update_title();
                // Ensure that we repaint so that any composing
                // text is updated
                window.invalidate();
                Ok(true)
            }
            WindowEvent::NeedRepaint => {
                if self.resizes_pending > 0 {
                    self.is_repaint_pending = true;
                    Ok(true)
                } else if self.webgpu().is_some() {
                    self.do_paint_webgpu()
                } else {
                    Ok(self.do_paint(window))
                }
            }
            WindowEvent::Notification(item) => {
                if let Ok(notif) = item.downcast::<TermWindowNotif>() {
                    self.dispatch_notif(*notif, window)
                        .context("dispatch_notif")?;
                }
                Ok(true)
            }
            WindowEvent::DroppedString(text) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let result = pane.send_paste(text.as_str());
                self.finish_terminal_input(&pane, result)?;
                Ok(true)
            }
            WindowEvent::DroppedUrl(urls) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let urls = urls
                    .iter()
                    .map(|url| self.config.quote_dropped_files.escape(&url.to_string()))
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                let result = pane.send_paste(urls.as_str());
                self.finish_terminal_input(&pane, result)?;
                Ok(true)
            }
            WindowEvent::DroppedFile(paths) => {
                let pane = match self.get_active_pane_or_overlay() {
                    Some(pane) => pane,
                    None => return Ok(true),
                };
                let paths = paths
                    .iter()
                    .map(|path| {
                        self.config
                            .quote_dropped_files
                            .escape(&path.to_string_lossy())
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                let result = pane.send_paste(&paths);
                self.finish_terminal_input(&pane, result)?;
                Ok(true)
            }
            WindowEvent::DraggedFile(_) => Ok(true),
        }
    }

    fn do_paint(&mut self, window: &Window) -> bool {
        let gl = match self.opengl() {
            Some(gl) => Rc::clone(gl),
            None => return false,
        };

        if gl.is_context_lost() {
            log::error!("opengl context was lost; should reinit");
            window.close();
            front_end().forget_known_window(window);
            return false;
        }

        let mut frame = glium::Frame::new(
            Rc::clone(&gl),
            (
                self.dimensions.pixel_width as u32,
                self.dimensions.pixel_height as u32,
            ),
        );
        if let Err(err) = self.paint_impl(&mut RenderFrame::Glium(&mut frame)) {
            log::error!("paint_impl failed: {:#}", err);
        }
        window.finish_frame(frame).is_ok()
    }

    fn do_paint_webgpu(&mut self) -> anyhow::Result<bool> {
        // WebGpuState::resize takes &self; the enum accessor returns a
        // borrow we can call straight through.
        let dims = self.dimensions;
        self.webgpu().expect("webgpu backend present").resize(dims);
        match self.do_paint_webgpu_impl() {
            Ok(ok) => {
                self.webgpu()
                    .expect("webgpu backend present")
                    .note_acquire_ok();
                Ok(ok)
            }
            Err(err) => {
                match err.downcast_ref::<wgpu::SurfaceError>() {
                    Some(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        // resize() no-ops here because the dimensions did not
                        // change: sleep/wake and display reconfiguration kill
                        // the drawable without a size change (#458), so force
                        // a fresh swapchain before retrying.
                        log::warn!(
                            "wgpu surface lost/outdated, reconfiguring surface and retrying"
                        );
                        let webgpu = self.webgpu().expect("webgpu backend present");
                        webgpu.note_acquire_failure();
                        webgpu.reconfigure_surface();
                        return match self.do_paint_webgpu_impl() {
                            Ok(ok) => {
                                self.webgpu()
                                    .expect("webgpu backend present")
                                    .note_acquire_ok();
                                Ok(ok)
                            }
                            Err(err) => {
                                self.schedule_webgpu_surface_recovery();
                                Err(err)
                            }
                        };
                    }
                    Some(wgpu::SurfaceError::Timeout) => {
                        // Under Fifo present mode this can happen transiently
                        // during rapid resize, but the Metal backend also maps
                        // a nil nextDrawable to Timeout, which is what a
                        // sleep/wake-invalidated layer keeps returning (#458).
                        // Skipping forever would freeze rendering while the
                        // PTY stays alive, so periodically force a fresh
                        // swapchain instead.
                        let failures = self
                            .webgpu()
                            .expect("webgpu backend present")
                            .note_acquire_failure();
                        if failures % CONSECUTIVE_TIMEOUTS_BEFORE_RECONFIGURE == 0 {
                            log::warn!(
                                "wgpu surface timed out {failures} consecutive frames, \
                                 reconfiguring surface"
                            );
                            self.webgpu()
                                .expect("webgpu backend present")
                                .reconfigure_surface();
                        } else {
                            log::debug!("wgpu surface timeout, skipping frame");
                        }
                        self.schedule_webgpu_surface_recovery();
                        return Ok(false);
                    }
                    _ => {}
                }
                Err(err)
            }
        }
    }

    /// Keep repaint attempts flowing while the surface is broken so an idle
    /// window recovers without user input. Capped so a surface that never
    /// recovers does not busy-render at max_fps forever; key presses and PTY
    /// output still trigger paint attempts past the cap.
    fn schedule_webgpu_surface_recovery(&mut self) {
        let failures = self
            .webgpu()
            .map(|webgpu| webgpu.acquire_failure_count())
            .unwrap_or(0);
        if failures > WEBGPU_SURFACE_RECOVERY_SELF_INVALIDATE_LIMIT {
            return;
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn do_paint_webgpu_impl(&mut self) -> anyhow::Result<bool> {
        self.paint_impl(&mut RenderFrame::WebGpu)?;
        Ok(true)
    }

    fn dispatch_notif(&mut self, notif: TermWindowNotif, window: &Window) -> anyhow::Result<()> {
        fn chan_err<T>(e: smol::channel::TrySendError<T>) -> anyhow::Error {
            anyhow::anyhow!("{}", e)
        }

        match notif {
            TermWindowNotif::InvalidateShapeCache => {
                self.shape_generation += 1;
                self.shape_cache.borrow_mut().clear();
                self.box_text_shape_cache.borrow_mut().clear();
                self.invalidate_modal();
                window.invalidate();
            }
            TermWindowNotif::PerformAssignment {
                pane_id,
                assignment,
                tx,
            } => {
                let mux = Mux::get();
                let result = || -> anyhow::Result<()> {
                    // The CopyMode overlay doesn't exist in the mux, but aliases
                    // itself with the overlaid pane's pane_id.
                    // So we do a bit of fancy footwork here to resolve the overlay
                    // and use that if it has the same pane_id, but otherwise fall
                    // back to what we get from the mux.
                    // <https://github.com/wezterm/wezterm/issues/3209>
                    let active_pane = self
                        .get_active_pane_or_overlay()
                        .ok_or_else(|| anyhow!("there is no active pane!?"))?;
                    let pane = if active_pane.pane_id() == pane_id {
                        active_pane
                    } else {
                        mux.get_pane(pane_id)
                            .ok_or_else(|| anyhow!("pane id {} is not valid", pane_id))?
                    };
                    self.perform_key_assignment(&pane, &assignment)
                        .context("perform_key_assignment")?;
                    Ok(())
                }();
                window.invalidate();
                if let Some(tx) = tx {
                    tx.try_send(result).ok();
                }
            }
            TermWindowNotif::SetRightStatus(status) => {
                if status != self.right_status {
                    self.right_status = status;
                    self.update_title_post_status();
                } else {
                    self.schedule_next_status_update();
                }
            }
            TermWindowNotif::SetLeftStatus(status) => {
                if status != self.left_status {
                    self.left_status = status;
                    self.update_title_post_status();
                } else {
                    self.schedule_next_status_update();
                }
            }
            TermWindowNotif::GetDimensions(tx) => {
                tx.try_send((self.dimensions, self.window_state))
                    .map_err(chan_err)
                    .context("send GetDimensions response")?;
            }
            TermWindowNotif::GetEffectiveConfig(tx) => {
                tx.try_send(self.config.clone())
                    .map_err(chan_err)
                    .context("send GetEffectiveConfig response")?;
            }
            TermWindowNotif::FinishWindowEvent { name, again } => {
                self.finish_window_event(&name, again);
            }
            TermWindowNotif::GetConfigOverrides(tx) => {
                tx.try_send(self.config_overrides.clone())
                    .map_err(chan_err)
                    .context("send GetConfigOverrides response")?;
            }
            TermWindowNotif::SetConfigOverrides(value) => {
                if value != self.config_overrides {
                    self.config_overrides = value;
                    // Overrides are often updated by runtime hooks (eg: resize/fullscreen),
                    // so keep this reload silent to avoid noisy toast spam.
                    // Defer the reload to avoid re-entrant borrow of WindowInner.
                    self.schedule_silent_config_reload(window);
                }
            }
            TermWindowNotif::CancelOverlayForPane(pane_id) => {
                self.cancel_overlay_for_pane(pane_id);
            }
            TermWindowNotif::CancelOverlayForTab { tab_id, pane_id } => {
                self.cancel_overlay_for_tab(tab_id, pane_id);
            }
            TermWindowNotif::MuxNotification(n) => match n {
                MuxNotification::Alert {
                    alert: Alert::SetUserVar { name, value },
                    pane_id,
                } => {
                    self.emit_user_var_event(pane_id, name, value);
                }
                MuxNotification::WindowTitleChanged { .. }
                | MuxNotification::Alert {
                    alert:
                        Alert::OutputSinceFocusLost
                        | Alert::CurrentWorkingDirectoryChanged
                        | Alert::WindowTitleChanged(_)
                        | Alert::TabTitleChanged(_)
                        | Alert::IconTitleChanged(_)
                        | Alert::Progress(_),
                    ..
                } => {
                    self.update_title();
                }
                MuxNotification::Alert {
                    alert: Alert::PaletteChanged,
                    pane_id,
                } => {
                    // Shape cache includes color information, so
                    // ensure that we invalidate that as part of
                    // this overall invalidation for the palette
                    self.dispatch_notif(TermWindowNotif::InvalidateShapeCache, window)?;
                    self.mux_pane_output_event(pane_id);
                }
                MuxNotification::Alert {
                    alert: Alert::Bell,
                    pane_id,
                } => {
                    if !self.window_contains_pane(pane_id) {
                        return Ok(());
                    }

                    match self.config.audible_bell {
                        AudibleBell::SystemBeep => {
                            Connection::get().expect("on main thread").beep();
                        }
                        AudibleBell::Disabled => {}
                    }

                    log::trace!("Ding! (this is the bell) in pane {}", pane_id);
                    self.emit_window_event("bell", Some(pane_id));

                    // Check active pane FIRST, before borrowing pane_state.
                    // get_active_pane_or_overlay() also borrows pane_state internally,
                    // so holding a RefMut from pane_state() simultaneously would cause
                    // a RefCell double-borrow panic at runtime.
                    let is_inactive = self
                        .get_active_pane_or_overlay()
                        .map_or(true, |p| p.pane_id() != pane_id);

                    let window_has_focus = self.focused.is_some();
                    let mut per_pane = self.pane_state(pane_id);
                    per_pane.bell_start.replace(Instant::now());
                    // Mark as unread if pane is inactive, OR if window has no focus
                    // (so Dock badge works even for active pane bells)
                    let should_mark_unread =
                        (is_inactive || !window_has_focus) && !per_pane.has_unread_bell;
                    if should_mark_unread {
                        per_pane.has_unread_bell = true;
                    }
                    drop(per_pane);

                    // Update global Dock badge count
                    if should_mark_unread {
                        front_end().adjust_unread_bell_count(1);
                    }

                    // Show macOS system notification when window is not focused
                    if !window_has_focus {
                        let (last_command, reported_program, pane_title, foreground_process) =
                            Mux::get()
                                .get_pane(pane_id)
                                .map(|pane| {
                                    let user_vars = pane.copy_user_vars();
                                    (
                                        user_vars.get("kaku_last_cmd").and_then(|value| {
                                            kaku_gui_lib::inline_ai_control::decode_control_value(
                                                "kaku_last_cmd",
                                                value,
                                            )
                                            .map(str::to_string)
                                        }),
                                        user_vars.get("WEZTERM_PROG").cloned(),
                                        pane.get_title(),
                                        pane.get_foreground_process_name(CachePolicy::AllowStale),
                                    )
                                })
                                .unwrap_or((None, None, String::new(), None));
                        ToastNotification {
                            title: "Kaku".to_string(),
                            message: bell_notification_message(
                                last_command.as_deref(),
                                reported_program.as_deref(),
                                &pane_title,
                                foreground_process.as_deref(),
                            ),
                            url: None,
                            timeout: Some(Duration::from_secs(5)),
                        }
                        .show();
                    }

                    window.invalidate();
                }
                MuxNotification::Alert {
                    alert: Alert::ToastNotification { .. },
                    ..
                } => {}
                MuxNotification::TabAddedToWindow {
                    window_id: _,
                    tab_id,
                } => {
                    let mux = Mux::get();
                    let mut size = self.terminal_size;
                    if let Some(tab) = mux.get_tab(tab_id) {
                        // If we attached to a remote domain and loaded in
                        // a tab async, we need to fixup its size, either
                        // by resizing it or resizes ourselves.
                        // The strategy here is to adjust both by taking
                        // the maximal size in both horizontal and vertical
                        // dimensions and applying that. In practice that
                        // means that a new local client will resize larger
                        // to adjust to the size of an existing client.
                        let tab_size = tab.get_size();
                        size.rows = size.rows.max(tab_size.rows);
                        size.cols = size.cols.max(tab_size.cols);

                        if size.rows != self.terminal_size.rows
                            || size.cols != self.terminal_size.cols
                            || size.pixel_width != self.terminal_size.pixel_width
                            || size.pixel_height != self.terminal_size.pixel_height
                        {
                            self.set_window_size(size, window)?;
                        } else if tab_size.dpi == 0 {
                            log::debug!("fixup dpi in newly added tab");
                            tab.resize(self.terminal_size);
                        }
                    }
                }
                MuxNotification::PaneOutput(pane_id) => {
                    self.mux_pane_output_event(pane_id);
                }
                MuxNotification::WindowInvalidated(_) => {
                    window.invalidate();
                    self.update_title_post_status();
                }
                MuxNotification::WindowRemoved(_window_id) => {
                    // Handled by frontend
                }
                MuxNotification::AssignClipboard { .. } => {
                    // Handled by frontend
                }
                MuxNotification::SaveToDownloads { .. } => {
                    // Handled by frontend
                }
                MuxNotification::PaneFocused(_) => {
                    // Also handled by clientpane
                    self.update_title_post_status();
                }
                MuxNotification::TabResized(_) => {
                    // Also handled by wezterm-client
                    self.resize_overlays();
                    self.update_title_post_status();
                }
                MuxNotification::TabTitleChanged { .. } => {
                    self.update_title_post_status();
                }
                MuxNotification::PaneRemoved(pane_id) => {
                    // Clean up pane state and adjust global bell count if needed
                    if let Some(state) = self.pane_state.borrow_mut().remove(&pane_id) {
                        if state.has_unread_bell {
                            front_end().adjust_unread_bell_count(-1);
                        }
                    }
                    // Closing a pane redistributes the freed space to siblings,
                    // so any overlay (e.g. AI chat) on a sibling needs its size
                    // updated to match the pane's new dimensions.
                    self.resize_overlays();
                    self.update_title_post_status();
                }
                MuxNotification::PaneAdded(_) => {
                    // A new split shrinks the donor pane; if that pane has an
                    // overlay open (e.g. AI chat), the overlay must shrink too.
                    self.resize_overlays();
                    self.update_title_post_status();
                }
                MuxNotification::WorkspaceRenamed { .. }
                | MuxNotification::WindowWorkspaceChanged(_)
                | MuxNotification::ActiveWorkspaceChanged(_)
                | MuxNotification::Empty
                | MuxNotification::WindowCreated(_) => {}
            },
            TermWindowNotif::EmitStatusUpdate => {
                self.status_update_queued = false;
                self.emit_status_event();
            }
            TermWindowNotif::EmitTitleUpdate => {
                self.title_update_queued = false;
                self.update_title_impl();
            }
            TermWindowNotif::GetSelectionForPane { pane_id, tx } => {
                let mux = Mux::get();
                let pane = mux
                    .get_pane(pane_id)
                    .ok_or_else(|| anyhow!("pane id {} is not valid", pane_id))?;

                tx.try_send(self.selection_text(&pane))
                    .map_err(chan_err)
                    .context("send GetSelectionForPane response")?;
            }
            TermWindowNotif::Apply(func) => {
                func(self);
            }
            TermWindowNotif::SwitchToMuxWindow(mux_window_id) => {
                self.mux_window_id = mux_window_id;
                *self.mux_window_id_for_subscriptions.lock().unwrap() = mux_window_id;

                self.clear_all_overlays();
                self.current_highlight.take();
                self.invalidate_fancy_tab_bar();
                self.invalidate_modal();

                let mux = Mux::get();
                if let Some(window) = mux.get_window(self.mux_window_id) {
                    for tab in window.iter() {
                        tab.resize(self.terminal_size);
                    }
                };
                self.update_title();
                window.invalidate();
            }
            TermWindowNotif::SetInnerSize { width, height } => {
                self.set_inner_size(window, width, height);
            }
        }

        Ok(())
    }

    fn set_inner_size(&mut self, window: &Window, width: usize, height: usize) {
        self.resizes_pending += 1;
        window.set_inner_size(width, height);
    }

    /// Take care to remove our panes from the mux, otherwise
    /// we can leave the mux with no windows but some panes
    /// and it won't believe that we are empty.
    fn clear_all_overlays(&mut self) {
        let overlay_panes_to_cancel = self
            .pane_state
            .borrow()
            .values()
            .filter_map(|state| state.overlay.as_ref().map(|overlay| overlay.pane.pane_id()))
            .collect::<Vec<_>>();

        for pane_id in overlay_panes_to_cancel {
            self.cancel_overlay_for_pane(pane_id);
        }

        let tab_overlays_to_cancel = self
            .tab_state
            .borrow()
            .iter()
            .filter_map(|(tab_id, state)| state.overlay.as_ref().map(|_| *tab_id))
            .collect::<Vec<_>>();

        for tab_id in tab_overlays_to_cancel {
            self.cancel_overlay_for_tab(tab_id, None);
        }

        // Adjust global bell count before clearing pane state
        let unread_count = self
            .pane_state
            .borrow()
            .values()
            .filter(|s| s.has_unread_bell)
            .count() as isize;
        if unread_count > 0 {
            front_end().adjust_unread_bell_count(-unread_count);
        }

        self.pane_state.borrow_mut().clear();
        self.tab_state.borrow_mut().clear();
    }

    fn apply_icon(_window: &Window) -> anyhow::Result<()> {
        // On macOS the app bundle provides the icon via Info.plist;
        // set_icon() is a no-op there, so skip the PNG decode entirely.
        #[cfg(not(target_os = "macos"))]
        {
            let image = image::load_from_memory(ICON_DATA)?.into_rgba8();
            let (width, height) = image.dimensions();
            _window.set_icon(Image::with_rgba32(
                width as usize,
                height as usize,
                width as usize * 4,
                image.as_raw(),
            ));
        }
        Ok(())
    }

    fn schedule_status_update(&mut self) {
        if self.status_update_queued {
            return;
        }
        if let Some(window) = self.window.as_ref().cloned() {
            self.status_update_queued = true;
            promise::spawn::spawn_into_main_thread(async move {
                window.notify(TermWindowNotif::EmitStatusUpdate);
            })
            .detach();
        }
    }

    fn schedule_title_update(&mut self) {
        if self.title_update_queued {
            return;
        }
        if let Some(window) = self.window.as_ref().cloned() {
            self.title_update_queued = true;
            promise::spawn::spawn_into_main_thread(async move {
                window.notify(TermWindowNotif::EmitTitleUpdate);
            })
            .detach();
        }
    }

    fn is_pane_visible(&mut self, pane_id: PaneId) -> bool {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return false,
        };

        let tab_id = tab.tab_id();
        if let Some(tab_overlay) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            return tab_overlay.pane_id() == pane_id;
        }

        if tab.contains_pane(pane_id) {
            return true;
        }

        // A per-pane overlay (e.g. the AI chat) renders in place of its
        // underlying pane and lives in pane_state, not the tab's pane tree. Its
        // PaneOutput would otherwise be judged invisible and never trigger a
        // window repaint, so the overlay would only update on input events.
        self.pane_state.borrow().values().any(|state| {
            state
                .overlay
                .as_ref()
                .is_some_and(|o| o.pane.pane_id() == pane_id)
        })
    }

    fn mux_pane_output_event(&mut self, pane_id: PaneId) {
        metrics::histogram!("mux.pane_output_event.rate").record(1.);
        if self.is_pane_visible(pane_id) {
            if let Some(ref win) = self.window {
                // Coalesce rapid output events: only invalidate once per 8ms
                // (well above the 16ms render interval at 60fps) to avoid
                // waking the runloop more often than the display can consume.
                let now = Instant::now();
                let should_invalidate = self
                    .last_pane_output_invalidate
                    .map(|t| now.duration_since(t) >= Duration::from_millis(8))
                    .unwrap_or(true);
                if should_invalidate {
                    self.last_pane_output_invalidate = Some(now);
                    self.pending_output_invalidate = false;
                    win.invalidate();
                } else if !self.pending_output_invalidate {
                    // First suppressed event in this coalesce window: schedule
                    // a follow-up invalidate so the final output state is
                    // always rendered even if no further events arrive.
                    self.pending_output_invalidate = true;
                    let win_clone = win.clone();
                    promise::spawn::spawn_into_main_thread(async move {
                        win_clone.invalidate();
                    })
                    .detach();
                }
            }
        }
    }

    fn mux_pane_output_event_callback(
        n: MuxNotification,
        window: &Window,
        mux_window_id: MuxWindowId,
        dead: &Arc<AtomicBool>,
    ) -> bool {
        if dead.load(Ordering::Relaxed) {
            return false;
        }

        // Most filtering is done in subscribe_to_pane_updates before spawning.
        // Here we only do final validity checks that require main thread context.
        match &n {
            MuxNotification::PaneFocused(_)
            | MuxNotification::PaneRemoved(_)
            | MuxNotification::PaneOutput(_)
            | MuxNotification::Alert { .. } => {
                // Verify window still exists
                let mux = Mux::get();
                if mux.get_window(mux_window_id).is_none() {
                    log::debug!(
                        "mux_window_id={} not found, cancel subscription",
                        mux_window_id
                    );
                    return false;
                }
            }
            MuxNotification::PaneAdded(_) => {
                let mux = Mux::get();
                return mux.get_window(mux_window_id).is_some();
            }
            // All other notifications are pre-filtered in subscribe_to_pane_updates
            _ => {}
        }

        window.notify(TermWindowNotif::MuxNotification(n));
        true
    }

    fn subscribe_to_pane_updates(&self) {
        let window = self.window.clone().expect("window to be valid on startup");
        let mux_window_id = Arc::clone(&self.mux_window_id_for_subscriptions);
        let mux = Mux::get();
        let dead = Arc::new(AtomicBool::new(false));
        mux.subscribe(move |n| {
            if dead.load(Ordering::Relaxed) {
                return false;
            }
            let mux_window_id = *mux_window_id.lock().unwrap();

            // Pre-filter notifications to avoid unnecessary main thread task spawning.
            // This reduces O(windows × notifications) fan-out in multi-window scenarios.
            let dominated_mux = Mux::try_get();
            let can_resolve_pane_ownership = dominated_mux
                .as_ref()
                .map(|mux| !mux.is_main_thread())
                .unwrap_or(false);
            match &n {
                // Notifications with explicit window_id: skip if not for this window
                MuxNotification::TabAddedToWindow { window_id, .. }
                | MuxNotification::WindowTitleChanged { window_id, .. }
                | MuxNotification::WindowInvalidated(window_id) => {
                    if *window_id != mux_window_id {
                        return true;
                    }
                }
                MuxNotification::WindowRemoved(window_id) => {
                    if *window_id != mux_window_id {
                        return true;
                    }
                    dead.store(true, Ordering::Relaxed);
                    return false;
                }
                // Notifications with pane_id: check pane ownership
                MuxNotification::PaneOutput(pane_id)
                | MuxNotification::PaneFocused(pane_id)
                | MuxNotification::PaneRemoved(pane_id)
                | MuxNotification::PaneAdded(pane_id) => {
                    if can_resolve_pane_ownership {
                        let mux = dominated_mux.as_ref().expect("checked above");
                        // If we can resolve the pane and it belongs to a different window, skip
                        if let Some((_, window_id, _)) = mux.resolve_pane_id(*pane_id) {
                            if window_id != mux_window_id {
                                return true;
                            }
                        }
                        // If pane not found (e.g. overlay), fall through to spawn
                        //
                        // Avoid resolving on the main thread: focus changes emit PaneFocused
                        // while still holding the tab mutex, so resolving ownership here would
                        // re-enter Tab::iter_panes_ignoring_zoom() and self-deadlock the UI.
                    }
                }
                // Alert notifications with pane_id
                MuxNotification::Alert { pane_id, .. } => {
                    if can_resolve_pane_ownership {
                        let mux = dominated_mux.as_ref().expect("checked above");
                        if let Some((_, window_id, _)) = mux.resolve_pane_id(*pane_id) {
                            if window_id != mux_window_id {
                                return true;
                            }
                        }
                    }
                }
                // Tab notifications: check tab ownership
                MuxNotification::TabResized(tab_id)
                | MuxNotification::TabTitleChanged { tab_id, .. } => {
                    if let Some(ref mux) = dominated_mux {
                        if let Some(window_id) = mux.window_containing_tab(*tab_id) {
                            if window_id != mux_window_id {
                                return true;
                            }
                        }
                    }
                }
                // Global notifications not relevant to individual windows
                MuxNotification::AssignClipboard { .. }
                | MuxNotification::SaveToDownloads { .. }
                | MuxNotification::WindowCreated(_)
                | MuxNotification::ActiveWorkspaceChanged(_)
                | MuxNotification::WorkspaceRenamed { .. }
                | MuxNotification::Empty
                | MuxNotification::WindowWorkspaceChanged(_) => {
                    return true;
                }
            }

            let window = window.clone();
            let dead = dead.clone();
            let n = n.clone();
            promise::spawn::spawn_into_main_thread(async move {
                Self::mux_pane_output_event_callback(n, &window, mux_window_id, &dead)
            })
            .detach();
            true
        });
    }

    fn emit_status_event(&mut self) {
        self.emit_window_event("update-right-status", None);
        self.emit_window_event("update-status", None);
    }

    fn schedule_window_event(&mut self, name: &str, pane_id: Option<PaneId>) {
        let window = GuiWin::new(self);
        let pane = match pane_id {
            Some(pane_id) => Mux::get().get_pane(pane_id),
            None => None,
        };
        let pane = match pane {
            Some(pane) => pane,
            None => match self.get_active_pane_or_overlay() {
                Some(pane) => pane,
                None => return,
            },
        };
        let pane = MuxPane(pane.pane_id());
        let name = name.to_string();

        async fn do_event(
            lua: Option<Rc<mlua::Lua>>,
            name: String,
            window: GuiWin,
            pane: MuxPane,
        ) -> anyhow::Result<()> {
            let again = if let Some(lua) = lua {
                let args = lua.pack_multi((window.clone(), pane))?;

                if let Err(err) = config::lua::emit_event(&lua, (name.clone(), args)).await {
                    log::error!("while processing {} event: {:#}", name, err);
                }
                true
            } else {
                false
            };

            window
                .window
                .notify(TermWindowNotif::FinishWindowEvent { name, again });

            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            do_event(lua, name, window, pane)
        }))
        .detach();
    }

    /// Called as part of finishing up a callout to lua.
    /// If again==false it means that there isn't a lua config
    /// to execute against, so we should just mark as done.
    /// Otherwise, if there is a queued item, schedule it now.
    fn finish_window_event(&mut self, name: &str, again: bool) {
        let state = self
            .event_states
            .entry(name.to_string())
            .or_insert(EventState::None);
        if again {
            match state {
                EventState::InProgress => {
                    *state = EventState::None;
                }
                EventState::InProgressWithQueued(pane) => {
                    let pane = *pane;
                    *state = EventState::InProgress;
                    self.schedule_window_event(name, pane);
                }
                EventState::None => {}
            }
        } else {
            *state = EventState::None;
        }
    }

    pub fn emit_window_event(&mut self, name: &str, pane_id: Option<PaneId>) {
        if self.get_active_pane_or_overlay().is_none() || self.window.is_none() {
            return;
        }

        let state = self
            .event_states
            .entry(name.to_string())
            .or_insert(EventState::None);
        match state {
            EventState::InProgress => {
                // Flag that we want to run again when the currently
                // executing event calls finish_window_event().
                *state = EventState::InProgressWithQueued(pane_id);
                return;
            }
            EventState::InProgressWithQueued(other_pane) => {
                // We've already got one copy executing and another
                // pending dispatch, so don't queue another.
                if pane_id != *other_pane {
                    log::warn!(
                        "Cannot queue {} event for pane {:?}, as \
                         there is already an event queued for pane {:?} \
                         in the same window",
                        name,
                        pane_id,
                        other_pane
                    );
                }
                return;
            }
            EventState::None => {
                // Nothing pending, so schedule a call now
                *state = EventState::InProgress;
                self.schedule_window_event(name, pane_id);
            }
        }
    }
}

impl TermWindow {
    /// Computes effective vertical padding for the current window state.
    pub fn effective_vertical_padding(&self) -> (usize, usize) {
        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height().unwrap_or(0.) as usize
        } else {
            0
        };
        resize::effective_vertical_padding(
            &self.config,
            DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.terminal_size.pixel_height as f32,
                pixel_cell: self.render_metrics.cell_size.height as f32,
            },
            self.show_tab_bar,
            self.config.tab_bar_at_bottom,
            tab_bar_height,
            self.layout_uses_edge_to_edge_padding(),
        )
    }

    /// Decide whether the tab bar should be visible based on tab count,
    /// fullscreen state, and config.
    fn should_show_tab_bar(&self, num_tabs: usize) -> bool {
        let is_full_screen = self.layout_is_effective_fullscreen();
        if is_full_screen {
            // Always show tab bar in fullscreen mode to display the right status (time)
            self.config.enable_tab_bar
        } else if num_tabs == 1 {
            self.config.enable_tab_bar && !self.config.hide_tab_bar_if_only_one_tab
        } else {
            self.config.enable_tab_bar
        }
    }

    fn sync_tab_bar_visibility_for_window_state(&mut self, _reason: &'static str) -> bool {
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            return false;
        };

        let show_tab_bar = self.should_show_tab_bar(window.len());
        if show_tab_bar == self.show_tab_bar {
            return false;
        }

        self.show_tab_bar = show_tab_bar;
        if show_tab_bar && self.config.use_fancy_tab_bar {
            let _ = self.fonts.title_font();
        }
        self.fancy_tab_bar.take();
        self.invalidate_fancy_tab_bar();
        true
    }

    fn palette(&mut self) -> &ColorPalette {
        if self.palette.is_none() {
            self.palette
                .replace(config::TermConfig::new().color_palette());
        }
        self.palette.as_ref().unwrap()
    }

    pub fn config_was_reloaded(&mut self) {
        // Skip config reload during live resizing to avoid performance issues
        // when dragging the window. The reload will be processed after resize completes.
        if self.live_resizing {
            log::trace!("Skipping config reload during live resizing");
            self.pending_config_reload_after_resize = true;
            return;
        }

        self.config_was_reloaded_impl();
    }

    fn config_was_reloaded_silently(&mut self) {
        if self.live_resizing {
            self.pending_config_reload_after_resize = true;
            return;
        }
        self.config_was_reloaded_impl();
    }

    fn config_was_reloaded_impl(&mut self) {
        log::debug!(
            "config was reloaded, overrides: {:?}",
            self.config_overrides
        );
        self.keyboard.key_table_state.clear_stack();
        self.connection_name = Connection::get().map_or_else(
            || {
                log::warn!(
                    "window connection is not initialized during config reload; keeping placeholder"
                );
                "uninitialized".to_string()
            },
            |conn| conn.name(),
        );
        let config = if matches!(&self.config_overrides, Value::Null)
            || matches!(&self.config_overrides, Value::Object(obj) if obj.is_empty())
        {
            configuration()
        } else {
            match config::overridden_config(&self.config_overrides) {
                Ok(config) => config,
                Err(err) => {
                    log::error!(
                        "Failed to apply config overrides to window: {:#}: {:?}",
                        err,
                        self.config_overrides
                    );
                    configuration()
                }
            }
        };
        self.config = config.clone();
        self.palette.take();
        let chat_colors = ai_chat::chat_palette(self.palette());
        self.ai_chat_overlay_panes
            .retain(|_, sender| sender.send(chat_colors.clone()).is_ok());

        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return,
        };
        self.show_tab_bar = self.should_show_tab_bar(window.len());
        *self.cursor_blink_state.borrow_mut() = ColorEase::new(
            config.cursor_blink_rate,
            config.cursor_blink_ease_in,
            config.cursor_blink_rate,
            config.cursor_blink_ease_out,
            None,
        );
        *self.blink_state.borrow_mut() = ColorEase::new(
            config.text_blink_rate,
            config.text_blink_ease_in,
            config.text_blink_rate,
            config.text_blink_ease_out,
            None,
        );
        *self.rapid_blink_state.borrow_mut() = ColorEase::new(
            config.text_blink_rate_rapid,
            config.text_blink_rapid_ease_in,
            config.text_blink_rate_rapid,
            config.text_blink_rapid_ease_out,
            None,
        );

        self.show_scroll_bar = config.enable_scroll_bar;
        self.scrollbar = ScrollbarState::default();
        self.shape_generation += 1;
        {
            let mut shape_cache = self.shape_cache.borrow_mut();
            shape_cache.update_config(&config);
            shape_cache.clear();
        }
        self.line_state_cache.borrow_mut().update_config(&config);
        self.line_quad_cache.borrow_mut().update_config(&config);
        self.line_to_ele_shape_cache
            .borrow_mut()
            .update_config(&config);
        self.box_text_shape_cache.borrow_mut().clear();
        self.fancy_tab_bar.take();
        self.invalidate_fancy_tab_bar();
        self.invalidate_modal();
        self.keyboard.input_map = InputMap::new(&config);
        self.keyboard.leader_is_down = None;
        self.render_state.as_mut().map(|rs| rs.config_changed());
        let dimensions = self.dimensions;

        if let Err(err) = self.fonts.config_changed(&config) {
            log::error!("Failed to load font configuration: {:#}", err);
        }

        // Recreate texture atlas to ensure subpixel AA and font rendering changes
        // correctly flush out the old cached glyphs when theme changes.
        if let Err(err) = self.recreate_texture_atlas(None) {
            log::error!(
                "recreate_texture_atlas after config reload failed: {:#}",
                err
            );
        }

        if let Some(window) = mux.get_window(self.mux_window_id) {
            let term_config: Arc<dyn TerminalConfiguration> =
                Arc::new(TermConfig::with_config(config.clone()));
            for tab in window.iter() {
                for pane in tab.iter_panes_ignoring_zoom() {
                    pane.pane.set_config(Arc::clone(&term_config));
                }
            }
            for state in self.pane_state.borrow().values() {
                if let Some(overlay) = &state.overlay {
                    overlay.pane.set_config(Arc::clone(&term_config));
                }
            }
            for state in self.tab_state.borrow().values() {
                if let Some(overlay) = &state.overlay {
                    overlay.pane.set_config(Arc::clone(&term_config));
                }
            }
            if let Some(active_pane) = self.get_active_pane_or_overlay() {
                active_pane.refresh_focus(self.focused.is_some());
            }
        }

        if let Some(window) = self.window.as_ref().map(|w| w.clone()) {
            self.load_os_parameters();
            self.sync_tab_bar_visibility_for_window_state("config_reload");
            self.apply_scale_change(&dimensions, self.fonts.get_font_scale());
            self.apply_dimensions(&dimensions, None, &window, true);
            self.update_scrollbar();
            // Rebuild tab bar state synchronously so tab bar colors match the
            // new palette in the same invalidate cycle as pane colors.
            self.update_title_impl();
            window.config_did_change(&config);
            self.quad_generation += 1;
            window.invalidate();

            // Schedule a deferred layout to ensure macOS window frames and native titlebars
            // correctly reposition themselves when configs that shift content (like tab_bar_at_bottom)
            // or window_padding are toggled.
            self.schedule_deferred_layout_relayout(&window);
        }

        // Do this after we've potentially adjusted scaling based on config/padding
        // and window size
        self.window_background = reload_background_image(
            &config,
            &self.window_background,
            &self.dimensions,
            &self.render_metrics,
        );

        self.invalidate_modal();
        self.emit_window_event("window-config-reloaded", None);

        // Sync Dock badge in case bell_dock_badge was toggled.
        // Passing 0 re-evaluates badge state without changing the count.
        front_end().sync_unread_bell_badge();
    }

    fn invalidate_modal(&mut self) {
        if let Some(modal) = self.get_modal() {
            modal.reconfigure(self);
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    pub fn cancel_modal(&self) {
        self.modal.borrow_mut().take();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn set_modal(&self, modal: Rc<dyn Modal>) {
        self.modal.borrow_mut().replace(modal);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn get_modal(&self) -> Option<Rc<dyn Modal>> {
        self.modal.borrow().as_ref().map(|m| Rc::clone(&m))
    }

    fn update_scrollbar(&mut self) {
        if !self.show_scroll_bar {
            return;
        }

        let tab = match self.get_active_pane_or_overlay() {
            Some(tab) => tab,
            None => return,
        };

        let render_dims = tab.get_dimensions();
        if render_dims == self.scrollbar.last_scroll_info {
            return;
        }

        self.scrollbar.last_scroll_info = render_dims;

        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn scrollbar_track_for_pane(&self, pane: &Arc<dyn Pane>) -> Option<ScrollbarTrack> {
        if !self.show_scroll_bar {
            return None;
        }

        let dims = pane.get_dimensions();
        if dims.scrollback_rows <= dims.viewport_rows {
            return None;
        }

        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height().unwrap_or(0.)
        } else {
            0.
        };
        let (top_bar_height, bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };
        let border = self.get_os_border();

        Some(scrollbar_track(
            self.dimensions.pixel_width,
            self.dimensions.pixel_height,
            (top_bar_height as usize).saturating_add(border.top.get()),
            border.bottom.get() + bottom_bar_height as usize,
            self.render_metrics.cell_size.height as usize,
            self.effective_right_padding(&self.config),
            border.right.get(),
        ))
    }

    fn update_scrollbar_hovering(&mut self, pane: &Arc<dyn Pane>, context: &dyn WindowOps) {
        let hovering = self.current_mouse_event.as_ref().is_some_and(|event| {
            matches!(self.current_mouse_capture, None | Some(MouseCapture::UI))
                && self.scrollbar_track_for_pane(pane).is_some_and(|track| {
                    scrollbar_hover_hit(
                        track.x,
                        track.top,
                        track.width,
                        track.height,
                        event.coords.x,
                        event.coords.y,
                    )
                })
        });

        if hovering != self.scrollbar.hovering {
            self.scrollbar.hovering = hovering;
            context.invalidate();
        }
    }

    fn reveal_scrollbar(&mut self) {
        self.scrollbar.visible_until = Some(Instant::now() + Duration::from_millis(900));
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn scrollbar_is_dragging(&self) -> bool {
        matches!(
            self.dragging,
            Some((
                UIItem {
                    item_type: UIItemType::AboveScrollThumb
                        | UIItemType::ScrollThumb
                        | UIItemType::BelowScrollThumb,
                    ..
                },
                _
            ))
        )
    }

    fn scrollbar_thumb_alpha(
        &self,
        pane: &Arc<dyn Pane>,
        track_x: usize,
        track_top: usize,
        track_width: usize,
        track_height: usize,
    ) -> Option<f32> {
        if !self.show_scroll_bar {
            return None;
        }

        let dims = pane.get_dimensions();
        if dims.scrollback_rows <= dims.viewport_rows {
            return None;
        }

        let is_scrolled = self.effective_viewport(pane).is_some();
        let is_dragging = self.scrollbar_is_dragging();
        let is_hovering = matches!(
            &self.current_mouse_event,
            Some(event)
                if scrollbar_hover_hit(
                    track_x,
                    track_top,
                    track_width,
                    track_height,
                    event.coords.x,
                    event.coords.y,
                ) && matches!(self.current_mouse_capture, None | Some(MouseCapture::UI))
        );
        let is_light = is_light_color(&pane.palette().background);
        let mut alpha: f32 = if is_scrolled {
            if is_light {
                0.62
            } else {
                0.54
            }
        } else {
            0.0
        };

        if is_hovering {
            alpha = alpha.max(if is_light { 0.78 } else { 0.70 });
        }
        if is_dragging {
            alpha = alpha.max(if is_light { 0.88 } else { 0.80 });
        }

        let now = Instant::now();
        if let Some(deadline) = self
            .scrollbar
            .visible_until
            .filter(|deadline| *deadline > now)
        {
            let progress = (deadline - now).as_secs_f32() / 0.9;
            alpha = alpha.max(if is_light { 0.34 } else { 0.30 } * progress.max(0.0));
            if !is_scrolled && !is_hovering && !is_dragging {
                let next = now + Duration::from_millis(16);
                let mut anim = self.has_animation.borrow_mut();
                match *anim {
                    Some(existing) if existing <= next => {}
                    _ => *anim = Some(next),
                }
            }
        }

        if alpha > 0.0 {
            Some(alpha)
        } else {
            None
        }
    }

    /// Called by various bits of code to update the title bar.
    /// Let's also trigger the status event so that it can choose
    /// to update the right-status.
    fn update_title(&mut self) {
        self.schedule_status_update();
        self.schedule_title_update();
    }

    fn window_contains_pane(&mut self, pane_id: PaneId) -> bool {
        let mux = Mux::get();

        let (_domain, window_id, _tab_id) = match mux.resolve_pane_id(pane_id) {
            Some(tuple) => tuple,
            None => return false,
        };

        return window_id == self.mux_window_id;
    }

    fn emit_user_var_event(&mut self, pane_id: PaneId, name: String, value: String) {
        let window_contains_pane = self.window_contains_pane(pane_id);

        // Config TUI signals that config file was just saved; reload immediately.
        // Only the window containing the signaling pane triggers reload to avoid
        // duplicate reloads when multiple windows are open.
        // Note: config::reload() notifies subscribers, and each window reloads from
        // that subscription path. We intentionally avoid calling
        // config_was_reloaded_impl() directly here so this event only triggers one
        // per-window reload, and we also avoid predicting the next generation value:
        // failed reloads do not advance config generation.
        if Self::should_reload_config_for_user_var(&name, window_contains_pane) {
            config::reload();
            return;
        }

        if !window_contains_pane {
            return;
        }

        let value = match kaku_gui_lib::inline_ai_control::decode_control_value(&name, &value) {
            Some(value) => value.to_string(),
            None => {
                log::warn!("Ignored unauthenticated Kaku control user var {name}");
                return;
            }
        };

        // `k` CLI running inside a Kaku pane signals us to open the AI chat overlay.
        if name == "kaku_open_ai_chat" {
            if let Some(win) = self.window.clone() {
                win.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    let mux = Mux::get();
                    if let Some(pane) = mux.get_pane(pane_id) {
                        let _ = term_window.perform_key_assignment(
                            &pane,
                            &KeyAssignment::EmitEvent("kaku-ai-chat".to_string()),
                        );
                    }
                })));
            }
            return;
        }

        let mux = Mux::get();
        let window = GuiWin::new(self);
        let pane = match mux.get_pane(pane_id) {
            Some(pane) => mux_lua::MuxPane(pane.pane_id()),
            None => return,
        };

        async fn do_event(
            lua: Option<Rc<mlua::Lua>>,
            name: String,
            value: String,
            window: GuiWin,
            pane: MuxPane,
        ) -> anyhow::Result<()> {
            if let Some(lua) = lua {
                let args = lua.pack_multi((window.clone(), pane, name, value))?;
                if let Err(err) =
                    config::lua::emit_event(&lua, ("user-var-changed".to_string(), args)).await
                {
                    log::error!("while processing user-var-changed event: {:#}", err);
                }
            }

            window
                .window
                .notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                    term_window.update_title();
                })));

            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            do_event(lua, name, value, window, pane)
        }))
        .detach();
    }

    /// Called by window:set_right_status after the status has
    /// been updated; let's update the bar
    pub fn update_title_post_status(&mut self) {
        self.schedule_title_update();
    }

    fn update_title_impl(&mut self) {
        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return,
        };
        let tabs = self.get_tab_information();
        let panes = self.get_pane_information();
        let active_tab = tabs.iter().find(|t| t.is_active).cloned();
        let active_pane = panes.iter().find(|p| p.is_active).cloned();

        let border = self.get_os_border();
        let tab_bar_height = self.tab_bar_pixel_height().unwrap_or(0.);
        let tab_bar_y = if self.config.tab_bar_at_bottom {
            ((self.dimensions.pixel_height as f32) - (tab_bar_height + border.bottom.get() as f32))
                .max(0.)
        } else {
            border.top.get() as f32
        };

        let tab_bar_height = self.tab_bar_pixel_height().unwrap_or(0.);

        let hovering_in_tab_bar = match &self.current_mouse_event {
            Some(event) => {
                let mouse_y = event.coords.y as f32;
                mouse_y >= tab_bar_y as f32 && mouse_y < tab_bar_y as f32 + tab_bar_height
            }
            None => false,
        };

        let new_tab_bar = TabBarState::new(
            self.dimensions.pixel_width / self.render_metrics.cell_size.width as usize,
            if hovering_in_tab_bar {
                Some(self.last_mouse_coords.0)
            } else {
                None
            },
            &tabs,
            &panes,
            self.layout_is_effective_fullscreen(),
            self.config.resolved_palette.tab_bar.as_ref(),
            &self.config,
            &self.left_status,
            &self.right_status,
        );
        if new_tab_bar != self.tab_bar {
            self.tab_bar = new_tab_bar;
            self.invalidate_fancy_tab_bar();
            self.invalidate_modal();
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }

        let num_tabs = window.len();
        if num_tabs == 0 {
            return;
        }
        drop(window);

        let title = match config::run_immediate_with_lua_config(|lua| {
            if let Some(lua) = lua {
                let tabs = lua.create_sequence_from(tabs.clone().into_iter())?;
                let panes = lua.create_sequence_from(panes.clone().into_iter())?;

                let v = config::lua::emit_sync_callback(
                    &*lua,
                    (
                        "format-window-title".to_string(),
                        (
                            active_tab.clone(),
                            active_pane.clone(),
                            tabs,
                            panes,
                            (*self.config).clone(),
                        ),
                    ),
                )?;
                match &v {
                    mlua::Value::Nil => Ok(None),
                    _ => Ok(Some(String::from_lua(v, &*lua)?)),
                }
            } else {
                Ok(None)
            }
        }) {
            Ok(s) => s,
            Err(err) => {
                log::warn!("format-window-title: {}", err);
                None
            }
        };

        let title = match title {
            Some(title) => title,
            None => {
                if let (Some(pos), Some(tab)) = (active_pane, active_tab) {
                    if num_tabs == 1 {
                        format!("{}{}", if pos.is_zoomed { "[Z] " } else { "" }, pos.title)
                    } else {
                        format!(
                            "{}[{}/{}] {}",
                            if pos.is_zoomed { "[Z] " } else { "" },
                            tab.tab_index + 1,
                            num_tabs,
                            pos.title
                        )
                    }
                } else {
                    "".to_string()
                }
            }
        };

        let title = match self.input_broadcast_title_prefix() {
            Some(prefix) if title.is_empty() => prefix.to_string(),
            Some(prefix) => format!("{prefix} {title}"),
            None => title,
        };

        if let Some(window) = self.window.as_ref().cloned() {
            if title != self.last_window_title {
                self.last_window_title = title.clone();
                window.set_title(&title);
            }

            // If the number of tabs changed and caused the tab bar to
            // hide/show, then we'll need to resize things. We only update
            // what's needed for tab bar visibility to avoid the stutter
            // caused by a full config_was_reloaded() call.
            if self.sync_tab_bar_visibility_for_window_state("update_title:sync_tab_bar") {
                let dimensions = self.dimensions;
                self.apply_dimensions(&dimensions, None, &window, true);
                window.invalidate();
            }
        }
        self.schedule_next_status_update();
    }

    fn schedule_next_status_update(&mut self) {
        if let Some(window) = self.window.as_ref() {
            let now = Instant::now();
            if self.last_status_call <= now {
                let interval = Duration::from_millis(self.config.status_update_interval);
                let target = now + interval;
                self.last_status_call = target;

                let window = window.clone();
                promise::spawn::spawn(async move {
                    Timer::at(target).await;
                    window.notify(TermWindowNotif::EmitStatusUpdate);
                })
                .detach();
            }
        }
    }

    fn update_text_cursor(&mut self, pos: &PositionedPane) {
        if let Some(win) = self.window.as_ref() {
            let cursor = pos.pane.get_cursor_position();
            let top = pos.pane.get_dimensions().physical_top;
            let tab_bar_height = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                self.tab_bar_pixel_height().unwrap()
            } else {
                0.0
            };
            let (padding_left, padding_top) = self.padding_left_top();

            let r = Rect::new(
                Point::new(
                    (((cursor.x + pos.left) as isize).max(0) * self.render_metrics.cell_size.width)
                        .add(padding_left as isize),
                    ((cursor.y + pos.top as isize - top).max(0)
                        * self.render_metrics.cell_size.height)
                        .add(tab_bar_height as isize)
                        .add(padding_top as isize),
                ),
                self.render_metrics.cell_size,
            );
            win.set_text_cursor_position(r);
        }
    }

    fn activate_window(&mut self, window_idx: usize) -> anyhow::Result<()> {
        let windows = front_end().gui_windows();
        if let Some(win) = windows.get(window_idx) {
            win.window.focus();
        }
        Ok(())
    }

    fn activate_window_relative(&mut self, delta: isize, wrap: bool) -> anyhow::Result<()> {
        let windows = front_end().gui_windows();
        let my_idx = windows
            .iter()
            .position(|w| Some(&w.window) == self.window.as_ref())
            .ok_or_else(|| anyhow!("I'm not in the window list!?"))?;

        let idx = my_idx as isize + delta;

        let idx = if wrap {
            let idx = if idx < 0 {
                windows.len() as isize + idx
            } else {
                idx
            };
            idx as usize % windows.len()
        } else {
            if idx < 0 {
                0
            } else if idx >= windows.len() as isize {
                windows.len().saturating_sub(1)
            } else {
                idx as usize
            }
        };

        if let Some(win) = windows.get(idx) {
            win.window.focus();
        }

        Ok(())
    }

    fn center_current_window(&self) {
        if self
            .window_state
            .intersects(WindowState::FULL_SCREEN | WindowState::MAXIMIZED | WindowState::HIDDEN)
        {
            return;
        }

        // Centering is computed natively on the window's own screen.
        // Screen rects from Connection::screens() and the window's pixel
        // dimensions use different scales on mixed-DPI setups, so doing the
        // math here misplaced the window on external displays.
        if let Some(window) = self.window.as_ref() {
            window.center();
        }
    }

    fn activate_tab(&mut self, tab_idx: isize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let mut window = mux
            .get_window_mut(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        // This logic is coupled with the CliSubCommand::ActivateTab
        // logic in wezterm/src/main.rs. If you update this, update that!
        let max = window.len();

        let tab_idx = if tab_idx < 0 {
            max.saturating_sub(tab_idx.abs() as usize)
        } else {
            tab_idx as usize
        };

        if tab_idx < max {
            window.save_and_then_set_active(tab_idx);

            drop(window);

            if let Some(tab) = self.get_active_pane_or_overlay() {
                tab.focus_changed(true);
            }

            self.update_title();
            self.update_scrollbar();
        }
        Ok(())
    }

    fn activate_tab_relative(&mut self, delta: isize, wrap: bool) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        // This logic is coupled with the CliSubCommand::ActivateTab
        // logic in wezterm/src/main.rs. If you update this, update that!
        let active = window.get_active_idx() as isize;
        let tab = active + delta;
        let tab = if wrap {
            let tab = if tab < 0 { max as isize + tab } else { tab };
            (tab as usize % max) as isize
        } else {
            if tab < 0 {
                0
            } else if tab >= max as isize {
                max as isize - 1
            } else {
                tab
            }
        };
        drop(window);
        self.activate_tab(tab)
    }

    fn activate_last_tab(&mut self) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let last_idx = window.get_last_active_idx();
        drop(window);
        match last_idx {
            Some(idx) => self.activate_tab(idx as isize),
            None => Ok(()),
        }
    }

    fn move_tab(&mut self, tab_idx: usize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let mut window = mux
            .get_window_mut(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        let active = window.get_active_idx();

        ensure!(tab_idx < max, "cannot move a tab out of range");

        let tab_inst = window.remove_by_idx(active);
        window.insert(tab_idx, &tab_inst);
        window.set_active_without_saving(tab_idx);

        drop(window);
        self.update_title();
        self.update_scrollbar();

        Ok(())
    }

    /// Move the active tab into a window of its own, panes and all.
    ///
    /// `PaneSelect(MoveToNewWindow)` only relocates a single pane, so a split
    /// tab cannot be detached without taking it apart first. The tab itself is
    /// just an `Arc<Tab>` moving between windows: the PTYs, scrollback and
    /// title ride along untouched.
    fn move_tab_to_new_window(&mut self) -> anyhow::Result<()> {
        let mux = Mux::get();

        let (tab, workspace) = {
            let mut window = mux
                .get_window_mut(self.mux_window_id)
                .ok_or_else(|| anyhow!("no such window"))?;

            // A lone tab already owns its window. Detaching it would prune the
            // source window and rebuild an identical one, which reads to the
            // user as the window randomly jumping.
            ensure!(window.len() > 1, "window has only one tab");

            let active = window.get_active_idx();
            let workspace = window.get_workspace().to_string();
            (window.remove_by_idx(active), workspace)
        };

        let builder = mux.new_empty_window(Some(workspace), None);
        let new_window_id = *builder;
        if let Err(err) = mux.add_tab_to_window(&tab, new_window_id) {
            // Never strand a tab: put it back before giving up, and drop the
            // window we speculatively created rather than leaving it empty.
            if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
                window.push(&tab);
            }
            builder.cancel();
            return Err(err);
        }
        // Dropping the builder fires WindowCreated, which is what makes the GUI
        // materialize the new window.
        drop(builder);

        self.update_title();
        self.update_scrollbar();

        Ok(())
    }

    /// Compute render offsets for all tabs (dragged + animated)
    pub fn compute_tab_render_offsets(&mut self) -> HashMap<usize, f32> {
        let mut offsets = HashMap::new();

        // 1. Dragged tab offset
        if let Some(ref state) = self.tab_drag_state {
            if state.has_dragged {
                offsets.insert(state.tab_idx, state.drag_offset_x);
            }
        }

        // 2. Animated tab offsets.
        // Never overwrite a live drag offset: if the dragged tab has a stale
        // animation (e.g. from a swap that landed on the same slot), discard it.
        let dragged_idx = self
            .tab_drag_state
            .as_ref()
            .filter(|s| s.has_dragged)
            .map(|s| s.tab_idx);
        let mut to_remove = Vec::new();
        for (&tab_idx, (start_offset, ease)) in self.tab_position_animations.iter() {
            if Some(tab_idx) == dragged_idx {
                to_remove.push(tab_idx);
                continue;
            }
            let mut ease_mut = ease.borrow_mut();
            if let Some((intensity, next_due)) = ease_mut.intensity_one_shot() {
                if intensity > 0.0 {
                    // Animate from start_offset to 0
                    let current_offset = *start_offset * intensity;
                    offsets.insert(tab_idx, current_offset);

                    // Schedule next frame
                    self.update_next_frame_time(Some(next_due));
                } else {
                    to_remove.push(tab_idx);
                }
            } else {
                to_remove.push(tab_idx);
            }
        }

        // Remove completed animations
        for tab_idx in to_remove {
            self.tab_position_animations.remove(&tab_idx);
        }

        offsets
    }

    pub fn cleanup_tab_animations(&mut self) {
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            self.tab_position_animations.clear();
            return;
        };

        let max_idx = window.len();
        self.tab_position_animations.retain(|&idx, _| idx < max_idx);
    }

    pub fn is_tab_being_dragged(&self, tab_idx: usize) -> bool {
        self.tab_drag_state
            .as_ref()
            .map(|s| s.has_dragged && s.tab_idx == tab_idx)
            .unwrap_or(false)
    }

    fn show_input_selector(&mut self, args: &config::keyassignment::InputSelector) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        // Ignore any current overlay: we're going to cancel it out below
        // and we don't want this new one to reference that cancelled pane
        let pane = match self.get_active_pane_no_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::selector::selector(term, args, gui_win, pane)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    fn show_prompt_input_line(&mut self, args: &PromptInputLine) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::prompt::show_line_prompt_overlay(term, args, gui_win, pane)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    fn show_confirmation(&mut self, args: &Confirmation) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        let pane = match self.get_active_pane_or_overlay() {
            Some(pane) => pane,
            None => return,
        };

        let args = args.clone();

        let gui_win = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        let (overlay, future) = start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::confirm::show_confirmation_overlay(term, args, gui_win, pane)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    /// Show a confirmation overlay before applying a pending update, since
    /// applying closes every window and stops running tasks. Routed here from
    /// the update toast click, menu "Restart to Update", and menu "Check for
    /// Updates" when a staged package is already ready.
    pub(crate) fn show_update_confirmation(&mut self) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        if let Some(window) = self.window.clone() {
            let (overlay, future) = start_overlay(self, &tab, move |tab_id, term| {
                crate::overlay::confirm_apply_update(term, window, tab_id)
            });
            self.assign_overlay(tab.tab_id(), overlay);
            promise::spawn::spawn(future).detach();
        }
    }

    fn show_tab_navigator(&mut self) {
        let mux = Mux::get();
        let active_tab_idx = match mux.get_window(self.mux_window_id) {
            Some(mux_window) => mux_window.get_active_idx(),
            None => return,
        };
        self.show_tab_navigator_at(active_tab_idx);
    }

    fn show_tab_navigator_at(&mut self, initial_choice_idx: usize) {
        let mux = Mux::get();
        let initial_choice_idx = match mux.get_window(self.mux_window_id) {
            Some(mux_window) if !mux_window.is_empty() => {
                initial_choice_idx.min(mux_window.len() - 1)
            }
            _ => return,
        };
        let title = "Tab Navigator".to_string();
        let args = LauncherActionArgs {
            title: Some(title),
            flags: LauncherFlags::TABS,
            help_text: Some(
                "Select an item and press Enter=launch  Backspace=close  Esc=cancel  /=filter"
                    .to_string(),
            ),
            fuzzy_help_text: None,
            alphabet: None,
        };
        self.show_launcher_impl(args, initial_choice_idx, true);
    }

    fn show_launcher(&mut self) {
        let title = "Launcher".to_string();
        let args = LauncherActionArgs {
            title: Some(title),
            flags: LauncherFlags::LAUNCH_MENU_ITEMS
                | LauncherFlags::WORKSPACES
                | LauncherFlags::DOMAINS,
            help_text: None,
            fuzzy_help_text: None,
            alphabet: None,
        };
        self.show_launcher_impl(args, 0, false);
    }

    fn show_launcher_impl(
        &mut self,
        args: LauncherActionArgs,
        initial_choice_idx: usize,
        is_tab_navigator: bool,
    ) {
        let window = self.window.as_ref().unwrap().clone();

        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        // A rebuilt Navigator can overlap the prior overlay's shutdown, so bind
        // its actions to the real pane rather than the overlay being replaced.
        let pane = if is_tab_navigator {
            match tab.get_active_pane() {
                Some(pane) => pane,
                None => return,
            }
        } else {
            match self.get_active_pane_or_overlay() {
                Some(pane) => pane,
                None => return,
            }
        };

        let domain_id_of_current_pane = tab
            .get_active_pane()
            .expect("tab has no panes!")
            .domain_id();
        let pane_id = pane.pane_id();
        let tab_id = tab.tab_id();
        let title = args.title.unwrap();
        let flags = args.flags;
        let help_text = args.help_text.unwrap_or(
            "Select an item and press Enter=launch  \
             Esc=cancel  /=filter"
                .to_string(),
        );
        let fuzzy_help_text = args
            .fuzzy_help_text
            .unwrap_or("Fuzzy matching: ".to_string());

        let config = self.config.clone();
        let alphabet = args.alphabet.unwrap_or(config.launcher_alphabet.clone());
        let mut launcher_initial_choice_idx = initial_choice_idx;
        let tabs = if flags.contains(LauncherFlags::TABS) {
            let tab_info = self.get_tab_information();
            let target_tab_idx = initial_choice_idx;
            let mut entries = vec![];
            for tab in &tab_info {
                let Some(mux_tab) = mux.get_tab(tab.tab_id) else {
                    continue;
                };
                let mut panes = mux_tab.iter_panes();
                if panes.len() <= 1 {
                    let Some(pane_id) = panes.first().map(|pane| pane.pane.pane_id()) else {
                        continue;
                    };
                    if tab.tab_index == target_tab_idx {
                        launcher_initial_choice_idx = entries.len();
                    }
                    entries.push(LauncherTabEntry {
                        title: crate::tabbar::compute_tab_plain_title(tab),
                        pane_id,
                        tab_id: tab.tab_id,
                    });
                    continue;
                }

                panes.sort_by_key(|pane| (!pane.is_active, pane.index));
                let include_foreground_process = config.tab_title_show_foreground_process;
                for pane in panes {
                    if tab.tab_index == target_tab_idx && pane.is_active {
                        launcher_initial_choice_idx = entries.len();
                    }
                    let pane_info = Self::pos_pane_to_pane_info(&pane);
                    let title = if pane.is_active && !tab.tab_title.is_empty() {
                        tab.tab_title.clone()
                    } else {
                        crate::tabbar::compute_pane_plain_title(
                            &pane_info,
                            include_foreground_process,
                        )
                    };
                    entries.push(LauncherTabEntry {
                        title: if pane.is_active {
                            title
                        } else {
                            format!("  |- {title}")
                        },
                        pane_id: pane.pane.pane_id(),
                        tab_id: tab.tab_id,
                    });
                }
            }
            entries
        } else {
            vec![]
        };

        promise::spawn::spawn(async move {
            let args = LauncherArgs::new(
                &title,
                flags,
                domain_id_of_current_pane,
                &help_text,
                &fuzzy_help_text,
                &alphabet,
                tabs,
            )
            .await;

            let win = window.clone();
            win.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                let mux = Mux::get();
                if let Some(tab) = mux.get_tab(tab_id) {
                    let window = window.clone();
                    let (overlay, future) =
                        start_overlay(term_window, &tab, move |_tab_id, term| {
                            launcher(
                                args,
                                term,
                                launcher_initial_choice_idx,
                                is_tab_navigator,
                            )
                        });

                    term_window.assign_overlay(tab_id, overlay);
                    promise::spawn::spawn(async move {
                        match future.await {
                            Ok(Some(LauncherAction::Assignment(assignment))) => {
                                window.notify(TermWindowNotif::PerformAssignment {
                                    pane_id,
                                    assignment,
                                    tx: None,
                                });
                            }
                            Ok(Some(LauncherAction::ActivatePane {
                                pane_id: target_pane_id,
                                ..
                            })) => {
                                window.notify(TermWindowNotif::Apply(Box::new(
                                    move |term_window| {
                                        if let Err(err) = Mux::get()
                                            .focus_pane_and_containing_tab(target_pane_id)
                                        {
                                            log::debug!(
                                                "launcher pane {target_pane_id} is no longer available: {err:#}"
                                            );
                                        }
                                        term_window.update_title_post_status();
                                    },
                                )));
                            }
                            Ok(Some(LauncherAction::CloseNavigatorTab(tab_id))) => {
                                window.notify(TermWindowNotif::Apply(Box::new(
                                    move |term_window| {
                                        term_window.close_tab_from_navigator(tab_id);
                                    },
                                )));
                            }
                            Ok(None) => {}
                            Err(err) => log::error!("launcher failed: {err:#}"),
                        }
                    })
                    .detach();
                }
            })));
        })
        .detach();
    }

    /// Returns the Prompt semantic zones
    fn get_semantic_prompt_zones(&mut self, pane: &Arc<dyn Pane>) -> &[StableRowIndex] {
        let cache = self
            .semantic_zones
            .entry(pane.pane_id())
            .or_insert_with(SemanticZoneCache::default);

        let seqno = pane.get_current_seqno();
        if cache.seqno != seqno {
            let zones = pane.get_semantic_zones().unwrap_or_else(|_| vec![]);
            let mut zones: Vec<StableRowIndex> = zones
                .into_iter()
                .filter_map(|zone| {
                    if zone.semantic_type == wezterm_term::SemanticType::Prompt {
                        Some(zone.start_y)
                    } else {
                        None
                    }
                })
                .collect();
            // dedup to avoid issues where both left and right prompts are
            // defined: we only care if there were 1+ prompts on a line,
            // not about how many prompts are on a line.
            // <https://github.com/wezterm/wezterm/issues/1121>
            zones.dedup();
            cache.zones = zones;
            cache.seqno = seqno;
        }
        &cache.zones
    }

    fn scroll_to_prompt(&mut self, amount: isize, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        // Exit peek mode when scroll_to_prompt leaves current viewport
        if pane.is_primary_peek() {
            pane.set_primary_peek(false);
        }
        let dims = pane.get_dimensions();
        let position = self.effective_viewport(pane).unwrap_or(dims.physical_top);
        let zone = {
            let zones = self.get_semantic_prompt_zones(&pane);
            let idx = match zones.binary_search(&position) {
                Ok(idx) | Err(idx) => idx,
            };
            let idx = ((idx as isize) + amount).max(0) as usize;
            zones.get(idx).cloned()
        };
        if let Some(zone) = zone {
            self.set_viewport(pane.pane_id(), Some(zone), dims);
        }

        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn scroll_by_page(&mut self, amount: f64, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let dims = pane.get_dimensions();
        let position = self.effective_viewport(pane).unwrap_or(dims.physical_top) as f64
            + (amount * dims.viewport_rows as f64);
        self.set_viewport(pane.pane_id(), Some(position as isize), dims);
        // Exit peek mode when scrolling to bottom
        if pane.is_primary_peek() && self.effective_viewport(pane).is_none() {
            pane.set_primary_peek(false);
        }
        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn scroll_by_current_event_wheel_delta(&mut self, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        if let Some(event) = &self.current_mouse_event {
            let amount = match event.kind {
                MouseEventKind::VertWheel(amount) => -amount,
                _ => return Ok(()),
            };
            self.scroll_by_line(amount.into(), pane)?;
        }
        Ok(())
    }

    fn scroll_by_line(&mut self, amount: isize, pane: &Arc<dyn Pane>) -> anyhow::Result<()> {
        let alt = pane.is_alt_screen_active();
        let was_peeking = pane.is_primary_peek();

        // Alt screen + scroll up → enter Primary Screen Peek
        if alt && amount < 0 && !was_peeking {
            pane.set_primary_peek(true);
        }

        let dims = pane.get_dimensions();
        let position = self
            .effective_viewport(pane)
            .unwrap_or(dims.physical_top)
            .saturating_add(amount)
            .max(dims.scrollback_top);

        self.reveal_scrollbar();
        self.set_viewport(pane.pane_id(), Some(position), dims);

        // Scroll to bottom → exit peek, return to alt screen
        if pane.is_primary_peek() && self.effective_viewport(pane).is_none() {
            pane.set_primary_peek(false);
        }

        if let Some(win) = self.window.as_ref() {
            win.invalidate();
        }
        Ok(())
    }

    fn move_tab_relative(&mut self, delta: isize) -> anyhow::Result<()> {
        let mux = Mux::get();
        let window = mux
            .get_window(self.mux_window_id)
            .ok_or_else(|| anyhow!("no such window"))?;

        let max = window.len();
        ensure!(max > 0, "no more tabs");

        let active = window.get_active_idx();
        let tab = active as isize + delta;
        let tab = if tab < 0 {
            0usize
        } else if tab >= max as isize {
            max - 1
        } else {
            tab as usize
        };

        drop(window);
        self.move_tab(tab)
    }

    pub fn perform_key_assignment(
        &mut self,
        pane: &Arc<dyn Pane>,
        assignment: &KeyAssignment,
    ) -> anyhow::Result<PerformAssignmentResult> {
        use KeyAssignment::*;

        if let Some(modal) = self.get_modal() {
            if modal.perform_assignment(assignment, self) {
                return Ok(PerformAssignmentResult::Handled);
            }
        }

        match pane.perform_assignment(assignment) {
            PerformAssignmentResult::Unhandled => {}
            result => return Ok(result),
        }

        let window = self.window.as_ref().map(|w| w.clone());

        match assignment {
            ActivateKeyTable {
                name,
                timeout_milliseconds,
                replace_current,
                one_shot,
                until_unknown,
                prevent_fallback,
            } => {
                anyhow::ensure!(
                    self.keyboard.input_map.has_table(name),
                    "ActivateKeyTable: no key_table named {}",
                    name
                );
                self.keyboard.key_table_state.activate(KeyTableArgs {
                    name,
                    timeout_milliseconds: *timeout_milliseconds,
                    replace_current: *replace_current,
                    one_shot: *one_shot,
                    until_unknown: *until_unknown,
                    prevent_fallback: *prevent_fallback,
                });
                self.update_title();
            }
            PopKeyTable => {
                self.keyboard.key_table_state.pop();
                self.update_title();
            }
            ClearKeyTableStack => {
                self.keyboard.key_table_state.clear_stack();
                self.update_title();
            }
            Multiple(actions) => {
                for a in actions {
                    self.perform_key_assignment(pane, a)?;
                }
            }
            SpawnTab(spawn_where) => {
                self.spawn_tab(spawn_where);
            }
            SpawnWindow => {
                self.spawn_command(&SpawnCommand::default(), SpawnWhere::NewWindow);
            }
            SpawnCommandInNewTab(spawn) => {
                self.spawn_command(spawn, SpawnWhere::NewTab);
            }
            SpawnCommandInNewWindow(spawn) => {
                self.spawn_command(spawn, SpawnWhere::NewWindow);
            }
            SplitHorizontal(spawn) => {
                log::trace!("SplitHorizontal {:?}", spawn);
                self.spawn_command(
                    spawn,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: SplitDirection::Horizontal,
                        target_is_second: true,
                        size: MuxSplitSize::Percent(50),
                        top_level: false,
                    }),
                );
            }
            SplitVertical(spawn) => {
                log::trace!("SplitVertical {:?}", spawn);
                self.spawn_command(
                    spawn,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: SplitDirection::Vertical,
                        target_is_second: true,
                        size: MuxSplitSize::Percent(50),
                        top_level: false,
                    }),
                );
            }
            ToggleFullScreen => {
                if let Some(w) = self.window.as_ref() {
                    w.toggle_fullscreen();
                }
            }
            CenterWindow => {
                self.center_current_window();
            }
            MaximizeWindow => {
                if let Some(w) = self.window.as_ref() {
                    w.maximize();
                }
            }
            ToggleAlwaysOnTop => {
                let window = match self.window.clone() {
                    Some(w) => w,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                let current_level = self.window_state.as_window_level();

                match current_level {
                    WindowLevel::AlwaysOnTop => {
                        window.set_window_level(WindowLevel::Normal);
                    }
                    WindowLevel::AlwaysOnBottom | WindowLevel::Normal => {
                        window.set_window_level(WindowLevel::AlwaysOnTop);
                    }
                }
            }
            ToggleAlwaysOnBottom => {
                if let Some(window) = self.window.clone() {
                    let current_level = self.window_state.as_window_level();

                    match current_level {
                        WindowLevel::AlwaysOnBottom => {
                            window.set_window_level(WindowLevel::Normal);
                        }
                        WindowLevel::AlwaysOnTop | WindowLevel::Normal => {
                            window.set_window_level(WindowLevel::AlwaysOnBottom);
                        }
                    }
                }
            }
            SetWindowLevel(level) => {
                if let Some(window) = self.window.clone() {
                    window.set_window_level(level.clone());
                }
            }
            CopyTo(dest) => {
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    self.copy_to_clipboard(*dest, text);
                }
            }
            CopyTextTo { text, destination } => {
                self.copy_to_clipboard(*destination, text.clone());
            }
            PasteFrom(source) => {
                self.paste_from_clipboard(pane, *source);
            }
            ActivateTabRelative(n) => {
                self.activate_tab_relative(*n, true)?;
            }
            ActivateTabRelativeNoWrap(n) => {
                self.activate_tab_relative(*n, false)?;
            }
            ActivateLastTab => self.activate_last_tab()?,
            ToggleCurrentTabPanesInputBroadcast => {
                self.toggle_input_broadcast_mode(InputBroadcastMode::CurrentTab)
            }
            ToggleAllPanesInputBroadcast => {
                self.toggle_input_broadcast_mode(InputBroadcastMode::AllTabs)
            }
            DecreaseFontSize => self.decrease_font_size(),
            IncreaseFontSize => self.increase_font_size(),
            ResetFontSize => self.reset_font_size(),
            ResetFontAndWindowSize => {
                if let Some(w) = window.as_ref() {
                    self.reset_font_and_window_size(&w)?
                }
            }
            ActivateTab(n) => {
                self.activate_tab(*n)?;
            }
            ActivateWindow(n) => {
                self.activate_window(*n)?;
            }
            ActivateWindowRelative(n) => {
                self.activate_window_relative(*n, true)?;
            }
            ActivateWindowRelativeNoWrap(n) => {
                self.activate_window_relative(*n, false)?;
            }
            SendString(s) => self.write_terminal_input_bytes(pane, s.as_bytes())?,
            SendStringIfNotAltScreen(s) => {
                if !pane.is_alt_screen_active() {
                    self.write_terminal_input_bytes(pane, s.as_bytes())?;
                }
            }
            SendKey(key) => {
                use keyevent::Key;
                let mods = key.mods;
                if let Key::Code(key) = self.win_key_code_to_termwiz_key_code(
                    &key.key.resolve(self.config.key_map_preference),
                ) {
                    self.send_terminal_input_key(pane, key, mods, true, None)?;
                }
            }
            Hide => {
                if let Some(w) = window.as_ref() {
                    w.hide();
                }
            }
            Show => {
                if let Some(w) = window.as_ref() {
                    w.show();
                }
            }
            CloseCurrentTab { confirm } => self.close_current_tab(*confirm),
            CloseCurrentPane { confirm } => self.close_current_pane(*confirm),
            ReopenLastClosedTab => {
                if let Some(cwd) = self.pop_closed_tab_cwd() {
                    let spawn = SpawnCommand {
                        cwd: Some(cwd),
                        domain: config::keyassignment::SpawnTabDomain::CurrentPaneDomain,
                        ..SpawnCommand::default()
                    };
                    self.spawn_command(&spawn, SpawnWhere::NewTab);
                }
                // No history: no-op, consistent with Chrome/Safari behavior
            }
            RestorePreviousWindow => {
                crate::session_restore::restore_previous_window_from_menu(Some(self.mux_window_id));
            }
            Nop | DisableDefaultAssignment => {}
            ReloadConfiguration => {
                config::reload();
                refresh_fast_config_snapshot();
            }
            MoveTab(n) => self.move_tab(*n)?,
            MoveTabToNewWindow => self.move_tab_to_new_window()?,
            MoveTabRelative(n) => self.move_tab_relative(*n)?,
            ScrollByPage(n) => self.scroll_by_page(**n, pane)?,
            ScrollByLine(n) => self.scroll_by_line(*n, pane)?,
            ScrollByCurrentEventWheelDelta => self.scroll_by_current_event_wheel_delta(pane)?,
            ScrollToPrompt(n) => self.scroll_to_prompt(*n, pane)?,
            ScrollToTop => self.scroll_to_top(pane),
            ScrollToBottom => self.scroll_to_bottom(pane),
            ShowTabNavigator => self.show_tab_navigator(),
            ShowDebugOverlay => {
                crate::frontend::run_kaku_doctor_in_new_tab();
            }
            ShowLauncher => self.show_launcher(),
            ShowLauncherArgs(args) => {
                let title = args.title.clone().unwrap_or("Launcher".to_string());
                let args = LauncherActionArgs {
                    title: Some(title),
                    flags: args.flags,
                    help_text: args.help_text.clone(),
                    fuzzy_help_text: args.fuzzy_help_text.clone(),
                    alphabet: args.alphabet.clone(),
                };
                self.show_launcher_impl(args, 0, false);
            }
            HideApplication => {
                let con = Connection::get().expect("call on gui thread");
                con.hide_application();
            }
            QuitApplication => {
                let mux = Mux::get();
                let config = &self.config;

                let prompt = match config.window_close_confirmation {
                    WindowCloseConfirmation::NeverPrompt => false,
                    WindowCloseConfirmation::AlwaysPrompt => true,
                    // Prompt only when some window still has a stateful
                    // process running; quit silently otherwise.
                    WindowCloseConfirmation::SmartPrompt => mux.iter_windows().iter().any(|id| {
                        mux.get_window(*id)
                            .map_or(false, |w| !w.can_close_without_prompting())
                    }),
                };

                if prompt {
                    let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                        Some(tab) => tab,
                        None => anyhow::bail!("no active tab!?"),
                    };

                    if let Some(window) = self.window.clone() {
                        let (overlay, future) = start_overlay(self, &tab, move |tab_id, term| {
                            confirm_quit_program(term, window, tab_id)
                        });
                        self.assign_overlay(tab.tab_id(), overlay);
                        promise::spawn::spawn(future).detach();
                    }
                } else {
                    #[cfg(target_os = "macos")]
                    {
                        ::window::request_terminate(
                            ::window::QuitOrigin::WindowScopeQuitApplication,
                        );
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let con = Connection::get().expect("call on gui thread");
                        con.terminate_message_loop();
                    }
                }
            }
            SelectTextAtMouseCursor(mode) => self.select_text_at_mouse_cursor(*mode, pane),
            ExtendSelectionToMouseCursor(mode) => {
                self.extend_selection_at_mouse_cursor(*mode, pane)
            }
            ClearSelection => {
                self.clear_selection(pane);
            }
            StartWindowDrag => {
                self.window_drag.position = self.current_mouse_event.clone();
                self.window_drag.is_window_dragging = self.window_drag.position.is_some();
            }
            OpenLinkAtMouseCursor => {
                self.do_open_link_at_mouse_cursor(pane);
            }
            EmitEvent(name) => {
                if let Some(job_id) = name.strip_prefix(crate::inline_ai::EVENT_PREFIX) {
                    if let Err(err) = crate::inline_ai::spawn_job(job_id) {
                        log::error!("failed to start inline AI job {job_id}: {err:#}");
                    }
                    return Ok(PerformAssignmentResult::Handled);
                } else if name == "kaku-ai-chat" {
                    ai_chat::toggle_overlay(self, pane);
                } else if name == "update-kaku" || name == "run-kaku-update" {
                    crate::frontend::check_for_updates_from_menu();
                } else if name == "restart-to-update" {
                    crate::frontend::confirm_and_apply_update();
                } else if name == "run-kaku-cli" {
                    let command = format!("{}\n", crate::frontend::kaku_cli_shell_invocation());
                    let result = pane.writer().write_all(command.as_bytes());
                    self.finish_terminal_input(pane, result)?;
                } else if name == "run-kaku-ai-config" {
                    let command = format!("{} ai\n", crate::frontend::kaku_cli_shell_invocation());
                    let result = pane.writer().write_all(command.as_bytes());
                    self.finish_terminal_input(pane, result)?;
                } else if let Some(msg) = lookup_kaku_toast(name) {
                    self.show_toast(msg.to_string());
                } else if name == "kaku-toast-ai-analyzing" {
                    let message = "Kaku Assistant analyzing command";
                    self.show_ai_progress_toast(message.to_string(), ai_toast_lifetime_ms(message));
                } else if name == "kaku-toast-ai-generating" {
                    let message = "Kaku generating command";
                    self.show_ai_progress_toast(message.to_string(), ai_toast_lifetime_ms(message));
                } else if name == "kaku-toast-ai-clear-progress" {
                    self.clear_ai_progress_toast();
                } else if name == "kaku-toast-ai-applied" {
                    // No notification on successful apply; command output is enough.
                } else if let Some(msg) = lookup_ai_toast(name) {
                    self.show_ai_result_notice(msg.to_string(), ai_toast_lifetime_ms(msg));
                } else if let Some(payload) = name.strip_prefix("kaku-toast-ai-") {
                    if let Some(message) = decode_hex_event_payload(payload) {
                        let lifetime = ai_toast_lifetime_ms(&message);
                        self.show_ai_result_notice(message, lifetime);
                    }
                } else if name == "open-kaku-config" {
                    crate::frontend::open_kaku_config();
                } else if name == crate::frontend::SET_DEFAULT_TERMINAL_EVENT {
                    match Connection::get() {
                        Some(conn) => match conn.set_default_terminal() {
                            Ok(()) => {
                                self.show_toast("Kaku is now the default terminal".to_string());
                            }
                            Err(err) => {
                                log::error!("Failed to set Kaku as default terminal: {err:#}");
                                self.show_toast("Failed to set default terminal".to_string());
                            }
                        },
                        None => {
                            log::error!(
                                "Cannot set default terminal because no GUI connection is available"
                            );
                            self.show_toast("Failed to set default terminal".to_string());
                        }
                    }
                } else {
                    self.emit_window_event(name, None);
                }
            }
            CompleteSelectionOrOpenLinkAtMouseCursor(dest) => {
                let text = self.selection_text(pane);
                if !text.is_empty() {
                    if self.config.copy_on_select {
                        self.copy_to_clipboard(*dest, text);
                        self.show_copy_toast();
                    } else {
                        self.show_copy_on_select_disabled_hint();
                    }
                } else {
                    self.do_open_link_at_mouse_cursor(pane);
                }
            }
            CompleteSelection(dest) => {
                let text = self.selection_text(pane);
                if !text.is_empty() && self.config.copy_on_select {
                    self.copy_to_clipboard(*dest, text);
                    self.show_copy_toast();
                } else if !text.is_empty() {
                    self.show_copy_on_select_disabled_hint();
                }
            }
            ClearScrollback(erase_mode) => {
                pane.erase_scrollback(scrollback_erase_mode_for_pane(
                    *erase_mode,
                    pane.is_alt_screen_active(),
                ));
                let window = self.window.as_ref().unwrap();
                window.invalidate();
            }
            Search(pattern) => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    let mut replace_current = false;
                    if let Some(existing) = pane.downcast_ref::<CopyOverlay>() {
                        let mut params = existing.get_params();
                        params.editing_search = true;
                        if !pattern.is_empty() {
                            params.pattern = self.resolve_search_pattern(pattern.clone(), &pane);
                        }
                        existing.apply_params(params);
                        replace_current = true;
                    } else {
                        let search = CopyOverlay::with_pane(
                            self,
                            &pane,
                            CopyModeParams {
                                pattern: self.resolve_search_pattern(pattern.clone(), &pane),
                                editing_search: true,
                            },
                        )?;
                        self.assign_overlay_for_pane(pane.pane_id(), search);
                    }
                    self.pane_state(pane.pane_id())
                        .overlay
                        .as_mut()
                        .map(|overlay| {
                            overlay.key_table_state.activate(KeyTableArgs {
                                name: "search_mode",
                                timeout_milliseconds: None,
                                replace_current,
                                one_shot: false,
                                until_unknown: false,
                                prevent_fallback: false,
                            });
                        });
                }
            }
            QuickSelect => {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let qa = QuickSelectOverlay::with_pane(
                        self,
                        &pane,
                        &QuickSelectArguments::default(),
                    );
                    self.assign_overlay_for_pane(pane.pane_id(), qa);
                }
            }
            QuickSelectArgs(args) => {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let qa = QuickSelectOverlay::with_pane(self, &pane, args);
                    self.assign_overlay_for_pane(pane.pane_id(), qa);
                }
            }
            ActivateCopyMode => {
                if let Some(pane) = self.get_active_pane_or_overlay() {
                    let mut replace_current = false;
                    if let Some(existing) = pane.downcast_ref::<CopyOverlay>() {
                        let mut params = existing.get_params();
                        params.editing_search = false;
                        existing.apply_params(params);
                        replace_current = true;
                    } else {
                        let copy = CopyOverlay::with_pane(
                            self,
                            &pane,
                            CopyModeParams {
                                pattern: MuxPattern::default(),
                                editing_search: false,
                            },
                        )?;
                        self.assign_overlay_for_pane(pane.pane_id(), copy);
                    }
                    self.pane_state(pane.pane_id())
                        .overlay
                        .as_mut()
                        .map(|overlay| {
                            overlay.key_table_state.activate(KeyTableArgs {
                                name: "copy_mode",
                                timeout_milliseconds: None,
                                replace_current,
                                one_shot: false,
                                until_unknown: false,
                                prevent_fallback: false,
                            });
                        });
                }
            }
            AdjustPaneSize(direction, amount) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    tab.adjust_pane_size(*direction, *amount);
                }
            }
            ActivatePaneByIndex(index) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    let panes = tab.iter_panes();
                    if panes.iter().position(|p| p.index == *index).is_some() {
                        tab.set_active_idx(*index);
                    }
                }
            }
            ActivatePaneDirection(direction) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };

                let tab_id = tab.tab_id();

                if self.tab_state(tab_id).overlay.is_none() {
                    tab.activate_pane_direction(*direction);
                }
            }
            TogglePaneZoomState => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.toggle_zoom();
            }
            SetPaneZoomState(zoomed) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.set_zoomed(*zoomed);
            }
            SwitchWorkspaceRelative(delta) => {
                let mux = Mux::get();
                let workspace = mux.active_workspace();
                let workspaces = mux.iter_workspaces();
                let idx = workspaces.iter().position(|w| *w == workspace).unwrap_or(0);
                let new_idx = idx as isize + delta;
                let new_idx = if new_idx < 0 {
                    workspaces.len() as isize + new_idx
                } else {
                    new_idx
                };
                let new_idx = new_idx as usize % workspaces.len();
                if let Some(w) = workspaces.get(new_idx) {
                    front_end().switch_workspace(w);
                }
            }
            SwitchToWorkspace { name, spawn } => {
                let activity = crate::Activity::new();
                let mux = Mux::get();
                let name = name
                    .as_ref()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| mux.generate_workspace_name());
                let switcher = crate::frontend::WorkspaceSwitcher::new(&name);
                mux.set_active_workspace(&name);

                if mux.iter_windows_in_workspace(&name).is_empty() {
                    let spawn = spawn.as_ref().map(|s| s.clone()).unwrap_or_default();
                    let size = self.terminal_size;
                    let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
                    let src_window_id = self.mux_window_id;

                    promise::spawn::spawn(async move {
                        if let Err(err) = crate::spawn::spawn_command_internal(
                            spawn,
                            SpawnWhere::NewWindow,
                            size,
                            Some(src_window_id),
                            term_config,
                        )
                        .await
                        {
                            log::error!("Failed to spawn: {:#}", err);
                        }
                        switcher.do_switch();
                        drop(activity);
                    })
                    .detach();
                } else {
                    switcher.do_switch();
                }
            }
            DetachDomain(domain) => {
                let domain = Mux::get().resolve_spawn_tab_domain(Some(pane.pane_id()), domain)?;
                domain.detach()?;
            }
            AttachDomain(domain) => {
                let window = self.mux_window_id;
                let domain = domain.to_string();
                let dpi = self.dimensions.dpi as u32;

                promise::spawn::spawn(async move {
                    let mux = Mux::get();
                    let domain = mux
                        .get_domain_by_name(&domain)
                        .ok_or_else(|| anyhow!("{} is not a valid domain name", domain))?;
                    domain.attach(Some(window)).await?;

                    let have_panes_in_domain = mux
                        .iter_panes()
                        .iter()
                        .any(|p| p.domain_id() == domain.domain_id());

                    if !have_panes_in_domain {
                        let config = config::configuration();
                        let _tab = domain
                            .spawn(
                                &mux,
                                config.initial_size(
                                    dpi,
                                    Some(crate::cell_pixel_dims(&config, dpi as f64)?),
                                ),
                                None,
                                None,
                                config.default_encoding,
                                window,
                            )
                            .await?;
                    }

                    Result::<(), anyhow::Error>::Ok(())
                })
                .detach();
            }
            CopyMode(_) => {
                // NOP here; handled by the overlay directly
            }
            RotatePanes(direction) => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                match direction {
                    RotationDirection::Clockwise => tab.rotate_clockwise(),
                    RotationDirection::CounterClockwise => tab.rotate_counter_clockwise(),
                }
            }
            TogglePaneSplitDirection => {
                let mux = Mux::get();
                let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
                    Some(tab) => tab,
                    None => return Ok(PerformAssignmentResult::Handled),
                };
                tab.toggle_pane_split_direction();
            }
            SplitPane(split) => {
                log::trace!("SplitPane {:?}", split);
                self.spawn_command(
                    &split.command,
                    SpawnWhere::SplitPane(SplitRequest {
                        direction: match split.direction {
                            PaneDirection::Down | PaneDirection::Up => SplitDirection::Vertical,
                            PaneDirection::Left | PaneDirection::Right => {
                                SplitDirection::Horizontal
                            }
                            PaneDirection::Next | PaneDirection::Prev => {
                                log::error!(
                                    "Invalid direction {:?} for SplitPane",
                                    split.direction
                                );
                                return Ok(PerformAssignmentResult::Handled);
                            }
                        },
                        target_is_second: match split.direction {
                            PaneDirection::Down | PaneDirection::Right => true,
                            PaneDirection::Up | PaneDirection::Left => false,
                            PaneDirection::Next | PaneDirection::Prev => unreachable!(),
                        },
                        size: match split.size {
                            SplitSize::Percent(n) => MuxSplitSize::Percent(n),
                            SplitSize::Cells(n) => MuxSplitSize::Cells(n),
                        },
                        top_level: split.top_level,
                    }),
                );
            }
            PaneSelect(args) => {
                let modal = crate::termwindow::paneselect::PaneSelector::new(self, args);
                self.set_modal(Rc::new(modal));
            }
            CharSelect(args) => {
                let modal = crate::termwindow::charselect::CharSelector::new(self, args);
                self.set_modal(Rc::new(modal));
            }
            ResetTerminal => {
                pane.perform_actions(vec![termwiz::escape::Action::Esc(
                    termwiz::escape::Esc::Code(termwiz::escape::EscCode::FullReset),
                )]);
            }
            OpenUri(link) => {
                wezterm_open_url::open_url(link);
            }
            ActivateCommandPalette => {
                let modal = crate::termwindow::palette::CommandPalette::new(self);
                self.set_modal(Rc::new(modal));
            }
            PromptInputLine(args) => self.show_prompt_input_line(args),
            InputSelector(args) => self.show_input_selector(args),
            Confirmation(args) => self.show_confirmation(args),
            SetPaneEncoding(encoding) => {
                let encoding: PaneEncoding = *encoding;
                PaneEncoding::set_last_selected(encoding);
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    pane.set_encoding(encoding);
                }
            }
        };
        Ok(PerformAssignmentResult::Handled)
    }

    fn do_open_link_at_mouse_cursor(&self, pane: &Arc<dyn Pane>) {
        // They clicked on a link, so let's open it!
        // We need to ensure that we spawn the `open` call outside of the context
        // of our window loop; on Windows it can cause a panic due to
        // triggering our WndProc recursively.
        // We get that assurance for free as part of the async dispatch that we
        // perform below; here we allow the user to define an `open-uri` event
        // handler that can bypass the normal `open_url` functionality.
        if let Some(link) = self.current_highlight.as_ref().cloned() {
            let uri = link.uri().to_string();
            let is_file_uri = uri.starts_with("file://");
            let is_explicit_file_link = is_file_uri && !link.is_implicit();
            let resolved_target = if is_file_uri {
                self.resolve_file_path(pane, &uri)
            } else {
                None
            };
            let file_link_editor = self.config.file_link_editor.clone();

            let window = GuiWin::new(self);
            let pane = MuxPane(pane.pane_id());

            async fn open_uri(
                lua: Option<Rc<mlua::Lua>>,
                window: GuiWin,
                pane: MuxPane,
                link: String,
                resolved_target: Option<FileLinkTarget>,
                explicit_file_link: bool,
                file_link_editor: Option<String>,
            ) -> anyhow::Result<()> {
                let default_click = match lua {
                    Some(lua) => {
                        let args = lua.pack_multi((window, pane, link.clone()))?;
                        config::lua::emit_event(&lua, ("open-uri".to_string(), args))
                            .await
                            .map_err(|e| {
                                log::error!("while processing open-uri event: {:#}", e);
                                e
                            })?
                    }
                    None => true,
                };
                if default_click {
                    if let Some(target) = resolved_target {
                        if target.path.exists() {
                            log::info!(
                                "Opening file path: {:?} line={:?} col={:?} explicit={}",
                                target.path,
                                target.line,
                                target.col,
                                explicit_file_link,
                            );
                            #[cfg(target_os = "macos")]
                            let use_default_app = target.path.is_file()
                                && crate::macos::file_link::should_open_with_default_app(
                                    &target.path,
                                );
                            #[cfg(not(target_os = "macos"))]
                            let use_default_app = false;

                            crate::thread_util::spawn_with_pool(move || {
                                if let Err(err) = TermWindow::open_file_link_target(
                                    &target,
                                    explicit_file_link,
                                    use_default_app,
                                    file_link_editor.as_deref(),
                                ) {
                                    log::warn!(
                                        "Failed to open file link target {:?}: {err:#}",
                                        target.path
                                    );
                                }
                            });
                        } else {
                            log::warn!("File does not exist: {:?}", target.path);
                        }
                    } else {
                        log::info!("clicking {}", link);
                        wezterm_open_url::open_url(&link);
                    }
                }
                Ok(())
            }

            promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
                open_uri(
                    lua,
                    window,
                    pane,
                    uri,
                    resolved_target,
                    is_explicit_file_link,
                    file_link_editor,
                )
            }))
            .detach();
        }
    }

    fn resolve_file_path(&self, pane: &Arc<dyn Pane>, uri: &str) -> Option<FileLinkTarget> {
        let decoded_uri_path = url::Url::parse(uri)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .map(|path| path.to_string_lossy().into_owned());

        let path_str = decoded_uri_path
            .as_deref()
            .unwrap_or_else(|| uri.strip_prefix("file://").unwrap_or(uri));
        let (base_path, line, col) = Self::parse_file_location(path_str);

        let path = if base_path.starts_with('/') {
            Some(PathBuf::from(&base_path))
        } else if base_path.starts_with("~/") {
            dirs_next::home_dir().map(|home| home.join(&base_path[2..]))
        } else {
            pane.get_current_working_dir(CachePolicy::AllowStale)
                .and_then(|url| url.to_file_path().ok())
                .map(|cwd| cwd.join(&base_path))
        }?;

        Some(FileLinkTarget { path, line, col })
    }

    fn open_file_link_target(
        target: &FileLinkTarget,
        explicit: bool,
        use_default_app: bool,
        configured_editor: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut configured_editor_error = None;
        if let Some(editor) = configured_editor {
            match Self::try_open_configured_editor(editor, target) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    log::warn!(
                        "Configured file link editor `{}` failed; trying fallbacks: {error:#}",
                        editor
                    );
                    configured_editor_error = Some(error);
                }
            }
        }

        #[cfg(target_os = "macos")]
        if explicit && Self::try_open_path_with_default_app(&target.path)? {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        if use_default_app && Self::try_open_path_with_default_app(&target.path)? {
            return Ok(());
        }

        if target.path.is_file() && Self::try_open_file_in_vscode(target)? {
            return Ok(());
        }

        if Self::try_open_in_configured_editor(&target.path)? {
            return Ok(());
        }

        if Self::try_open_path_in_vscode(&target.path)? {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            if Self::try_open_path_with_default_app(&target.path)? {
                return Ok(());
            }

            if target.path.is_file() && Self::try_open_text(&target.path)? {
                return Ok(());
            }

            if target.path.is_file() && Self::try_reveal_in_finder(&target.path)? {
                return Ok(());
            }
        }

        if let Some(error) = configured_editor_error {
            return Err(error).with_context(|| {
                format!(
                    "configured editor and all fallbacks failed for {}",
                    target.path.display()
                )
            });
        }
        anyhow::bail!("failed to open {}", target.path.display())
    }

    fn try_open_file_in_vscode(target: &FileLinkTarget) -> anyhow::Result<bool> {
        let Some(line) = target.line else {
            return Ok(false);
        };

        let mut location = format!("{}:{line}", target.path.display());
        if let Some(col) = target.col {
            location.push(':');
            location.push_str(&col.to_string());
        }

        for candidate in VSCODE_OPEN_CANDIDATES {
            let result = std::process::Command::new(candidate)
                .arg("-g")
                .arg(&location)
                .status();

            match result {
                Ok(status) if status.success() => return Ok(true),
                Ok(_) => return Ok(false),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            }
        }

        Ok(false)
    }

    fn try_open_configured_editor(raw: &str, target: &FileLinkTarget) -> anyhow::Result<()> {
        let (program, args) =
            Self::parse_editor_command(raw.trim()).context("parse config.file_link_editor")?;
        let location = Self::file_link_location(target);

        Self::run_editor_path_command(&program, &args, Path::new(&location))
            .with_context(|| format!("launch config.file_link_editor `{program}`"))
    }

    fn editor_program_candidates(program: &str) -> Vec<String> {
        if program.contains('/') {
            return vec![program.to_string()];
        }

        let mut candidates = vec![program.to_string()];
        if let Some(home) = dirs_next::home_dir() {
            candidates.push(
                home.join(".local")
                    .join("bin")
                    .join(program)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        candidates.push(format!("/opt/homebrew/bin/{program}"));
        candidates.push(format!("/usr/local/bin/{program}"));
        match program {
            "code" => candidates.extend(
                VSCODE_OPEN_CANDIDATES
                    .iter()
                    .skip(1)
                    .map(|candidate| (*candidate).to_string()),
            ),
            "cursor" => candidates
                .push("/Applications/Cursor.app/Contents/Resources/app/bin/cursor".to_string()),
            _ => {}
        }
        // De-dup while preserving probe order: PATH lookup first, then the
        // conventional locations in the order they were pushed.
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.clone()));
        candidates
    }

    fn run_editor_path_command(program: &str, args: &[String], path: &Path) -> std::io::Result<()> {
        let mut not_found = None;
        for candidate in Self::editor_program_candidates(program) {
            match Self::run_path_command(&candidate, args, path) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    not_found = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(not_found.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("editor executable `{program}` was not found"),
            )
        }))
    }

    fn file_link_location(target: &FileLinkTarget) -> String {
        let mut location = target.path.display().to_string();
        if let Some(line) = target.line {
            location.push(':');
            location.push_str(&line.to_string());
            if let Some(col) = target.col {
                location.push(':');
                location.push_str(&col.to_string());
            }
        }
        location
    }

    fn try_open_in_configured_editor(path: &Path) -> anyhow::Result<bool> {
        for var in ["VISUAL", "EDITOR"] {
            if Self::try_open_in_env_editor(var, path)? {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn try_open_in_env_editor(var: &str, path: &Path) -> anyhow::Result<bool> {
        let Some(raw) = std::env::var_os(var) else {
            return Ok(false);
        };

        let raw = raw.to_string_lossy();
        let (program, args) =
            Self::parse_editor_command(raw.trim()).with_context(|| format!("parse ${var}"))?;

        Self::run_path_command(&program, &args, path)
            .with_context(|| format!("launch ${var} editor `{program}`"))?;
        Ok(true)
    }

    fn parse_editor_command(raw: &str) -> anyhow::Result<(String, Vec<String>)> {
        let parts = shlex::split(raw).context("invalid shell quoting")?;
        let Some((program, args)) = parts.split_first() else {
            anyhow::bail!("editor command is empty");
        };
        Ok((program.clone(), args.to_vec()))
    }

    fn try_open_path_in_vscode(path: &Path) -> anyhow::Result<bool> {
        for candidate in VSCODE_OPEN_CANDIDATES {
            let result = Command::new(candidate).arg(path).status();

            match result {
                Ok(status) if status.success() => return Ok(true),
                Ok(_) => return Ok(false),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            }
        }

        Ok(false)
    }

    #[cfg(target_os = "macos")]
    fn try_open_text(path: &Path) -> anyhow::Result<bool> {
        match Self::run_path_command("/usr/bin/open", &["-t".to_string()], path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).context("launch macOS text editor"),
        }
    }

    #[cfg(target_os = "macos")]
    fn try_open_path_with_default_app(path: &Path) -> anyhow::Result<bool> {
        crate::macos::file_link::open_with_default_app(path).context("launch default app")
    }

    #[cfg(target_os = "macos")]
    fn try_reveal_in_finder(path: &Path) -> anyhow::Result<bool> {
        match Self::run_path_command("/usr/bin/open", &["-R".to_string()], path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).context("reveal file in Finder"),
        }
    }

    fn run_path_command(program: &str, args: &[String], path: &Path) -> std::io::Result<()> {
        let status = Command::new(program)
            .args(args)
            .arg(path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if status.success() {
            return Ok(());
        }

        Err(std::io::Error::other(format!(
            "`{program}` exited with status {status}"
        )))
    }

    fn parse_file_location(path: &str) -> (String, Option<usize>, Option<usize>) {
        let parts: Vec<&str> = path.rsplitn(3, ':').collect();

        match parts.as_slice() {
            [col_str, line_str, file_path]
                if !col_str.is_empty()
                    && !line_str.is_empty()
                    && col_str.chars().all(|c| c.is_ascii_digit())
                    && line_str.chars().all(|c| c.is_ascii_digit()) =>
            {
                (
                    file_path.to_string(),
                    line_str.parse().ok(),
                    col_str.parse().ok(),
                )
            }
            _ => {
                let parts: Vec<&str> = path.rsplitn(2, ':').collect();
                match parts.as_slice() {
                    [line_str, file_path]
                        if !line_str.is_empty() && line_str.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        (file_path.to_string(), line_str.parse().ok(), None)
                    }
                    _ => (path.to_string(), None, None),
                }
            }
        }
    }

    fn close_current_pane(&mut self, confirm: bool) {
        let mux_window_id = self.mux_window_id;
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(mux_window_id) {
            Some(tab) => tab,
            None => return,
        };

        // Last pane in tab: close the tab instead so remove_tab cascades
        // to window removal (fixes Cmd+W in fullscreen with one tab).
        if tab.count_panes() == Some(1) {
            drop(tab);
            return self.close_current_tab(confirm);
        }

        let pane = match tab.get_active_pane() {
            Some(p) => p,
            None => return,
        };

        let pane_id = pane.pane_id();
        let should_confirm = self
            .config
            .pane_close_confirmation
            .should_prompt(confirm, || {
                pane.can_close_without_prompting(CloseReason::Pane)
            });
        if should_confirm {
            if let Some(window) = self.window.clone() {
                let (overlay, future) = start_overlay_pane(self, &pane, move |pane_id, term| {
                    confirm_close_pane(pane_id, term, mux_window_id, window)
                });
                self.assign_overlay_for_pane(pane_id, overlay);
                promise::spawn::spawn(future).detach();
            }
        } else {
            mux.remove_pane(pane_id);
        }
    }

    fn close_tab_from_navigator(&mut self, tab_id: TabId) {
        let mux = Mux::get();
        let target = {
            let mux_window = match mux.get_window(self.mux_window_id) {
                Some(window) => window,
                None => return,
            };
            if mux_window.len() <= 1 {
                return;
            }
            mux_window.idx_by_id(tab_id).and_then(|tab_idx| {
                mux_window
                    .get_by_idx(tab_idx)
                    .cloned()
                    .map(|tab| (tab_idx, tab))
            })
        };

        let Some((tab_idx, tab)) = target else {
            self.show_tab_navigator();
            return;
        };

        let should_confirm = self
            .config
            .tab_close_confirmation
            .should_prompt(true, || tab.can_close_without_prompting(CloseReason::Tab));
        if should_confirm {
            let Some(host_tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
                return;
            };
            let host_tab_id = host_tab.tab_id();
            if let Some(window) = self.window.clone() {
                let notify_window = window.clone();
                let (overlay, future) = start_overlay(self, &host_tab, move |_tab_id, mut term| {
                    crate::overlay::confirm::run_confirmation(
                        "Close this tab?\nAll panes in this tab will be terminated.",
                        &mut term,
                    )
                });
                self.assign_overlay(host_tab_id, overlay);
                promise::spawn::spawn(async move {
                    let confirmed = match future.await {
                        Ok(confirmed) => confirmed,
                        Err(err) => {
                            log::error!("tab navigator close confirmation failed: {err:#}");
                            false
                        }
                    };
                    notify_window.notify(TermWindowNotif::Apply(Box::new(move |term_window| {
                        term_window.finish_navigator_tab_close(tab_id, confirmed);
                    })));
                })
                .detach();
            }
        } else {
            self.record_closed_tab_cwd(&tab);
            if mux.remove_tab(tab_id).is_some() {
                self.update_title();
                self.show_tab_navigator_at(tab_idx);
            } else {
                self.show_tab_navigator();
            }
        }
    }

    fn finish_navigator_tab_close(&mut self, tab_id: TabId, confirmed: bool) {
        let mux = Mux::get();
        let (tab_count, active_idx, target) = {
            let mux_window = match mux.get_window(self.mux_window_id) {
                Some(window) => window,
                None => return,
            };
            let target = mux_window.idx_by_id(tab_id).and_then(|tab_idx| {
                mux_window
                    .get_by_idx(tab_idx)
                    .cloned()
                    .map(|tab| (tab_idx, tab))
            });
            (mux_window.len(), mux_window.get_active_idx(), target)
        };

        let initial_choice_idx = match target {
            Some((tab_idx, tab)) if confirmed && tab_count > 1 => {
                self.record_closed_tab_cwd(&tab);
                if mux.remove_tab(tab_id).is_some() {
                    self.update_title();
                    tab_idx
                } else {
                    active_idx
                }
            }
            Some((tab_idx, _)) => tab_idx,
            None => active_idx,
        };

        self.show_tab_navigator_at(initial_choice_idx);
    }

    fn close_specific_tab(&mut self, tab_idx: usize, confirm: bool) {
        let mux = Mux::get();
        let mux_window_id = self.mux_window_id;
        let mux_window = match mux.get_window(mux_window_id) {
            Some(w) => w,
            None => return,
        };

        let tab = match mux_window.get_by_idx(tab_idx) {
            Some(tab) => Arc::clone(tab),
            None => return,
        };
        drop(mux_window);

        let tab_id = tab.tab_id();
        let should_confirm = self
            .config
            .tab_close_confirmation
            .should_prompt(confirm, || {
                tab.can_close_without_prompting(CloseReason::Tab)
            });
        if should_confirm {
            if self.activate_tab(tab_idx as isize).is_err() {
                return;
            }

            if let Some(window) = self.window.clone() {
                let (overlay, future) = start_overlay(self, &tab, move |tab_id, term| {
                    confirm_close_tab(tab_id, term, mux_window_id, window)
                });
                self.assign_overlay(tab_id, overlay);
                promise::spawn::spawn(future).detach();
            }
        } else {
            mux.remove_tab(tab_id);
        }
    }

    fn close_current_tab(&mut self, confirm: bool) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };
        let tab_id = tab.tab_id();
        let mux_window_id = self.mux_window_id;

        let should_confirm = self
            .config
            .tab_close_confirmation
            .should_prompt(confirm, || {
                tab.can_close_without_prompting(CloseReason::Tab)
            });
        if should_confirm {
            // Tab has running processes; ask the user first.
            // We do not record the cwd here: the user may cancel, and we cannot
            // reliably hook the async confirmation result from this call site.
            // In practice, tabs with active processes are rare to close with reopen-intent.
            if let Some(window) = self.window.clone() {
                let (overlay, future) = start_overlay(self, &tab, move |tab_id, term| {
                    confirm_close_tab(tab_id, term, mux_window_id, window)
                });
                self.assign_overlay(tab_id, overlay);
                promise::spawn::spawn(future).detach();
            }
        } else {
            // No confirmation needed: record cwd and close immediately.
            self.record_closed_tab_cwd(&tab);
            mux.remove_tab(tab_id);
        }
    }

    fn record_closed_tab_cwd(&mut self, tab: &Arc<Tab>) {
        if let Some(pane) = tab.get_active_pane() {
            if let Some(cwd) = pane
                .get_current_working_dir(mux::pane::CachePolicy::AllowStale)
                .and_then(|url| url.to_file_path().ok())
            {
                if cwd.is_absolute() {
                    self.push_closed_tab_cwd(cwd);
                }
            }
        }
    }

    /// Push a cwd onto this window's closed-tab history stack (max 10 entries).
    fn push_closed_tab_cwd(&mut self, cwd: PathBuf) {
        const MAX_CLOSED_TABS: usize = 10;
        self.closed_tab_history.push_back(cwd);
        if self.closed_tab_history.len() > MAX_CLOSED_TABS {
            self.closed_tab_history.pop_front();
        }
    }

    fn pop_closed_tab_cwd(&mut self) -> Option<PathBuf> {
        self.closed_tab_history.pop_back()
    }

    pub fn pane_state(&self, pane_id: PaneId) -> RefMut<'_, PaneState> {
        RefMut::map(self.pane_state.borrow_mut(), |state| {
            state.entry(pane_id).or_insert_with(PaneState::default)
        })
    }

    pub fn tab_state(&self, tab_id: TabId) -> RefMut<'_, TabState> {
        RefMut::map(self.tab_state.borrow_mut(), |state| {
            state.entry(tab_id).or_insert_with(TabState::default)
        })
    }

    /// Resize overlays to match their corresponding tab/pane dimensions
    pub fn resize_overlays(&self) {
        let mux = Mux::get();
        for (_, state) in self.tab_state.borrow().iter() {
            if let Some(overlay) = state.overlay.as_ref().map(|o| &o.pane) {
                overlay.resize(self.terminal_size).ok();
            }
        }
        // Build a pane_id → TerminalSize map using the tab layout tree.
        // iter_panes() returns visual/layout dimensions, which are kept up-to-date
        // by resize_visual() during live dragging, unlike pane.get_dimensions(),
        // which only reflects the last physical (non-live) resize.
        let cell_h = self
            .terminal_size
            .pixel_height
            .checked_div(self.terminal_size.rows)
            .unwrap_or(1);
        let cell_w = self
            .terminal_size
            .pixel_width
            .checked_div(self.terminal_size.cols)
            .unwrap_or(1);
        let mut pane_sizes: HashMap<PaneId, TerminalSize> = HashMap::new();
        if let Some(window) = mux.get_window(self.mux_window_id) {
            for tab in window.iter() {
                for pos in tab.iter_panes() {
                    pane_sizes.insert(
                        pos.pane.pane_id(),
                        TerminalSize {
                            cols: pos.width,
                            rows: pos.height,
                            dpi: self.terminal_size.dpi,
                            pixel_width: pos.width * cell_w,
                            pixel_height: pos.height * cell_h,
                        },
                    );
                }
            }
        }
        for (pane_id, state) in self.pane_state.borrow().iter() {
            if let Some(overlay) = state.overlay.as_ref().map(|o| &o.pane) {
                if let Some(size) = pane_sizes.get(pane_id) {
                    overlay.resize(*size).ok();
                }
            }
        }
    }

    pub fn get_viewport(&self, pane_id: PaneId) -> Option<StableRowIndex> {
        self.pane_state(pane_id).viewport
    }

    pub fn effective_viewport(&self, pane: &Arc<dyn Pane>) -> Option<StableRowIndex> {
        let pane_id = pane.pane_id();
        let dims = pane.get_dimensions();
        let viewport = self.get_viewport(pane_id);
        let effective = Self::normalize_viewport(viewport, dims);

        if effective != viewport {
            log::trace!(
                "effective_viewport: pane={} normalized {:?} -> {:?} physical_top={} scrollback_top={}",
                pane_id,
                viewport,
                effective,
                dims.physical_top,
                dims.scrollback_top,
            );
        }

        effective
    }

    fn normalize_viewport(
        position: Option<StableRowIndex>,
        dims: RenderableDimensions,
    ) -> Option<StableRowIndex> {
        match position {
            Some(pos) if pos >= dims.physical_top => None,
            Some(pos) if pos < dims.scrollback_top => {
                // The viewport position has been pruned from scrollback. Snap to
                // the bottom (follow live output) instead of clamping to
                // scrollback_top: during continuous output the oldest row keeps
                // advancing, so clamping pinned the viewport to that moving top
                // edge, which read as a jarring "jump to the top" (#448). The
                // content being read is already gone, so following current
                // output is the least surprising fallback.
                None
            }
            Some(pos) => Some(pos),
            None => None,
        }
    }

    /// Normalize an explicit user-driven scroll request. Unlike passive
    /// scrollback pruning, an interactive request that overshoots the oldest
    /// row should stop there instead of snapping to the bottom.
    fn normalize_interactive_viewport(
        position: Option<StableRowIndex>,
        dims: RenderableDimensions,
    ) -> Option<StableRowIndex> {
        match position {
            Some(pos) if pos >= dims.physical_top => None,
            Some(pos) => {
                let clamped = pos.max(dims.scrollback_top);
                (clamped < dims.physical_top).then_some(clamped)
            }
            None => None,
        }
    }

    fn selection_drag_controls_pane(
        selection_drag_active: bool,
        current_mouse_capture: Option<&MouseCapture>,
        pane_id: PaneId,
    ) -> bool {
        selection_drag_active
            && matches!(
                current_mouse_capture,
                Some(MouseCapture::TerminalPane(captured_pane_id))
                    if *captured_pane_id == pane_id
            )
    }

    fn reconcile_viewport(
        position: Option<StableRowIndex>,
        was_primary_peek: bool,
        is_primary_peek: bool,
        pin_pruned_viewport: bool,
        dims: RenderableDimensions,
    ) -> Option<StableRowIndex> {
        if was_primary_peek && !is_primary_peek {
            None
        } else if pin_pruned_viewport {
            Self::normalize_interactive_viewport(position, dims)
        } else {
            Self::normalize_viewport(position, dims)
        }
    }

    fn sync_pane_viewport_state(&self, pane: &Arc<dyn Pane>) {
        let pane_id = pane.pane_id();
        let is_primary_peek = pane.is_primary_peek();
        let dims = pane.get_dimensions();
        let mut state = self.pane_state(pane_id);
        let viewport = state.viewport;
        let was_primary_peek = state.was_primary_peek;
        let pin_pruned_viewport = Self::selection_drag_controls_pane(
            self.selection_drag_active,
            self.current_mouse_capture.as_ref(),
            pane_id,
        );
        let next_viewport = Self::reconcile_viewport(
            viewport,
            was_primary_peek,
            is_primary_peek,
            pin_pruned_viewport,
            dims,
        );

        if next_viewport != viewport {
            if was_primary_peek && !is_primary_peek {
                log::trace!(
                    "sync_pane_viewport_state: clearing stale viewport after peek exit pane={} viewport={:?}",
                    pane_id,
                    viewport,
                );
            } else {
                log::trace!(
                    "sync_pane_viewport_state: normalizing viewport pane={} from {:?} to {:?} physical_top={} scrollback_top={}",
                    pane_id,
                    viewport,
                    next_viewport,
                    dims.physical_top,
                    dims.scrollback_top,
                );
            }
            state.viewport = next_viewport;
        }

        state.was_primary_peek = is_primary_peek;
    }

    pub fn set_viewport(
        &mut self,
        pane_id: PaneId,
        position: Option<StableRowIndex>,
        dims: RenderableDimensions,
    ) {
        log::trace!(
            "set_viewport: pane={} pos={:?} physical_top={} scrollback_top={}",
            pane_id,
            position,
            dims.physical_top,
            dims.scrollback_top,
        );
        let pos = Self::normalize_interactive_viewport(position, dims);

        let mut state = self.pane_state(pane_id);
        if pos != state.viewport {
            state.viewport = pos;

            // This is a bit gross.  If we add other overlays that need this information,
            // this should get extracted out into a trait
            if let Some(overlay) = state.overlay.as_ref() {
                if let Some(copy) = overlay.pane.downcast_ref::<CopyOverlay>() {
                    copy.viewport_changed(pos);
                } else if let Some(qs) = overlay.pane.downcast_ref::<QuickSelectOverlay>() {
                    qs.viewport_changed(pos);
                }
            }
        }
        if let Some(w) = self.window.as_ref() {
            w.invalidate();
        }
    }

    fn maybe_scroll_to_bottom_for_input(&mut self, pane: &Arc<dyn Pane>) {
        if self.config.scroll_to_bottom_on_input {
            self.scroll_to_bottom(pane);
        }
    }

    fn scroll_to_top(&mut self, pane: &Arc<dyn Pane>) {
        // Exit peek mode when scroll_to_top jumps to scrollback top
        if pane.is_primary_peek() {
            pane.set_primary_peek(false);
        }
        log::trace!("scroll_to_top: pane={}", pane.pane_id());
        let dims = pane.get_dimensions();
        self.reveal_scrollbar();
        self.set_viewport(pane.pane_id(), Some(dims.scrollback_top), dims);
    }

    fn scroll_to_bottom(&mut self, pane: &Arc<dyn Pane>) {
        log::trace!("scroll_to_bottom: pane={}", pane.pane_id());
        self.reveal_scrollbar();
        self.pane_state(pane.pane_id()).viewport = None;
        pane.set_primary_peek(false);
    }

    fn get_active_pane_no_overlay(&self) -> Option<Arc<dyn Pane>> {
        let mux = Mux::get();
        let tab = mux.get_active_tab_for_window(self.mux_window_id)?;
        tab.get_active_pane()
    }

    fn active_tab_id(&self) -> Option<TabId> {
        let mux = Mux::get();
        mux.get_active_tab_for_window(self.mux_window_id)
            .map(|tab| tab.tab_id())
    }

    fn input_broadcast_target_tab_id(&self) -> Option<TabId> {
        if self.input_broadcast_mode != InputBroadcastMode::CurrentTab {
            return None;
        }

        let target_tab_id = self.input_broadcast_tab_id?;
        let mux = Mux::get();
        let _tab = mux.get_tab(target_tab_id)?;
        (mux.window_containing_tab(target_tab_id) == Some(self.mux_window_id))
            .then_some(target_tab_id)
    }

    fn input_broadcast_title_prefix(&self) -> Option<&'static str> {
        if self.input_broadcast_target_count(self.input_broadcast_mode) >= 2
            && self
                .input_broadcast_mode
                .applies_to_active_tab(self.input_broadcast_target_tab_id(), self.active_tab_id())
        {
            self.input_broadcast_mode.title_prefix()
        } else {
            None
        }
    }

    fn input_broadcast_target_count_for_tab(&self, tab_id: TabId) -> usize {
        let mux = Mux::get();
        let Some(tab) = mux.get_tab(tab_id) else {
            return 0;
        };

        let mut pane_ids = std::collections::HashSet::new();
        for pos in tab.iter_panes() {
            pane_ids.insert(pos.pane.pane_id());
        }
        pane_ids.len()
    }

    fn input_broadcast_target_count(&self, mode: InputBroadcastMode) -> usize {
        let mux = Mux::get();
        let mut pane_ids = std::collections::HashSet::new();

        match mode {
            InputBroadcastMode::Off => 0,
            InputBroadcastMode::CurrentTab => {
                let Some(tab_id) = self.input_broadcast_target_tab_id() else {
                    return 0;
                };
                self.input_broadcast_target_count_for_tab(tab_id)
            }
            InputBroadcastMode::AllTabs => {
                let Some(window) = mux.get_window(self.mux_window_id) else {
                    return 0;
                };

                for tab in window.iter() {
                    for pos in tab.iter_panes() {
                        pane_ids.insert(pos.pane.pane_id());
                    }
                }
                pane_ids.len()
            }
        }
    }

    fn broadcast_input_visual_mode(&self) -> bool {
        if !self
            .input_broadcast_mode
            .applies_to_active_tab(self.input_broadcast_target_tab_id(), self.active_tab_id())
            || self.input_broadcast_target_count(self.input_broadcast_mode) < 2
        {
            return false;
        }

        match (
            self.get_active_pane_or_overlay(),
            self.get_active_pane_no_overlay(),
        ) {
            (Some(active_or_overlay), Some(active_pane)) => {
                Arc::ptr_eq(&active_or_overlay, &active_pane)
            }
            _ => false,
        }
    }

    fn is_active_mux_pane(&self, pane: &Arc<dyn Pane>) -> bool {
        self.get_active_pane_no_overlay()
            .as_ref()
            .is_some_and(|active_pane| Arc::ptr_eq(active_pane, pane))
    }

    fn terminal_input_targets(&self, pane: &Arc<dyn Pane>) -> Vec<Arc<dyn Pane>> {
        let target_tab_id = self.input_broadcast_target_tab_id();
        let active_tab_id = self.active_tab_id();
        let target_count = self.input_broadcast_target_count(self.input_broadcast_mode);
        if !self.input_broadcast_mode.should_broadcast_input(
            target_tab_id,
            active_tab_id,
            self.is_active_mux_pane(pane),
            target_count,
        ) {
            return vec![pane.clone()];
        }

        let mut result = vec![pane.clone()];

        let mux = Mux::get();
        match self.input_broadcast_mode {
            InputBroadcastMode::Off => {}
            InputBroadcastMode::CurrentTab => {
                let Some(target_tab_id) = self.input_broadcast_target_tab_id() else {
                    return vec![pane.clone()];
                };
                if self.active_tab_id() != Some(target_tab_id) {
                    return vec![pane.clone()];
                }
                let Some(tab) = mux.get_tab(target_tab_id) else {
                    return vec![pane.clone()];
                };

                for pos in tab.iter_panes() {
                    let tab_pane = pos.pane;
                    if result
                        .iter()
                        .any(|existing| existing.pane_id() == tab_pane.pane_id())
                    {
                        continue;
                    }
                    result.push(tab_pane);
                }
            }
            InputBroadcastMode::AllTabs => {
                let Some(window) = mux.get_window(self.mux_window_id) else {
                    return vec![pane.clone()];
                };

                for tab in window.iter() {
                    for pos in tab.iter_panes() {
                        let tab_pane = pos.pane;
                        if result
                            .iter()
                            .any(|existing| existing.pane_id() == tab_pane.pane_id())
                        {
                            continue;
                        }
                        result.push(tab_pane);
                    }
                }
            }
        }

        if result.len() > 1 {
            result
        } else {
            vec![pane.clone()]
        }
    }

    fn for_each_terminal_input_target<F>(
        &self,
        pane: &Arc<dyn Pane>,
        mut func: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&Arc<dyn Pane>) -> anyhow::Result<()>,
    {
        let targets = self.terminal_input_targets(pane);
        let mut primary_error = None;
        let mut fallback_error = None;
        let mut any_success = false;

        for (idx, target) in targets.iter().enumerate() {
            match func(target) {
                Ok(()) => any_success = true,
                Err(err) => {
                    if idx == 0 {
                        primary_error = Some(err);
                    } else {
                        log::warn!(
                            "broadcast terminal input to pane {} failed: {err:#}",
                            target.pane_id()
                        );
                        fallback_error.get_or_insert(err);
                    }
                }
            }
        }

        if let Some(err) = primary_error {
            return Err(err);
        }
        if any_success {
            return Ok(());
        }
        if let Some(err) = fallback_error {
            return Err(err);
        }
        Ok(())
    }

    fn write_terminal_input_bytes(&self, pane: &Arc<dyn Pane>, bytes: &[u8]) -> anyhow::Result<()> {
        self.for_each_terminal_input_target(pane, |target| {
            let result = target
                .writer()
                .write_all(bytes)
                .context("sending terminal input bytes");
            if bytes.is_empty() {
                result
            } else {
                self.finish_terminal_input(target, result)
            }
        })
    }

    fn send_terminal_input_key(
        &self,
        pane: &Arc<dyn Pane>,
        key: ::termwiz::input::KeyCode,
        modifiers: Modifiers,
        is_down: bool,
        key_event: Option<&KeyEvent>,
    ) -> anyhow::Result<()> {
        self.for_each_terminal_input_target(pane, |target| {
            let encoded = key_event.and_then(|event| {
                self.encode_win32_input(target, event)
                    .or_else(|| self.encode_kitty_input(target, event))
            });
            let result = if let Some(encoded) = encoded {
                target
                    .writer()
                    .write_all(encoded.as_bytes())
                    .context("sending encoded terminal input")
            } else if is_down {
                target.key_down(key.clone(), modifiers)
            } else {
                target.key_up(key.clone(), modifiers)
            };

            if is_down && !key.is_modifier() {
                self.finish_terminal_input(target, result)
            } else {
                result
            }
        })
    }

    fn toggle_input_broadcast_mode(&mut self, mode: InputBroadcastMode) {
        match mode {
            InputBroadcastMode::Off => {
                self.input_broadcast_mode = InputBroadcastMode::Off;
                self.input_broadcast_tab_id = None;
                self.update_title();
                self.show_toast(mode.disabled_toast().to_string());
            }
            InputBroadcastMode::CurrentTab => {
                let target_tab_id = self.active_tab_id();
                if self.input_broadcast_mode == InputBroadcastMode::CurrentTab
                    && self.input_broadcast_tab_id == target_tab_id
                {
                    self.input_broadcast_mode = InputBroadcastMode::Off;
                    self.input_broadcast_tab_id = None;
                    self.update_title();
                    self.show_toast(mode.disabled_toast().to_string());
                    return;
                }

                let target_count = target_tab_id
                    .map(|tab_id| self.input_broadcast_target_count_for_tab(tab_id))
                    .unwrap_or(0);
                if target_count < 2 {
                    self.show_toast(mode.unavailable_toast().to_string());
                    return;
                }

                self.input_broadcast_mode = InputBroadcastMode::CurrentTab;
                self.input_broadcast_tab_id = target_tab_id;
                self.update_title();
                self.show_toast(mode.enabled_toast(target_count));
            }
            InputBroadcastMode::AllTabs => {
                if self.input_broadcast_mode == InputBroadcastMode::AllTabs {
                    self.input_broadcast_mode = InputBroadcastMode::Off;
                    self.input_broadcast_tab_id = None;
                    self.update_title();
                    self.show_toast(mode.disabled_toast().to_string());
                    return;
                }

                let target_count = self.input_broadcast_target_count(mode);
                if target_count < 2 {
                    self.show_toast(mode.unavailable_toast().to_string());
                    return;
                }

                self.input_broadcast_mode = InputBroadcastMode::AllTabs;
                self.input_broadcast_tab_id = None;
                self.update_title();
                self.show_toast(mode.enabled_toast(target_count));
            }
        }
    }

    /// Returns a Pane that we can interact with; this will typically be
    /// the active tab for the window, but if the window has a tab-wide
    /// overlay (such as the launcher / tab navigator),
    /// then that will be returned instead.  Otherwise, if the pane has
    pub fn get_terminal_size(&self) -> TerminalSize {
        self.terminal_size
    }

    /// an active overlay (such as search or copy mode) then that will
    /// be returned.
    pub fn get_active_pane_or_overlay(&self) -> Option<Arc<dyn Pane>> {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return None,
        };

        let tab_id = tab.tab_id();

        if let Some(tab_overlay) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            Some(tab_overlay)
        } else {
            let pane = tab.get_active_pane()?;
            let pane_id = pane.pane_id();
            self.pane_state(pane_id)
                .overlay
                .as_ref()
                .map(|overlay| overlay.pane.clone())
                .or_else(|| Some(pane))
        }
    }

    fn get_splits(&mut self) -> Vec<PositionedSplit> {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return vec![],
        };

        let tab_id = tab.tab_id();

        if self.tab_state(tab_id).overlay.is_some() {
            vec![]
        } else {
            tab.iter_splits()
        }
    }

    fn pos_pane_to_pane_info(pos: &PositionedPane) -> PaneInformation {
        PaneInformation {
            pane_id: pos.pane.pane_id(),
            pane_index: pos.index,
            is_active: pos.is_active,
            is_zoomed: pos.is_zoomed,
            has_unseen_output: pos.pane.has_unseen_output(),
            left: pos.left,
            top: pos.top,
            width: pos.width,
            height: pos.height,
            pixel_width: pos.pixel_width,
            pixel_height: pos.pixel_height,
            title: pos.pane.get_title(),
            user_vars: pos.pane.copy_user_vars(),
            progress: pos.pane.get_progress(),
        }
    }

    fn get_tab_information(&mut self) -> Vec<TabInformation> {
        let mux = Mux::get();
        let window = match mux.get_window(self.mux_window_id) {
            Some(window) => window,
            _ => return vec![],
        };
        let tab_index = window.get_active_idx();

        window
            .iter()
            .enumerate()
            .map(|(idx, tab)| TabInformation {
                tab_index: idx,
                tab_id: tab.tab_id(),
                is_active: tab_index == idx,
                is_last_active: window
                    .get_last_active_idx()
                    .map(|last_active| last_active == idx)
                    .unwrap_or(false),
                window_id: self.mux_window_id,
                tab_title: tab.get_title(),
                active_pane: tab
                    .iter_panes()
                    .into_iter()
                    .find(|p| p.is_active)
                    .map(|p| Self::pos_pane_to_pane_info(&p)),
            })
            .collect()
    }

    fn get_pane_information(&self) -> Vec<PaneInformation> {
        self.get_panes_to_render()
            .iter()
            .map(Self::pos_pane_to_pane_info)
            .collect()
    }

    fn get_pos_panes_for_tab(&self, tab: &Arc<Tab>) -> Vec<PositionedPane> {
        let tab_id = tab.tab_id();

        if let Some(pane) = self
            .tab_state(tab_id)
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.clone())
        {
            let size = tab.get_size();
            vec![PositionedPane {
                index: 0,
                is_active: true,
                is_zoomed: false,
                left: 0,
                top: 0,
                width: size.cols as _,
                height: size.rows as _,
                pixel_width: size.cols as usize * self.render_metrics.cell_size.width as usize,
                pixel_height: size.rows as usize * self.render_metrics.cell_size.height as usize,
                pane,
            }]
        } else {
            let mut panes = tab.iter_panes();
            for p in &mut panes {
                self.sync_pane_viewport_state(&p.pane);
                if let Some(overlay) = self.pane_state(p.pane.pane_id()).overlay.as_ref() {
                    p.pane = Arc::clone(&overlay.pane);
                }
            }
            panes
        }
    }

    fn get_panes_to_render(&self) -> Vec<PositionedPane> {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return vec![],
        };

        self.get_pos_panes_for_tab(&tab)
    }

    /// if pane_id.is_none(), removes any overlay for the specified tab.
    /// Otherwise: if the overlay is the specified pane for that tab, remove it.
    fn cancel_overlay_for_tab(&mut self, tab_id: TabId, pane_id: Option<PaneId>) {
        if pane_id.is_some() {
            let current = self
                .tab_state(tab_id)
                .overlay
                .as_ref()
                .map(|o| o.pane.pane_id());
            if current != pane_id {
                return;
            }
        }
        if let Some(overlay) = self.tab_state(tab_id).overlay.take() {
            Mux::get().remove_pane(overlay.pane.pane_id());
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn schedule_cancel_overlay(window: Window, tab_id: TabId, pane_id: Option<PaneId>) {
        window.notify(TermWindowNotif::CancelOverlayForTab { tab_id, pane_id });
    }

    fn cancel_overlay_for_pane(&mut self, pane_id: PaneId) {
        if let Some(overlay) = self.pane_state(pane_id).overlay.take() {
            // Ungh, when I built the CopyOverlay, its pane doesn't get
            // added to the mux and instead it reports the overlaid
            // pane id.  Take care to avoid killing ourselves off
            // when closing the CopyOverlay
            if pane_id != overlay.pane.pane_id() {
                Mux::get().remove_pane(overlay.pane.pane_id());
            }
        }
        let was_chat = self.ai_chat_overlay_panes.remove(&pane_id).is_some();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        // The AI chat overlay uses a tighter bottom padding. When it closes,
        // re-run layout so the normal padding is restored immediately instead
        // of waiting for the next external resize event.
        if was_chat {
            if let Some(window) = self.window.clone() {
                let dims = self.dimensions.clone();
                self.apply_dimensions(&dims, None, &window, false);
            }
        }
    }

    pub fn schedule_cancel_overlay_for_pane(window: Window, pane_id: PaneId) {
        window.notify(TermWindowNotif::CancelOverlayForPane(pane_id));
    }

    pub fn assign_overlay_for_pane(&mut self, pane_id: PaneId, pane: Arc<dyn Pane>) {
        self.cancel_overlay_for_pane(pane_id);
        self.pane_state(pane_id).overlay.replace(OverlayState {
            pane,
            key_table_state: KeyTableState::default(),
        });
        self.update_title();
    }

    pub fn assign_overlay(&mut self, tab_id: TabId, overlay: Arc<dyn Pane>) {
        self.cancel_overlay_for_tab(tab_id, None);
        self.tab_state(tab_id).overlay.replace(OverlayState {
            pane: overlay,
            key_table_state: KeyTableState::default(),
        });
        self.update_title();
    }

    fn resolve_search_pattern(&self, pattern: Pattern, pane: &Arc<dyn Pane>) -> MuxPattern {
        match pattern {
            Pattern::CaseSensitiveString(s) => MuxPattern::CaseSensitiveString(s),
            Pattern::CaseInSensitiveString(s) => MuxPattern::CaseInSensitiveString(s),
            Pattern::Regex(s) => MuxPattern::Regex(s),
            Pattern::CurrentSelectionOrEmptyString => {
                let text = self.selection_text(pane);
                let first_line = text
                    .lines()
                    .next()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                MuxPattern::CaseSensitiveString(first_line)
            }
        }
    }
}

impl Drop for TermWindow {
    fn drop(&mut self) {
        self.clear_all_overlays();
        resize::clear_deferred_font_scale_pty_resize(self.mux_window_id);
        if let Some(window) = self.window.take() {
            if let Some(fe) = try_front_end() {
                fe.forget_known_window(&window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bell_notification_message, scrollback_erase_mode_for_pane, FileLinkTarget,
        InputBroadcastMode, MouseCapture, RenderableDimensions, TermWindow,
    };
    use config::keyassignment::ScrollbackEraseMode;
    use mux::pane::PaneId;
    use mux::tab::TabId;
    use std::path::PathBuf;
    use wezterm_term::StableRowIndex;

    #[test]
    fn other_user_vars_never_trigger_reload() {
        assert!(!TermWindow::should_reload_config_for_user_var(
            "SOME_OTHER_USER_VAR",
            true
        ));
    }

    #[test]
    fn clear_scrollback_keeps_alternate_screen_viewport_intact() {
        assert_eq!(
            scrollback_erase_mode_for_pane(ScrollbackEraseMode::ScrollbackAndViewport, true),
            ScrollbackEraseMode::ScrollbackOnly,
        );
    }

    #[test]
    fn clear_scrollback_still_clears_normal_screen_viewport() {
        assert_eq!(
            scrollback_erase_mode_for_pane(ScrollbackEraseMode::ScrollbackAndViewport, false),
            ScrollbackEraseMode::ScrollbackAndViewport,
        );
    }

    #[test]
    fn parse_file_location_extracts_line_and_column() {
        let (path, line, col) = TermWindow::parse_file_location("/tmp/demo.rs:12:34");
        assert_eq!(path, "/tmp/demo.rs");
        assert_eq!(line, Some(12));
        assert_eq!(col, Some(34));
    }

    #[test]
    fn parse_file_location_leaves_plain_paths_unchanged() {
        let (path, line, col) = TermWindow::parse_file_location("/tmp/demo.rs");
        assert_eq!(path, "/tmp/demo.rs");
        assert_eq!(line, None);
        assert_eq!(col, None);
    }

    #[test]
    fn parse_editor_command_extracts_program_and_flags() {
        let (program, args) =
            TermWindow::parse_editor_command(r#"code -g "/tmp/demo.rs:12:3""#).unwrap();
        assert_eq!(program, "code");
        assert_eq!(args, vec!["-g", "/tmp/demo.rs:12:3"]);
    }

    #[test]
    fn parse_editor_command_rejects_empty_value() {
        let err = TermWindow::parse_editor_command("   ").unwrap_err();
        assert!(err.to_string().contains("editor command is empty"));
    }

    #[test]
    fn configured_editor_candidates_cover_finder_launch_paths() {
        let zed = TermWindow::editor_program_candidates("zed");
        assert_eq!(zed.first().map(String::as_str), Some("zed"));
        assert!(zed.iter().any(|path| path == "/usr/local/bin/zed"));

        let cursor = TermWindow::editor_program_candidates("cursor");
        assert!(cursor
            .iter()
            .any(|path| { path == "/Applications/Cursor.app/Contents/Resources/app/bin/cursor" }));

        assert_eq!(
            TermWindow::editor_program_candidates("/custom/editor"),
            vec!["/custom/editor"]
        );
    }

    #[test]
    fn configured_editor_location_preserves_line_and_column() {
        let target = FileLinkTarget {
            path: PathBuf::from("/tmp/demo.rs"),
            line: Some(12),
            col: Some(3),
        };
        assert_eq!(TermWindow::file_link_location(&target), "/tmp/demo.rs:12:3");
    }

    #[test]
    fn configured_editor_location_uses_plain_path_without_line() {
        let target = FileLinkTarget {
            path: PathBuf::from("/tmp/demo.rs"),
            line: None,
            col: Some(3),
        };
        assert_eq!(TermWindow::file_link_location(&target), "/tmp/demo.rs");
    }

    #[test]
    fn current_tab_broadcast_only_applies_to_bound_tab() {
        let tab_a = TabId::from(1usize);
        let tab_b = TabId::from(2usize);

        assert!(InputBroadcastMode::CurrentTab.applies_to_active_tab(Some(tab_a), Some(tab_a)));
        assert!(!InputBroadcastMode::CurrentTab.applies_to_active_tab(Some(tab_a), Some(tab_b)));
        assert!(!InputBroadcastMode::CurrentTab.applies_to_active_tab(None, Some(tab_a)));
        assert!(InputBroadcastMode::AllTabs.applies_to_active_tab(Some(tab_a), Some(tab_b)));
        assert!(!InputBroadcastMode::Off.applies_to_active_tab(Some(tab_a), Some(tab_a)));
    }

    #[test]
    fn broadcast_input_requires_explicit_mode_active_pane_and_multiple_targets() {
        let tab_a = TabId::from(1usize);
        let tab_b = TabId::from(2usize);

        assert!(!InputBroadcastMode::Off.should_broadcast_input(Some(tab_a), Some(tab_a), true, 2));
        assert!(!InputBroadcastMode::AllTabs.should_broadcast_input(None, Some(tab_a), false, 2));
        assert!(!InputBroadcastMode::AllTabs.should_broadcast_input(None, Some(tab_a), true, 1));
        assert!(InputBroadcastMode::AllTabs.should_broadcast_input(None, Some(tab_a), true, 2));
        assert!(!InputBroadcastMode::CurrentTab.should_broadcast_input(
            Some(tab_a),
            Some(tab_b),
            true,
            2
        ));
        assert!(InputBroadcastMode::CurrentTab.should_broadcast_input(
            Some(tab_a),
            Some(tab_a),
            true,
            2
        ));
    }

    fn dims(physical_top: StableRowIndex, scrollback_top: StableRowIndex) -> RenderableDimensions {
        RenderableDimensions {
            cols: 80,
            viewport_rows: 24,
            scrollback_rows: 200,
            physical_top,
            scrollback_top,
            dpi: 96,
            pixel_width: 800,
            pixel_height: 480,
            reverse_video: false,
        }
    }

    #[test]
    fn normalize_viewport_snaps_to_bottom_on_small_pruning() {
        // Even a small prune (here 10 rows, within one viewport page) snaps to
        // bottom (None) rather than clamping to the moving scrollback_top, which
        // read as a jarring "jump to the top" during continuous output (#448).
        assert_eq!(
            TermWindow::normalize_viewport(Some(90), dims(150, 100)),
            None
        );
    }

    #[test]
    fn normalize_viewport_snaps_to_bottom_on_large_pruning() {
        // Any pruning of the viewport position snaps to bottom regardless of how
        // far the content scrolled off the top.
        assert_eq!(
            TermWindow::normalize_viewport(Some(90), dims(200, 150)),
            None
        );
    }

    #[test]
    fn normalize_viewport_clears_when_position_reaches_bottom() {
        assert_eq!(
            TermWindow::normalize_viewport(Some(150), dims(150, 100)),
            None
        );
        assert_eq!(
            TermWindow::normalize_viewport(Some(180), dims(150, 100)),
            None
        );
    }

    #[test]
    fn normalize_viewport_preserves_scroll_until_output_prunes_it() {
        assert_eq!(
            TermWindow::normalize_viewport(Some(120), dims(140, 100)),
            Some(120)
        );
        // Once scrollback_top advances past the viewport, snap to bottom.
        assert_eq!(
            TermWindow::normalize_viewport(Some(120), dims(150, 121)),
            None
        );
    }

    #[test]
    fn reconcile_viewport_clears_stale_peek_viewport_on_exit() {
        assert_eq!(
            TermWindow::reconcile_viewport(Some(20), true, false, false, dims(40, 0)),
            None
        );
        // A peek exit remains authoritative while a selection drag is active.
        assert_eq!(
            TermWindow::reconcile_viewport(Some(20), true, false, true, dims(40, 0)),
            None
        );
    }

    #[test]
    fn interactive_viewport_clamps_page_up_past_scrollback_top() {
        let page_up_target = 110isize.saturating_sub(24);
        assert_eq!(
            TermWindow::normalize_interactive_viewport(Some(page_up_target), dims(150, 100)),
            Some(100)
        );
        // Positions still inside scrollback are preserved.
        assert_eq!(
            TermWindow::normalize_interactive_viewport(Some(120), dims(150, 100)),
            Some(120)
        );
    }

    #[test]
    fn interactive_viewport_follows_bottom_when_nothing_left_to_pin() {
        // No scrollback remains (scrollback_top == physical_top): follow live
        // output.
        assert_eq!(
            TermWindow::normalize_interactive_viewport(Some(90), dims(150, 150)),
            None
        );
        // Bottom-follow position stays bottom-follow.
        assert_eq!(
            TermWindow::normalize_interactive_viewport(Some(150), dims(150, 100)),
            None
        );
        assert_eq!(
            TermWindow::normalize_interactive_viewport(None, dims(150, 100)),
            None
        );
    }

    #[test]
    fn selection_drag_only_controls_the_captured_pane() {
        let captured_pane = PaneId::new(1);
        let sibling_pane = PaneId::new(2);
        let capture = MouseCapture::TerminalPane(captured_pane);

        assert!(TermWindow::selection_drag_controls_pane(
            true,
            Some(&capture),
            captured_pane
        ));
        assert!(!TermWindow::selection_drag_controls_pane(
            true,
            Some(&capture),
            sibling_pane
        ));
        assert!(!TermWindow::selection_drag_controls_pane(
            false,
            Some(&capture),
            captured_pane
        ));
        assert!(!TermWindow::selection_drag_controls_pane(
            true,
            Some(&MouseCapture::UI),
            captured_pane
        ));
    }

    #[test]
    fn bell_notification_message_prefers_last_command() {
        assert_eq!(
            bell_notification_message(
                Some("cargo test -p kaku-gui"),
                Some("zsh"),
                "kaku",
                Some("/bin/zsh")
            ),
            "Task complete: cargo test"
        );
    }

    #[test]
    fn bell_notification_message_uses_reported_program_before_default_title() {
        assert_eq!(
            bell_notification_message(None, Some("npm run build"), "kaku", None),
            "Task complete: npm run build"
        );
    }

    #[test]
    fn bell_notification_message_falls_back_to_process_basename() {
        assert_eq!(
            bell_notification_message(None, None, "kaku", Some("/opt/homebrew/bin/git")),
            "Task complete: git"
        );
    }

    #[test]
    fn bell_notification_message_uses_background_fallback_for_uninformative_values() {
        assert_eq!(
            bell_notification_message(Some("   "), Some("wezterm"), "kaku", None),
            "Background task complete"
        );
    }

    #[test]
    fn bell_notification_message_summarizes_shell_wrapped_command() {
        assert_eq!(
            bell_notification_message(Some("zsh -lc cargo check -p kaku-gui"), None, "kaku", None),
            "Task complete: cargo check"
        );
    }

    #[test]
    fn bell_notification_message_ignores_weak_short_source() {
        assert_eq!(
            bell_notification_message(None, None, "vo", None),
            "Background task complete"
        );
    }

    #[test]
    fn bell_notification_message_handles_sudo_with_user_flag() {
        assert_eq!(
            bell_notification_message(Some("sudo -u root cargo build"), None, "kaku", None),
            "Task complete: cargo build"
        );
    }

    #[test]
    fn bell_notification_message_handles_docker_compose_up() {
        assert_eq!(
            bell_notification_message(Some("docker compose up -d"), None, "kaku", None),
            "Task complete: docker compose up"
        );
    }

    #[test]
    fn bell_notification_message_skips_decorative_symbols_in_title() {
        assert_eq!(
            bell_notification_message(None, None, "\u{273B} claude", None),
            "Task complete: claude"
        );
    }

    #[test]
    fn bell_notification_message_falls_back_when_title_is_only_symbols() {
        assert_eq!(
            bell_notification_message(None, None, "\u{273B}", None),
            "Background task complete"
        );
    }
}
