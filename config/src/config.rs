use crate::background::{BackgroundLayer, Gradient};
use crate::bell::{AudibleBell, EasingFunction, VisualBell};
use crate::color::{
    ColorSchemeFile, HsbTransform, Palette, SrgbaTuple, TabBarStyle, WindowFrameConfig,
};
use crate::daemon::DaemonOptions;
use crate::exec_domain::ExecDomain;
use crate::font::{
    AllowSquareGlyphOverflow, DisplayPixelGeometry, FontLocatorSelection, FontRasterizerSelection,
    FontShaperSelection, FreeTypeLoadFlags, FreeTypeLoadTarget, StyleRule, TextStyle,
};
use crate::frontend::FrontEndSelection;
use crate::keyassignment::{
    KeyAssignment, KeyTable, KeyTableEntry, KeyTables, MouseEventTrigger, PaneEncoding,
    SpawnCommand,
};
use crate::keys::{DeferredKeyCode, Key, KeyNoAction, LeaderKey, Mouse};
use crate::lua::make_lua_context;
use crate::ssh::{SshBackend, SshDomain};
use crate::tls::{TlsDomainClient, TlsDomainServer};
use crate::units::Dimension;
use crate::unix::UnixDomain;
use crate::wsl::WslDomain;
use crate::{
    default_config_with_overrides_applied, default_one_point_oh, default_one_point_oh_f64,
    default_true, default_win32_acrylic_accent_color, CellWidth, GpuInfo,
    IntegratedTitleButtonColor, KeyMapPreference, LoadedConfig, MouseEventTriggerMods, RgbaColor,
    SerialDomain, SystemBackdrop, WebGpuPowerPreference, CONFIG_DIRS, CONFIG_FILE_OVERRIDE,
    CONFIG_OVERRIDES, CONFIG_SKIP,
};
use anyhow::Context;
use luahelper::impl_lua_conversion_dynamic;
use mlua::FromLua;
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;
use termwiz::hyperlink;
use termwiz::surface::CursorShape;
use wezterm_bidi::ParagraphDirectionHint;
use wezterm_config_derive::ConfigMeta;
use wezterm_dynamic::{FromDynamic, ToDynamic};
use wezterm_input_types::{
    IntegratedTitleButton, IntegratedTitleButtonAlignment, IntegratedTitleButtonStyle, Modifiers,
    UIKeyCapRendering, WindowDecorations,
};
use wezterm_term::TerminalSize;

#[derive(Debug, Clone, FromDynamic, ToDynamic, ConfigMeta)]
pub struct Config {
    /// The font size, measured in points
    #[dynamic(default = "default_font_size")]
    pub font_size: f64,

    #[dynamic(
        default = "default_one_point_oh_f64",
        validate = "validate_line_height"
    )]
    pub line_height: f64,

    #[dynamic(default = "default_one_point_oh_f64")]
    pub cell_width: f64,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub cursor_thickness: Option<Dimension>,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub underline_thickness: Option<Dimension>,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub underline_position: Option<Dimension>,

    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub strikethrough_position: Option<Dimension>,

    #[dynamic(default)]
    pub allow_square_glyphs_to_overflow_width: AllowSquareGlyphOverflow,

    #[dynamic(default)]
    pub window_decorations: WindowDecorations,

    #[dynamic(default = "default_integrated_title_buttons")]
    pub integrated_title_buttons: Vec<IntegratedTitleButton>,

    #[dynamic(default)]
    pub log_unknown_escape_sequences: bool,

    #[dynamic(default)]
    pub integrated_title_button_alignment: IntegratedTitleButtonAlignment,

    #[dynamic(default)]
    pub integrated_title_button_style: IntegratedTitleButtonStyle,

    #[dynamic(default)]
    pub integrated_title_button_color: IntegratedTitleButtonColor,

    /// When using FontKitXXX font systems, a set of directories to
    /// search ahead of the standard font locations for fonts.
    /// Relative paths are taken to be relative to the directory
    /// from which the config was loaded.
    #[dynamic(default)]
    pub font_dirs: Vec<PathBuf>,

    #[dynamic(default)]
    pub color_scheme_dirs: Vec<PathBuf>,

    /// The DPI to assume
    pub dpi: Option<f64>,

    #[dynamic(default)]
    pub dpi_by_screen: HashMap<String, f64>,

    /// The baseline font to use
    #[dynamic(default)]
    pub font: TextStyle,

    /// An optional set of style rules to select the font based
    /// on the cell attributes
    #[dynamic(default)]
    pub font_rules: Vec<StyleRule>,

    /// When true (the default), PaletteIndex 0-7 are shifted to
    /// bright when the font intensity is bold.  The brightening
    /// doesn't apply to text that is the default color.
    #[dynamic(default)]
    pub bold_brightens_ansi_colors: BoldBrightening,

    /// The color palette
    pub colors: Option<Palette>,

    #[dynamic(default)]
    pub switch_to_last_active_tab_when_closing_tab: bool,

    /// When true, launching a new wezterm instance will prefer
    /// to spawn a new tab into an existing instance.
    /// Otherwise, it will spawn a new window.
    #[dynamic(default)]
    pub prefer_to_spawn_tabs: bool,

    #[dynamic(default = "default_true")]
    pub restore_previous_session: bool,

    #[dynamic(default)]
    pub window_frame: WindowFrameConfig,

    /// Font to use for CharSelect
    #[dynamic(default)]
    pub char_select_font: Option<TextStyle>,

    #[dynamic(default = "default_char_select_font_size")]
    pub char_select_font_size: f64,

    #[dynamic(default = "default_char_select_fg_color")]
    pub char_select_fg_color: RgbaColor,

    #[dynamic(default = "default_char_select_bg_color")]
    pub char_select_bg_color: RgbaColor,

    /// Font to use for ActivateCommandPalette
    #[dynamic(default)]
    pub command_palette_font: Option<TextStyle>,

    #[dynamic(default = "default_command_palette_font_size")]
    pub command_palette_font_size: f64,

    pub command_palette_rows: Option<usize>,
    #[dynamic(default = "default_command_palette_fg_color")]
    pub command_palette_fg_color: RgbaColor,

    #[dynamic(default = "default_command_palette_bg_color")]
    pub command_palette_bg_color: RgbaColor,

    /// Font to use for PaneSelect
    #[dynamic(default)]
    pub pane_select_font: Option<TextStyle>,

    #[dynamic(default = "default_pane_select_font_size")]
    pub pane_select_font_size: f64,

    #[dynamic(default = "default_pane_select_fg_color")]
    pub pane_select_fg_color: RgbaColor,

    #[dynamic(default = "default_pane_select_bg_color")]
    pub pane_select_bg_color: RgbaColor,

    #[dynamic(default)]
    pub tab_bar_style: TabBarStyle,

    #[dynamic(default)]
    pub resolved_palette: Palette,

    /// Use a named color scheme rather than the palette specified
    /// by the colors setting.
    pub color_scheme: Option<String>,

    /// Named color schemes
    #[dynamic(default)]
    pub color_schemes: HashMap<String, Palette>,

    /// How many lines of scrollback you want to retain
    #[dynamic(
        default = "default_scrollback_lines",
        validate = "validate_scrollback_lines"
    )]
    pub scrollback_lines: usize,

    /// If no `prog` is specified on the command line, use this
    /// instead of running the user's shell.
    /// For example, to have `wezterm` always run `top` by default,
    /// you'd use this:
    ///
    /// ```toml
    /// default_prog = ["top"]
    /// ```
    ///
    /// `default_prog` is implemented as an array where the 0th element
    /// is the command to run and the rest of the elements are passed
    /// as the positional arguments to that command.
    pub default_prog: Option<Vec<String>>,

    #[dynamic(default = "default_gui_startup_args")]
    pub default_gui_startup_args: Vec<String>,

    /// Specifies the default current working directory if none is specified
    /// through configuration or OSC 7 (see docs for `default_cwd` for more
    /// info!)
    pub default_cwd: Option<PathBuf>,

    /// When true, new windows inherit the cwd from the active pane if possible.
    #[dynamic(default = "default_true")]
    pub window_inherit_working_directory: bool,

    /// When true, new tabs inherit the cwd from the active pane if possible.
    #[dynamic(default = "default_true")]
    pub tab_inherit_working_directory: bool,

    /// When true, new split panes inherit the cwd from the source pane if possible.
    #[dynamic(default = "default_true")]
    pub split_pane_inherit_working_directory: bool,

    #[dynamic(default = "default_pane_encoding")]
    pub default_encoding: PaneEncoding,

    #[dynamic(default)]
    pub exit_behavior: ExitBehavior,

    #[dynamic(default)]
    pub exit_behavior_messaging: ExitBehaviorMessaging,

    #[dynamic(default = "default_clean_exits")]
    pub clean_exit_codes: Vec<u32>,

    #[dynamic(default = "default_true")]
    pub detect_password_input: bool,

    /// Specifies a map of environment variables that should be set
    /// when spawning commands in the local domain.
    /// This is not used when working with remote domains.
    #[dynamic(default)]
    pub set_environment_variables: HashMap<String, String>,

    /// Controls how the Tab key behaves in zsh inside Kaku sessions.
    /// The environment variables `KAKU_SMART_TAB_DISABLE` and
    /// `KAKU_TAB_ACCEPT_SUGGEST_FIRST` are set automatically based on this.
    #[dynamic(default)]
    pub smart_tab_mode: SmartTabMode,

    /// Specifies the height of a new window, expressed in character cells.
    #[dynamic(default = "default_initial_rows", validate = "validate_row_or_col")]
    pub initial_rows: u16,

    #[dynamic(default = "default_true")]
    pub enable_kitty_graphics: bool,
    #[dynamic(default)]
    pub enable_kitty_keyboard: bool,

    /// Whether the terminal should respond to requests to read the
    /// title string.
    /// Disabled by default for security concerns with shells that might
    /// otherwise attempt to execute the response.
    /// <https://marc.info/?l=bugtraq&m=104612710031920&w=2>
    #[dynamic(default)]
    pub enable_title_reporting: bool,

    /// Specifies the width of a new window, expressed in character cells
    #[dynamic(default = "default_initial_cols", validate = "validate_row_or_col")]
    pub initial_cols: u16,

    #[dynamic(default = "default_hyperlink_rules")]
    pub hyperlink_rules: Vec<hyperlink::Rule>,

    /// Optional command used to open local file links. Kaku appends the
    /// resolved path and, when present, its line and column.
    pub file_link_editor: Option<String>,

    /// What to set the TERM variable to
    #[dynamic(default = "default_term")]
    pub term: String,

    #[dynamic(default)]
    pub font_locator: FontLocatorSelection,
    #[dynamic(default)]
    pub font_rasterizer: FontRasterizerSelection,
    #[dynamic(default = "default_colr_rasterizer")]
    pub font_colr_rasterizer: FontRasterizerSelection,
    #[dynamic(default)]
    pub font_shaper: FontShaperSelection,

    #[dynamic(default)]
    pub display_pixel_geometry: DisplayPixelGeometry,
    #[dynamic(default)]
    pub freetype_load_target: FreeTypeLoadTarget,
    #[dynamic(default)]
    pub freetype_render_target: Option<FreeTypeLoadTarget>,
    #[dynamic(default)]
    pub freetype_load_flags: Option<FreeTypeLoadFlags>,

    /// Selects the freetype interpret version to use.
    /// Likely values are 35, 38 and 40 which have different
    /// characteristics with respective to subpixel hinting.
    /// See https://freetype.org/freetype2/docs/subpixel-hinting.html
    pub freetype_interpreter_version: Option<u32>,

    #[dynamic(default)]
    pub freetype_pcf_long_family_names: bool,

    /// Specify the features to enable when using harfbuzz for font shaping.
    /// There is some light documentation here:
    /// <https://harfbuzz.github.io/shaping-opentype-features.html>
    /// but it boils down to allowing opentype feature names to be specified
    /// using syntax similar to the CSS font-feature-settings options:
    /// <https://developer.mozilla.org/en-US/docs/Web/CSS/font-feature-settings>.
    /// The OpenType spec lists a number of features here:
    /// <https://docs.microsoft.com/en-us/typography/opentype/spec/featurelist>
    ///
    /// Options of likely interest will be:
    ///
    /// * `calt` - <https://docs.microsoft.com/en-us/typography/opentype/spec/features_ae#tag-calt>
    /// * `clig` - <https://docs.microsoft.com/en-us/typography/opentype/spec/features_ae#tag-clig>
    ///
    /// If you want to disable ligatures in most fonts, then you may want to
    /// use a setting like this:
    ///
    /// ```toml
    /// harfbuzz_features = ["calt=0", "clig=0", "liga=0"]
    /// ```
    ///
    /// Some fonts make available extended options via stylistic sets.
    /// If you use the [Fira Code font](https://github.com/tonsky/FiraCode),
    /// it lists available stylistic sets here:
    /// <https://github.com/tonsky/FiraCode/wiki/How-to-enable-stylistic-sets>
    ///
    /// and you can set them in wezterm:
    ///
    /// ```toml
    /// # Use this for a zero with a dot rather than a line through it
    /// # when using the Fira Code font
    /// harfbuzz_features = ["zero"]
    /// ```
    #[dynamic(default = "default_harfbuzz_features")]
    pub harfbuzz_features: Vec<String>,

    #[dynamic(default)]
    pub front_end: FrontEndSelection,

    /// Whether to select the higher powered discrete GPU when
    /// the system has a choice of integrated or discrete.
    /// Defaults to low power.
    #[dynamic(default)]
    pub webgpu_power_preference: WebGpuPowerPreference,

    #[dynamic(default)]
    pub webgpu_force_fallback_adapter: bool,

    #[dynamic(default)]
    pub webgpu_preferred_adapter: Option<GpuInfo>,

    #[dynamic(default)]
    pub wsl_domains: Option<Vec<WslDomain>>,

    #[dynamic(default)]
    pub exec_domains: Vec<ExecDomain>,

    #[dynamic(default)]
    pub serial_ports: Vec<SerialDomain>,

    /// The set of unix domains
    #[dynamic(default = "UnixDomain::default_unix_domains")]
    pub unix_domains: Vec<UnixDomain>,

    #[dynamic(default)]
    pub ssh_domains: Option<Vec<SshDomain>>,

    #[dynamic(default)]
    pub ssh_backend: SshBackend,

    /// When running in server mode, defines configuration for
    /// each of the endpoints that we'll listen for connections
    #[dynamic(default)]
    pub tls_servers: Vec<TlsDomainServer>,

    /// The set of tls domains that we can connect to as a client
    #[dynamic(default)]
    pub tls_clients: Vec<TlsDomainClient>,

    /// Constrains the rate at which the multiplexer client will
    /// speculatively fetch line data.
    /// This helps to avoid saturating the link between the client
    /// and server if the server is dumping a large amount of output
    /// to the client.
    #[dynamic(default = "default_ratelimit_line_prefetches_per_second")]
    pub ratelimit_mux_line_prefetches_per_second: u32,

    /// The buffer size used by parse_buffered_data in the mux module.
    /// This should not be too large, otherwise the processing cost
    /// of applying a batch of actions to the terminal will be too
    /// high and the user experience will be laggy and less responsive.
    #[dynamic(default = "default_mux_output_parser_buffer_size")]
    pub mux_output_parser_buffer_size: usize,

    #[dynamic(default = "default_true")]
    pub mux_enable_ssh_agent: bool,

    #[dynamic(default)]
    pub default_ssh_auth_sock: Option<String>,

    /// How many ms to delay after reading a chunk of output
    /// in order to try to coalesce fragmented writes into
    /// a single bigger chunk of output and reduce the chances
    /// observing "screen tearing" with un-synchronized output
    #[dynamic(default = "default_mux_output_parser_coalesce_delay_ms")]
    pub mux_output_parser_coalesce_delay_ms: u64,

    /// Maximum idle time in milliseconds while holding synchronized output
    /// (mode 2026) before force-flushing. Active output resets the timer so a
    /// slow remote frame remains atomic, while a missing end marker still
    /// cannot block rendering indefinitely. Set to 0 to disable the timeout.
    #[dynamic(default = "default_mux_synchronized_output_timeout_ms")]
    pub mux_synchronized_output_timeout_ms: u64,

    #[dynamic(default = "default_mux_env_remove")]
    pub mux_env_remove: Vec<String>,

    #[dynamic(default)]
    pub keys: Vec<Key>,
    #[dynamic(default)]
    pub key_tables: HashMap<String, Vec<Key>>,

    #[dynamic(default = "default_bypass_mouse_reporting_modifiers")]
    pub bypass_mouse_reporting_modifiers: Modifiers,

    #[dynamic(default)]
    pub debug_key_events: bool,

    #[dynamic(default)]
    pub normalize_output_to_unicode_nfc: bool,

    #[dynamic(default)]
    pub disable_default_key_bindings: bool,
    pub leader: Option<LeaderKey>,

    #[dynamic(default = "default_num_alphabet")]
    pub launcher_alphabet: String,

    #[dynamic(default)]
    pub disable_default_quick_select_patterns: bool,
    #[dynamic(default)]
    pub quick_select_patterns: Vec<String>,
    #[dynamic(default = "default_alphabet")]
    pub quick_select_alphabet: String,
    #[dynamic(default)]
    pub quick_select_remove_styling: bool,

    #[dynamic(default)]
    pub mouse_bindings: Vec<Mouse>,
    #[dynamic(default)]
    pub disable_default_mouse_bindings: bool,
    /// When false, completing a mouse text selection will not copy text
    /// to the clipboard. Kaku may show a one-time in-window hint so the
    /// selection behavior is less surprising.
    #[dynamic(default = "default_true")]
    pub copy_on_select: bool,

    #[dynamic(default)]
    pub daemon_options: DaemonOptions,

    #[dynamic(default)]
    pub send_composed_key_when_left_alt_is_pressed: bool,

    #[dynamic(default = "default_true")]
    pub send_composed_key_when_right_alt_is_pressed: bool,

    #[dynamic(default = "default_macos_forward_mods")]
    pub macos_forward_to_ime_modifier_mask: Modifiers,

    /// Global hotkey to show or hide Kaku on macOS.
    /// Set this to nil to disable the system-wide hotkey.
    #[dynamic(default = "default_macos_global_hotkey")]
    pub macos_global_hotkey: Option<KeyNoAction>,

    /// Reveal the global hotkey window on the display containing the mouse.
    /// When false, keep using AppKit's main display.
    #[dynamic(default)]
    pub macos_global_hotkey_follow_mouse: bool,

    /// Omitted sides inherit window_padding; the display safe area is added separately.
    #[dynamic(default)]
    pub macos_global_hotkey_padding: Option<HotkeyWindowPadding>,

    #[dynamic(try_from = "Option<MacosGlobalHotkeyAppearance>", default)]
    pub macos_global_hotkey_appearance: MacosGlobalHotkeyAppearance,

    #[dynamic(default)]
    pub macos_global_hotkey_mode: MacosGlobalHotkeyMode,

    #[dynamic(
        default = "default_hotkey_height_percent",
        validate = "validate_hotkey_height_percent"
    )]
    pub macos_global_hotkey_height_percent: f64,

    #[dynamic(
        default = "default_hotkey_animation_duration_ms",
        validate = "validate_hotkey_animation_duration_ms"
    )]
    pub macos_global_hotkey_animation_duration_ms: u64,

    #[dynamic(default = "default_true")]
    pub macos_global_hotkey_hide_on_focus_loss: bool,

    /// None inherits macos_fullscreen_extend_behind_notch.
    #[dynamic(default)]
    pub macos_global_hotkey_extend_behind_notch: Option<bool>,

    #[dynamic(default)]
    pub treat_left_ctrlalt_as_altgr: bool,

    /// If true, the `Backspace` and `Delete` keys generate `Delete` and `Backspace`
    /// keypresses, respectively, rather than their normal keycodes.
    /// On macOS the default for this is true because its Backspace key
    /// is labeled as Delete and things are backwards.
    #[dynamic(default = "default_swap_backspace_and_delete")]
    pub swap_backspace_and_delete: bool,

    /// If true, display the tab bar UI at the top of the window.
    /// The tab bar shows the titles of the tabs and which is the
    /// active tab.  Clicking on a tab activates it.
    #[dynamic(default = "default_true")]
    pub enable_tab_bar: bool,
    #[dynamic(default = "default_true")]
    pub use_fancy_tab_bar: bool,

    #[dynamic(default)]
    pub tab_bar_at_bottom: bool,

    /// If true, auto-generated tab titles use only the current folder name
    /// instead of the default parent/current path pair.
    #[dynamic(default)]
    pub tab_title_show_basename_only: bool,

    /// If true, auto-generated tab titles include the foreground process
    /// alongside the path, for example `project·codex`.
    #[dynamic(default)]
    pub tab_title_show_foreground_process: bool,

    /// Prefer the active pane's OSC 0/2 title over auto-generated directory titles.
    /// Explicitly renamed tabs still take precedence.
    #[dynamic(default)]
    pub tab_title_use_pane_title: bool,

    #[dynamic(default = "default_true")]
    pub mouse_wheel_scrolls_tabs: bool,

    /// If true, tab bar titles are prefixed with the tab index
    #[dynamic(default)]
    pub show_tab_index_in_tab_bar: bool,

    #[dynamic(default = "default_true")]
    pub show_tabs_in_tab_bar: bool,

    #[dynamic(default = "default_true")]
    pub show_new_tab_button_in_tab_bar: bool,

    #[dynamic(default = "default_true")]
    pub show_close_tab_button_in_tabs: bool,

    /// If true, show_tab_index_in_tab_bar uses a zero-based index.
    /// The default is false and the tab shows a one-based index.
    #[dynamic(default)]
    pub tab_and_split_indices_are_zero_based: bool,

    /// Specifies the maximum width that a tab can have in the
    /// tab bar.  Defaults to 16 glyphs in width.
    #[dynamic(default = "default_tab_max_width")]
    pub tab_max_width: usize,

    /// If true, hide the tab bar if the window only has a single tab.
    #[dynamic(default)]
    pub hide_tab_bar_if_only_one_tab: bool,

    #[dynamic(default)]
    pub enable_scroll_bar: bool,

    /// When true, mouse wheel events in alternate-screen apps such as nano
    /// and vim are sent to the app instead of scrolling Kaku's primary
    /// scrollback peek.
    #[dynamic(default)]
    pub alternate_screen_wheel_scrolls_terminal: bool,

    /// Controls how the mouse wheel behaves while a left-button terminal
    /// selection drag is in progress.
    ///
    /// - `Extend` (default, matches macOS `NSTextView` apps like Safari /
    ///   TextEdit / VS Code): scroll the scrollback and extend the selection
    ///   so it follows the cursor across screens.
    /// - `ScrollOnly`: scroll the scrollback but leave the selection range
    ///   unchanged.
    /// - `Ignore`: drop the wheel event entirely. Restores the legacy Kaku
    ///   behavior from v0.10 and earlier, which kept the selection stable but
    ///   blocked cross-screen text selection.
    #[dynamic(default)]
    pub selection_wheel_scroll_behavior: SelectionWheelScrollBehavior,

    /// Deprecated and ignored. The i18n support was removed and Kaku's
    /// built-in UI is English-only. Kept as a deprecated field (rather than
    /// dropped) so that `kaku.lua` files carrying `config.language` from
    /// V0.11.0 still load on upgrade with a warning instead of a hard error.
    #[dynamic(
        default,
        deprecated = "the language option was removed; Kaku's built-in UI is English-only"
    )]
    pub language: String,

    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub min_scroll_bar_height: Dimension,

    /// If false, do not try to use a Wayland protocol connection
    /// when starting the gui frontend, and instead use X11.
    /// This option is only considered on X11/Wayland systems and
    /// has no effect on macOS or Windows.
    /// The default is true.
    #[dynamic(default = "default_true")]
    pub enable_wayland: bool,
    #[dynamic(default)]
    pub enable_zwlr_output_manager: bool,

    /// Whether to prefer EGL over other GL implementations.
    /// EGL on Windows has jankier resize behavior than WGL (which
    /// is used if EGL is unavailable), but EGL survives graphics
    /// driver updates without breaking and losing your work.
    #[dynamic(default = "default_prefer_egl")]
    pub prefer_egl: bool,

    #[dynamic(default = "default_true")]
    pub custom_block_glyphs: bool,
    #[dynamic(default = "default_true")]
    pub anti_alias_custom_block_glyphs: bool,

    /// Controls the amount of padding to use around the terminal cell area
    #[dynamic(default)]
    pub window_padding: WindowPadding,

    /// Number of extra cells of padding added on each side of a split line.
    /// The split gutter width becomes `1 + 2 * split_pane_gap` cells.
    /// A value of 2 gives ~40px visual breathing room at typical font sizes,
    /// matching the outer window padding aesthetic without clipping content.
    #[dynamic(default)]
    pub split_pane_gap: u8,

    /// The thickness of the split line in pixels.
    /// Defaults to 2.0 pixels.
    #[dynamic(default = "default_split_thickness")]
    pub split_thickness: f32,

    #[dynamic(default)]
    pub window_content_alignment: WindowContentAlignment,

    /// Specifies the path to a background image attachment file.
    /// The file can be any image format that the rust `image`
    /// crate is able to identify and load.
    /// A window background image is rendered into the background
    /// of the window before any other content.
    ///
    /// The image will be scaled to fit the window.
    #[dynamic(default)]
    pub window_background_image: Option<PathBuf>,
    #[dynamic(default)]
    pub window_background_gradient: Option<Gradient>,
    #[dynamic(default)]
    pub window_background_image_hsb: Option<HsbTransform>,
    #[dynamic(default)]
    pub foreground_text_hsb: HsbTransform,

    #[dynamic(default)]
    pub background: Vec<BackgroundLayer>,

    /// Only works on MacOS
    #[dynamic(default)]
    pub macos_window_background_blur: i64,

    /// Only works on KDE Wayland
    #[dynamic(default)]
    pub kde_window_background_blur: bool,

    /// Only works on Windows
    #[dynamic(default)]
    pub win32_system_backdrop: SystemBackdrop,

    #[dynamic(default = "default_win32_acrylic_accent_color")]
    pub win32_acrylic_accent_color: RgbaColor,

    /// Specifies the alpha value to use when rendering the background
    /// of the window.  The background is taken either from the
    /// window_background_image, or if there is none, the background
    /// color of the cell in the current position.
    /// The default is 1.0 which is 100% opaque.  Setting it to a number
    /// between 0.0 and 1.0 will allow for the screen behind the window
    /// to "shine through" to varying degrees.
    /// This only works on systems with a compositing window manager.
    /// Setting opacity to a value other than 1.0 can impact render
    /// performance.
    #[dynamic(default = "default_one_point_oh")]
    pub window_background_opacity: f32,

    /// inactive_pane_hue, inactive_pane_saturation and
    /// inactive_pane_brightness allow for transforming the color
    /// of inactive panes.
    /// The pane colors are converted to HSV values and multiplied
    /// by these values before being converted back to RGB to
    /// use in the display.
    ///
    /// The default is 1.0 which leaves the values as-is.
    ///
    /// Modifying the hue changes the hue of the color by rotating
    /// it through the color wheel.  It is not as useful as the
    /// other components, but is available "for free" as part of
    /// the colorspace conversion.
    ///
    /// Modifying the saturation can add or reduce the amount of
    /// "colorfulness".  Making the value smaller can make it appear
    /// more washed out.
    ///
    /// Modifying the brightness can be used to dim or increase
    /// the perceived amount of light.
    ///
    /// The range of these values is 0.0 and up; they are used to
    /// multiply the existing values, so the default of 1.0
    /// preserves the existing component, whilst 0.5 will reduce
    /// it by half, and 2.0 will double the value.
    ///
    /// A subtle dimming effect can be achieved by setting:
    /// inactive_pane_saturation = 0.9
    /// inactive_pane_brightness = 0.8
    #[dynamic(default = "default_inactive_pane_hsb")]
    pub inactive_pane_hsb: HsbTransform,

    #[dynamic(default = "default_one_point_oh")]
    pub text_background_opacity: f32,

    /// Specifies how often a blinking cursor transitions between visible
    /// and invisible, expressed in milliseconds.
    /// Setting this to 0 disables blinking.
    /// Note that this value is approximate due to the way that the system
    /// event loop schedulers manage timers; non-zero values will be at
    /// least the interval specified with some degree of slop.
    #[dynamic(default = "default_cursor_blink_rate")]
    pub cursor_blink_rate: u64,
    #[dynamic(default = "linear_ease")]
    pub cursor_blink_ease_in: EasingFunction,
    #[dynamic(default = "linear_ease")]
    pub cursor_blink_ease_out: EasingFunction,

    #[dynamic(default = "default_anim_fps")]
    pub animation_fps: u8,

    #[dynamic(default = "default_text_min_contrast_ratio")]
    pub text_min_contrast_ratio: Option<f32>,

    #[dynamic(default)]
    pub force_reverse_video_cursor: bool,
    #[dynamic(default = "default_reverse_video_cursor_min_contrast")]
    pub reverse_video_cursor_min_contrast: f32,

    /// Specifies the default cursor style.  various escape sequences
    /// can override the default style in different situations (eg:
    /// an editor can change it depending on the mode), but this value
    /// controls how the cursor appears when it is reset to default.
    /// The default is `SteadyBlock`.
    /// Acceptable values are `SteadyBlock`, `BlinkingBlock`,
    /// `SteadyUnderline`, `BlinkingUnderline`, `SteadyBar`,
    /// and `BlinkingBar`.
    #[dynamic(default)]
    pub default_cursor_style: DefaultCursorStyle,

    /// Specifies how often blinking text (normal speed) transitions
    /// between visible and invisible, expressed in milliseconds.
    /// Setting this to 0 disables slow text blinking.  Note that this
    /// value is approximate due to the way that the system event loop
    /// schedulers manage timers; non-zero values will be at least the
    /// interval specified with some degree of slop.
    #[dynamic(default = "default_text_blink_rate")]
    pub text_blink_rate: u64,
    #[dynamic(default = "linear_ease")]
    pub text_blink_ease_in: EasingFunction,
    #[dynamic(default = "linear_ease")]
    pub text_blink_ease_out: EasingFunction,

    /// Specifies how often blinking text (rapid speed) transitions
    /// between visible and invisible, expressed in milliseconds.
    /// Setting this to 0 disables rapid text blinking.  Note that this
    /// value is approximate due to the way that the system event loop
    /// schedulers manage timers; non-zero values will be at least the
    /// interval specified with some degree of slop.
    #[dynamic(default = "default_text_blink_rate_rapid")]
    pub text_blink_rate_rapid: u64,
    #[dynamic(default = "linear_ease")]
    pub text_blink_rapid_ease_in: EasingFunction,
    #[dynamic(default = "linear_ease")]
    pub text_blink_rapid_ease_out: EasingFunction,

    /// If true, the mouse cursor will be hidden while typing.
    /// This option is true by default.
    #[dynamic(default = "default_true")]
    pub hide_mouse_cursor_when_typing: bool,

    /// If non-zero, specifies the period (in seconds) at which various
    /// statistics are logged.  Note that there is a minimum period of
    /// 10 seconds.
    #[dynamic(default)]
    pub periodic_stat_logging: u64,

    /// If false, do not scroll to the bottom of the terminal when
    /// you send input to the terminal.
    /// The default is to scroll to the bottom when you send input
    /// to the terminal.
    #[dynamic(default = "default_true")]
    pub scroll_to_bottom_on_input: bool,

    #[dynamic(default = "default_true")]
    pub use_ime: bool,
    #[dynamic(default)]
    pub xim_im_name: Option<String>,
    #[dynamic(default)]
    pub ime_preedit_rendering: ImePreeditRendering,

    #[dynamic(default)]
    pub notification_handling: NotificationHandling,

    #[dynamic(default = "default_true")]
    pub use_dead_keys: bool,

    #[dynamic(default)]
    pub launch_menu: Vec<SpawnCommand>,

    #[dynamic(default)]
    pub use_box_model_render: bool,

    /// When true, watch the config file and reload it automatically
    /// when it is detected as changing.
    #[dynamic(default = "default_true")]
    pub automatically_reload_config: bool,

    #[dynamic(default = "default_check_for_updates")]
    pub check_for_updates: bool,
    #[dynamic(
        default,
        deprecated = "this option no longer does anything and will be removed in a future release"
    )]
    pub show_update_window: bool,

    #[dynamic(default = "default_update_interval")]
    pub check_for_updates_interval_seconds: u64,

    /// When set to true, use the CSI-U encoding scheme as described
    /// in http://www.leonerd.org.uk/hacks/fixterms/
    /// This is off by default because @wez and @jsgf find the shift-space
    /// mapping annoying in vim :-p
    #[dynamic(default)]
    pub enable_csi_u_key_encoding: bool,

    #[dynamic(default)]
    pub window_close_confirmation: WindowCloseConfirmation,

    /// Controls confirmation before closing a tab.
    #[dynamic(default)]
    pub tab_close_confirmation: CloseConfirmation,

    /// Controls confirmation before closing a pane.
    #[dynamic(default)]
    pub pane_close_confirmation: CloseConfirmation,

    #[dynamic(default)]
    pub native_macos_fullscreen_mode: bool,

    #[dynamic(default)]
    pub macos_fullscreen_extend_behind_notch: bool,

    #[dynamic(default = "default_word_boundary")]
    pub selection_word_boundary: String,

    /// When true, copying a selection that spans rows filled to the terminal
    /// width will join those rows without a newline. This recovers single-line
    /// commands that TUI apps (codex, cursor-cli, etc.) visually broke across
    /// rows by filling each row to the right edge themselves.
    /// Defaults to false to avoid silently removing newlines from legitimate
    /// fixed-width content (tables, logs, records) that naturally end at column N.
    #[dynamic(default)]
    pub copy_unwrap_tui_lines: bool,

    #[dynamic(default)]
    pub copy_strip_leading_whitespace: bool,

    #[dynamic(default = "default_enq_answerback")]
    pub enq_answerback: String,

    #[dynamic(default)]
    pub adjust_window_size_when_changing_font_size: Option<bool>,

    #[dynamic(default = "default_tiling_desktop_environments")]
    pub tiling_desktop_environments: Vec<String>,

    #[dynamic(default)]
    pub use_resize_increments: bool,

    #[dynamic(default = "default_alternate_buffer_wheel_scroll_speed")]
    pub alternate_buffer_wheel_scroll_speed: u8,

    #[dynamic(default = "default_status_update_interval")]
    pub status_update_interval: u64,

    #[dynamic(default)]
    pub experimental_pixel_positioning: bool,

    #[dynamic(default)]
    pub ignore_svg_fonts: bool,

    #[dynamic(default)]
    pub bidi_enabled: bool,

    #[dynamic(default)]
    pub bidi_direction: ParagraphDirectionHint,

    #[dynamic(default = "default_stateless_process_list")]
    pub skip_close_confirmation_for_processes_named: Vec<String>,

    #[dynamic(default = "default_quit_when_all_windows_are_closed")]
    pub quit_when_all_windows_are_closed: bool,

    #[dynamic(default = "default_true")]
    pub warn_about_missing_glyphs: bool,

    #[dynamic(default)]
    pub sort_fallback_fonts_by_coverage: bool,

    #[dynamic(default)]
    pub search_font_dirs_for_fallback: bool,

    #[dynamic(default)]
    pub use_cap_height_to_scale_fallback_fonts: bool,

    #[dynamic(default)]
    pub swallow_mouse_click_on_pane_focus: bool,

    #[dynamic(default = "default_swallow_mouse_click_on_window_focus")]
    pub swallow_mouse_click_on_window_focus: bool,

    #[dynamic(default)]
    pub pane_focus_follows_mouse: bool,

    #[dynamic(default = "default_true")]
    pub unzoom_on_switch_pane: bool,

    #[dynamic(default = "default_max_fps")]
    pub max_fps: u64,

    #[dynamic(default = "default_shape_cache_size")]
    pub shape_cache_size: usize,
    #[dynamic(default = "default_line_state_cache_size")]
    pub line_state_cache_size: usize,
    #[dynamic(default = "default_line_quad_cache_size")]
    pub line_quad_cache_size: usize,
    #[dynamic(default = "default_line_to_ele_shape_cache_size")]
    pub line_to_ele_shape_cache_size: usize,
    #[dynamic(default = "default_glyph_cache_image_cache_size")]
    pub glyph_cache_image_cache_size: usize,

    #[dynamic(default)]
    pub visual_bell: VisualBell,

    #[dynamic(default)]
    pub audible_bell: AudibleBell,

    /// Show a dot indicator on inactive tabs with unread bell events
    #[dynamic(default = "default_true")]
    pub bell_tab_indicator: bool,

    /// Show a badge on the Dock icon when bell fires in unfocused window
    #[dynamic(default)]
    pub bell_dock_badge: bool,

    /// Restore the last working directory when opening new tabs or windows
    #[dynamic(default = "default_true")]
    pub remember_last_cwd: bool,

    #[dynamic(default)]
    pub canonicalize_pasted_newlines: Option<NewlineCanon>,

    #[dynamic(default = "default_unicode_version")]
    pub unicode_version: u8,

    #[dynamic(default)]
    pub treat_east_asian_ambiguous_width_as_wide: bool,

    #[dynamic(default)]
    pub cell_widths: Option<Vec<CellWidth>>,

    #[dynamic(default = "default_true")]
    pub allow_download_protocols: bool,

    #[dynamic(default = "default_true")]
    pub allow_win32_input_mode: bool,

    #[dynamic(default)]
    pub default_domain: Option<String>,

    #[dynamic(default)]
    pub default_mux_server_domain: Option<String>,

    #[dynamic(default)]
    pub default_workspace: Option<String>,

    #[dynamic(default)]
    pub xcursor_theme: Option<String>,

    #[dynamic(default)]
    pub xcursor_size: Option<u32>,

    #[dynamic(default)]
    pub key_map_preference: KeyMapPreference,

    #[dynamic(default)]
    pub quote_dropped_files: DroppedFileQuoting,

    #[dynamic(default)]
    pub ui_key_cap_rendering: UIKeyCapRendering,

    #[dynamic(default = "default_one")]
    pub palette_max_key_assigments_for_action: usize,

    #[dynamic(default = "default_ulimit_nofile")]
    pub ulimit_nofile: u64,

    #[dynamic(default = "default_ulimit_nproc")]
    pub ulimit_nproc: u64,

    /// Configuration for the Kaku Remote iOS bridge.
    /// When enabled, a WebSocket server is started so the iOS app can
    /// view and control panes over the local network.
    #[dynamic(default)]
    pub remote: RemoteConfig,
}
impl_lua_conversion_dynamic!(Config);

#[derive(Debug, Clone, FromDynamic, ToDynamic, ConfigMeta)]
pub struct RemoteConfig {
    /// Whether the remote bridge is enabled.
    #[dynamic(default = "default_remote_enabled")]
    pub enabled: bool,

    /// The TCP port to listen on.
    #[dynamic(default = "default_remote_port")]
    pub port: u16,

    /// The address to bind. Use "0.0.0.0" for all interfaces (LAN access)
    /// or "127.0.0.1" for local-only.
    #[dynamic(default = "default_remote_bind")]
    pub bind: String,

    /// Enable outbound relay tunnel so the phone can connect from outside LAN.
    #[dynamic(default = "default_remote_tunnel")]
    pub tunnel: bool,

    /// Relay server WebSocket URL.
    #[dynamic(default = "default_tunnel_url")]
    pub tunnel_url: String,
}
impl_lua_conversion_dynamic!(RemoteConfig);

fn default_remote_enabled() -> bool {
    false
}

fn default_remote_port() -> u16 {
    9988
}

fn default_remote_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_tunnel_url() -> String {
    "wss://kaku-relay.fly.dev".to_string()
}

fn default_remote_tunnel() -> bool {
    true
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_remote_port(),
            bind: default_remote_bind(),
            tunnel: default_remote_tunnel(),
            tunnel_url: default_tunnel_url(),
        }
    }
}

fn default_one() -> usize {
    1
}

fn default_ulimit_nofile() -> u64 {
    2048
}

fn default_ulimit_nproc() -> u64 {
    2048
}

impl Default for Config {
    fn default() -> Self {
        // Ask FromDynamic to provide the defaults based on the attributes
        // specified in the struct so that we don't have to repeat
        // the same thing in a different form down here
        Config::from_dynamic(
            &wezterm_dynamic::Value::Object(Default::default()),
            Default::default(),
        )
        .unwrap()
    }
}

impl Config {
    pub fn load() -> LoadedConfig {
        Self::load_with_overrides(&wezterm_dynamic::Value::default())
    }

    /// It is relatively expensive to parse all the ssh config files,
    /// so we defer producing the default list until someone explicitly
    /// asks for it
    pub fn ssh_domains(&self) -> Vec<SshDomain> {
        if let Some(domains) = &self.ssh_domains {
            domains.clone()
        } else {
            SshDomain::default_domains()
        }
    }

    pub fn wsl_domains(&self) -> Vec<WslDomain> {
        if let Some(domains) = &self.wsl_domains {
            domains.clone()
        } else {
            WslDomain::default_domains()
        }
    }

    pub fn update_ulimit(&self) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            use nix::sys::resource::{getrlimit, rlim_t, setrlimit, Resource};
            use std::convert::TryInto;

            let (no_file_soft, no_file_hard) = getrlimit(Resource::RLIMIT_NOFILE)?;

            let ulimit_nofile: rlim_t = self.ulimit_nofile.try_into().with_context(|| {
                format!(
                    "ulimit_nofile value {} is out of range for this system",
                    self.ulimit_nofile
                )
            })?;

            if no_file_soft < ulimit_nofile {
                setrlimit(
                    Resource::RLIMIT_NOFILE,
                    ulimit_nofile.min(no_file_hard),
                    no_file_hard,
                )
                .with_context(|| {
                    format!(
                        "raise RLIMIT_NOFILE from {no_file_soft} to ulimit_nofile {}",
                        ulimit_nofile
                    )
                })?;
            }
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use nix::sys::resource::{getrlimit, rlim_t, setrlimit, Resource};
            use std::convert::TryInto;

            let (nproc_soft, nproc_hard) = getrlimit(Resource::RLIMIT_NPROC)?;

            let ulimit_nproc: rlim_t = self.ulimit_nproc.try_into().with_context(|| {
                format!(
                    "ulimit_nproc value {} is out of range for this system",
                    self.ulimit_nproc
                )
            })?;

            if nproc_soft < ulimit_nproc {
                setrlimit(
                    Resource::RLIMIT_NPROC,
                    ulimit_nproc.min(nproc_hard),
                    nproc_hard,
                )
                .with_context(|| {
                    format!(
                        "raise RLIMIT_NPROC from {nproc_soft} to ulimit_nproc {}",
                        ulimit_nproc
                    )
                })?;
            }
        }

        Ok(())
    }

    pub fn load_with_overrides(overrides: &wezterm_dynamic::Value) -> LoadedConfig {
        Self::load_with_window_overrides(overrides, false)
    }

    pub(crate) fn load_hotkey_with_overrides(overrides: &wezterm_dynamic::Value) -> LoadedConfig {
        Self::load_with_window_overrides(overrides, true)
    }

    fn load_with_window_overrides(
        overrides: &wezterm_dynamic::Value,
        hotkey: bool,
    ) -> LoadedConfig {
        // Note that the directories crate has methods for locating project
        // specific config directories, but only returns one of them, not
        // multiple.  In addition, it spawns a lot of subprocesses,
        // so we do this bit "by-hand"

        let mut paths = vec![];
        for dir in CONFIG_DIRS.iter() {
            paths.push(PathPossibility::optional(dir.join("kaku.lua")))
        }

        if cfg!(windows) {
            // On Windows, a common use case is to maintain a thumb drive
            // with a set of portable tools that don't need to be installed
            // to run on a target system.  In that scenario, the user would
            // like to run with the config from their thumbdrive because
            // either the target system won't have any config, or will have
            // the config of another user.
            // So we prioritize that here: if there is a config in the same
            // dir as the executable that will take precedence.
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(exe_dir) = exe_name.parent() {
                    paths.insert(0, PathPossibility::optional(exe_dir.join("kaku.lua")));
                }
            }
        }

        if cfg!(target_os = "macos") {
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(contents_dir) = exe_name.parent().and_then(|p| p.parent()) {
                    paths.push(PathPossibility::optional(
                        contents_dir.join("Resources").join("kaku.lua"),
                    ));
                }
            }
        }

        if let Some(path) = CONFIG_FILE_OVERRIDE.lock().unwrap().as_ref() {
            log::trace!("Note: config file override is set");
            paths.insert(0, PathPossibility::required(path.clone()));
        }

        for path_item in &paths {
            if CONFIG_SKIP.load(Ordering::Relaxed) {
                break;
            }

            match Self::try_load(path_item, overrides, hotkey) {
                Err(err) => {
                    return LoadedConfig {
                        config: Err(err),
                        file_name: Some(path_item.path.clone()),
                        lua: None,
                        warnings: vec![],
                    };
                }
                Ok(None) => continue,
                Ok(Some(loaded)) => return loaded,
            }
        }

        // We didn't find (or were asked to skip) a kaku.lua file, so
        // update the environment to make it simpler to understand this
        // state.
        std::env::remove_var("KAKU_CONFIG_FILE");
        std::env::remove_var("KAKU_CONFIG_DIR");

        let default = if hotkey {
            Self::try_default_hotkey(overrides)
        } else {
            Self::try_default()
        };
        match default {
            Err(err) => LoadedConfig {
                config: Err(err),
                file_name: None,
                lua: None,
                warnings: vec![],
            },
            Ok(cfg) => cfg,
        }
    }

    pub fn try_default() -> anyhow::Result<LoadedConfig> {
        let (config, warnings) =
            wezterm_dynamic::Error::capture_warnings(|| -> anyhow::Result<Config> {
                Ok(default_config_with_overrides_applied()?.compute_extra_defaults(None))
            });

        let loaded = LoadedConfig {
            config: Ok(config?),
            file_name: None,
            lua: Some(make_lua_context(Path::new(""))?),
            warnings,
        };
        Ok(loaded)
    }

    fn try_default_hotkey(overrides: &wezterm_dynamic::Value) -> anyhow::Result<LoadedConfig> {
        let lua = make_lua_context(Path::new(""))?;
        let (config, warnings) = wezterm_dynamic::Error::capture_warnings(|| {
            let raw = mlua::Value::Table(lua.create_table()?);
            let raw = Self::apply_overrides_to(&lua, raw)?;
            Self::finish_hotkey_config(&lua, raw, overrides, None)
        });
        Ok(LoadedConfig {
            config: Ok(config?),
            file_name: None,
            lua: Some(lua),
            warnings,
        })
    }

    /// Runtime signature embedded in every cache entry.
    /// Changing the Kaku version automatically invalidates all cached bytecode,
    /// preventing cross-version mismatches.
    const CACHE_SIGNATURE: &'static str = concat!("kaku/", env!("CARGO_PKG_VERSION"), "/lua54");

    /// Magic header for the bytecode cache file format.
    const CACHE_MAGIC: &'static [u8; 4] = b"KLBC";

    /// Compute the bytecode cache path for a given config source file.
    fn bytecode_cache_path(source: &Path) -> PathBuf {
        // Use a stable hash of the source path to avoid collisions.
        // SipHasher24 is version-stable, unlike DefaultHasher.
        let hash = {
            use siphasher::sip::SipHasher24;
            use std::hash::{Hash, Hasher};
            let mut hasher = SipHasher24::new();
            source.hash(&mut hasher);
            hasher.finish()
        };
        crate::CACHE_DIR.join(format!("lua_bytecode_{:016x}.bin", hash))
    }

    /// Hash the content of a source file for cache validation.
    fn source_content_hash(content: &[u8]) -> u64 {
        use siphasher::sip::SipHasher24;
        use std::hash::{Hash, Hasher};
        let mut hasher = SipHasher24::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Encode bytecode with a validation header:
    ///   MAGIC(4) | SIG_LEN(2 LE) | SIGNATURE(SIG_LEN) | SRC_HASH(8 LE) | BYTECODE
    fn encode_cache(source_content: &[u8], bytecode: &[u8]) -> Vec<u8> {
        let sig = Self::CACHE_SIGNATURE.as_bytes();
        let sig_len = sig.len() as u16;
        let src_hash = Self::source_content_hash(source_content);

        let mut buf = Vec::with_capacity(4 + 2 + sig.len() + 8 + bytecode.len());
        buf.extend_from_slice(Self::CACHE_MAGIC);
        buf.extend_from_slice(&sig_len.to_le_bytes());
        buf.extend_from_slice(sig);
        buf.extend_from_slice(&src_hash.to_le_bytes());
        buf.extend_from_slice(bytecode);
        buf
    }

    /// Decode and validate cache header; returns raw bytecode on success.
    fn decode_cache(source_content: &[u8], data: &[u8]) -> Option<Vec<u8>> {
        // Parse magic
        if data.get(..4) != Some(Self::CACHE_MAGIC) {
            log::trace!("bytecode cache: bad magic, invalidating");
            return None;
        }
        let data = &data[4..];

        // Parse signature length + value
        if data.len() < 2 {
            return None;
        }
        let sig_len = u16::from_le_bytes([data[0], data[1]]) as usize;
        let data = &data[2..];
        if data.len() < sig_len {
            return None;
        }
        let cached_sig = &data[..sig_len];
        if cached_sig != Self::CACHE_SIGNATURE.as_bytes() {
            log::trace!("bytecode cache: signature mismatch, invalidating");
            return None;
        }
        let data = &data[sig_len..];

        // Parse source content hash
        if data.len() < 8 {
            return None;
        }
        let mut hash_bytes = [0u8; 8];
        hash_bytes.copy_from_slice(&data[..8]);
        let cached_hash = u64::from_le_bytes(hash_bytes);
        let expected_hash = Self::source_content_hash(source_content);
        if cached_hash != expected_hash {
            log::trace!("bytecode cache: source hash mismatch, invalidating");
            return None;
        }

        Some(data[8..].to_vec())
    }

    /// Try to load bytecode from cache, validating header + source hash.
    /// The source-content hash inside the header is the authoritative
    /// freshness check; an mtime pre-check would false-miss whenever cache
    /// and source land in the same filesystem timestamp granule (e.g. an
    /// edit followed by an immediate launch).
    pub(crate) fn try_load_bytecode_cache(source: &Path, source_content: &[u8]) -> Option<Vec<u8>> {
        let cache_path = Self::bytecode_cache_path(source);
        let data = std::fs::read(&cache_path).ok()?;
        Self::decode_cache(source_content, &data)
    }

    /// Save compiled bytecode to the cache with validation header.
    pub(crate) fn save_bytecode_cache(source: &Path, source_content: &[u8], bytecode: &[u8]) {
        let cache_path = Self::bytecode_cache_path(source);
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let encoded = Self::encode_cache(source_content, bytecode);
        if let Err(err) = std::fs::write(&cache_path, &encoded) {
            log::trace!("failed to write bytecode cache: {:#}", err);
        }
    }

    fn try_load(
        path_item: &PathPossibility,
        overrides: &wezterm_dynamic::Value,
        hotkey: bool,
    ) -> anyhow::Result<Option<LoadedConfig>> {
        let p = path_item.path.as_path();
        log::trace!("consider config: {}", p.display());
        let mut file = match std::fs::File::open(p) {
            Ok(file) => file,
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound if !path_item.is_required => return Ok(None),
                _ => anyhow::bail!("Error opening {}: {}", p.display(), err),
            },
        };

        let mut s = String::new();
        file.read_to_string(&mut s)?;
        let trace = std::env::var_os("KAKU_STARTUP_TRACE").is_some();
        let t0 = std::time::Instant::now();
        let lua = make_lua_context(p)?;
        if trace {
            eprintln!(
                "[startup:config] make_lua_context: {:.3}ms",
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }

        // Try loading from bytecode cache first
        let source_bytes = s.as_bytes();
        let t1 = std::time::Instant::now();
        let cached_bytecode = Self::try_load_bytecode_cache(p, source_bytes);
        if trace {
            eprintln!(
                "[startup:config] bytecode_cache_lookup: {:.3}ms (hit={})",
                t1.elapsed().as_secs_f64() * 1000.0,
                cached_bytecode.is_some()
            );
        }

        let (config, warnings) =
            wezterm_dynamic::Error::capture_warnings(|| -> anyhow::Result<Config> {
                let source_text = s.trim_start_matches('\u{FEFF}');

                let map_lua_err = |e: mlua::Error| -> anyhow::Error {
                    let err_str = format!("{}", e);
                    if err_str.contains("attempt to index a nil value")
                        && err_str.contains("global 'config'")
                    {
                        anyhow::anyhow!(
                            "Config error: You may have forgotten to define the config variable.\n\
                             \n\
                             In kaku.lua, you need to create the config table first:\n\
                             \n\
                             local wezterm = require 'wezterm'\n\
                             local config = {{}}  -- or wezterm.config_builder()\n\
                             \n\
                             config.line_height = 1.2\n\
                             config.font_size = 14.0\n\
                             \n\
                             return config\n\
                             \n\
                             Original error: {}",
                            e
                        )
                    } else {
                        anyhow::anyhow!("{}", e)
                    }
                };

                let t2 = std::time::Instant::now();
                let config: mlua::Value = if let Some(ref bytecode) = cached_bytecode {
                    match smol::block_on(
                        lua.load(bytecode.as_slice())
                            .set_name(p.to_string_lossy())
                            .eval_async::<mlua::Value>(),
                    ) {
                        Ok(val) => val,
                        Err(_) => {
                            // Cache is corrupt or incompatible, fall back to source
                            log::trace!("bytecode cache miss, loading from source");
                            smol::block_on(
                                lua.load(source_text)
                                    .set_name(p.to_string_lossy())
                                    .eval_async::<mlua::Value>(),
                            )
                            .map_err(&map_lua_err)?
                        }
                    }
                } else {
                    // No cache, load from source and dump bytecode for next time.
                    // Compile to a function first so we can extract bytecode.
                    let func = lua
                        .load(source_text)
                        .set_name(p.to_string_lossy())
                        .into_function()
                        .map_err(&map_lua_err)?;
                    let bytecode = func.dump(true);
                    Self::save_bytecode_cache(p, source_bytes, &bytecode);
                    smol::block_on(func.call_async::<_, mlua::Value>(())).map_err(&map_lua_err)?
                };
                if trace {
                    eprintln!(
                        "[startup:config] lua_eval: {:.3}ms",
                        t2.elapsed().as_secs_f64() * 1000.0
                    );
                }

                let config = Config::apply_overrides_to(&lua, config)?;
                let config = if hotkey {
                    Self::apply_hotkey_to(&lua, config, overrides)?
                } else {
                    config
                };
                let config = Config::apply_overrides_obj_to(&lua, config, overrides)?;
                let t3 = std::time::Instant::now();
                let cfg = Config::from_lua(config, &lua).with_context(|| {
                    format!(
                        "Error converting lua value returned by script {} to Config struct",
                        p.display()
                    )
                })?;
                if trace {
                    eprintln!(
                        "[startup:config] Config::from_lua: {:.3}ms",
                        t3.elapsed().as_secs_f64() * 1000.0
                    );
                }
                cfg.check_consistency()?;

                std::env::set_var("KAKU_CONFIG_FILE", p);
                if let Some(dir) = p.parent() {
                    std::env::set_var("KAKU_CONFIG_DIR", dir);
                }
                Ok(cfg)
            });
        let cfg = config?;
        let cfg = cfg.compute_extra_defaults(Some(p));

        Ok(Some(LoadedConfig {
            config: Ok(cfg),
            file_name: Some(p.to_path_buf()),
            lua: Some(lua),
            warnings,
        }))
    }

    pub(crate) fn apply_overrides_obj_to<'l>(
        lua: &'l mlua::Lua,
        mut config: mlua::Value<'l>,
        overrides: &wezterm_dynamic::Value,
    ) -> anyhow::Result<mlua::Value<'l>> {
        // config may be a table, or it may be a config builder.
        // We'll leave it up to lua to call the appropriate
        // index function as managing that from Rust is a PITA.
        let setter: mlua::Function = lua
            .load(
                r#"
                    return function(config, key, value)
                        config[key] = value;
                        return config;
                    end
                    "#,
            )
            .eval()?;

        match overrides {
            wezterm_dynamic::Value::Object(obj) => {
                for (key, value) in obj {
                    let key = luahelper::dynamic_to_lua_value(lua, key.clone())?;
                    let value = luahelper::dynamic_to_lua_value(lua, value.clone())?;
                    config = setter.call((config, key, value))?;
                }
                Ok(config)
            }
            _ => Ok(config),
        }
    }

    fn apply_hotkey_to<'l>(
        lua: &'l mlua::Lua,
        config: mlua::Value<'l>,
        overrides: &wezterm_dynamic::Value,
    ) -> anyhow::Result<mlua::Value<'l>> {
        // Runtime hooks may replace or clear the hotkey layer itself. Plain
        // window fields are applied later, so they retain highest priority.
        let mut layer = wezterm_dynamic::Object::default();
        if let wezterm_dynamic::Value::Object(overrides) = overrides {
            for key in [
                "macos_global_hotkey_padding",
                "macos_global_hotkey_appearance",
            ] {
                if let Some(value) = overrides.get_by_str(key) {
                    layer.insert(wezterm_dynamic::Value::String(key.into()), value.clone());
                }
            }
        }
        let config =
            Self::apply_overrides_obj_to(lua, config, &wezterm_dynamic::Value::Object(layer))?;
        let get: mlua::Function = lua
            .load("return function(config, key) return config[key] end")
            .eval()?;
        let options = wezterm_dynamic::FromDynamicOptions {
            unknown_fields: wezterm_dynamic::UnknownFieldAction::Deny,
            ..Default::default()
        };
        let appearance = luahelper::lua_value_to_dynamic(
            get.call((config.clone(), "macos_global_hotkey_appearance"))?,
        )?;
        let appearance = Option::<MacosGlobalHotkeyAppearance>::from_dynamic(&appearance, options)?
            .unwrap_or_default();
        let wezterm_dynamic::Value::Object(mut fields) = appearance.to_dynamic() else {
            unreachable!("typed appearance is an object")
        };
        fields.retain(|_, value| !matches!(value, wezterm_dynamic::Value::Null));

        if let Some(colors) = &appearance.colors {
            let base = luahelper::lua_value_to_dynamic(get.call((config.clone(), "colors"))?)?;
            let base = Option::<Palette>::from_dynamic(&base, options)?.unwrap_or_default();
            fields.insert(
                wezterm_dynamic::Value::String("colors".into()),
                base.overlay_with(colors).to_dynamic(),
            );
        }

        let padding = luahelper::lua_value_to_dynamic(
            get.call((config.clone(), "macos_global_hotkey_padding"))?,
        )?;
        if let Some(padding) = Option::<HotkeyWindowPadding>::from_dynamic(&padding, options)? {
            let base =
                luahelper::lua_value_to_dynamic(get.call((config.clone(), "window_padding"))?)?;
            let base = if matches!(base, wezterm_dynamic::Value::Null) {
                WindowPadding::default()
            } else {
                WindowPadding::from_dynamic(&base, options)?
            };
            fields.insert(
                wezterm_dynamic::Value::String("window_padding".into()),
                padding.resolve(&base).to_dynamic(),
            );
        }
        Self::apply_overrides_obj_to(lua, config, &wezterm_dynamic::Value::Object(fields))
    }

    fn finish_hotkey_config(
        lua: &mlua::Lua,
        raw: mlua::Value,
        overrides: &wezterm_dynamic::Value,
        config_path: Option<&Path>,
    ) -> anyhow::Result<Config> {
        let raw = Self::apply_hotkey_to(lua, raw, overrides)?;
        let raw = Self::apply_overrides_obj_to(lua, raw, overrides)?;
        let config = Self::from_lua(raw, lua)?;
        config.check_consistency()?;
        Ok(config.compute_extra_defaults(config_path))
    }

    pub(crate) fn apply_overrides_to<'l>(
        lua: &'l mlua::Lua,
        mut config: mlua::Value<'l>,
    ) -> anyhow::Result<mlua::Value<'l>> {
        let overrides = CONFIG_OVERRIDES.lock().unwrap();
        for (key, value) in &*overrides {
            if value == "nil" {
                // Literal nil as the value is the same as not specifying the value.
                // We special case this here as we want to explicitly check for
                // the value evaluating as nil, as can happen in the case where the
                // user specifies something like: `--config term=xterm`.
                // The RHS references a global that doesn't exist and evaluates as
                // nil. We want to raise this as an error.
                continue;
            }
            let literal = value.escape_debug();
            let code = format!(
                r#"
                local wezterm = require 'wezterm';
                local value = {value};
                if value == nil then
                    error("{literal} evaluated as nil. Check for missing quotes or other syntax issues")
                end
                config.{key} = value;
                return config;
                "#,
            );
            let chunk = lua.load(&code);
            let chunk = chunk.set_name(format!("--config {}={}", key, value));
            lua.globals().set("config", config.clone())?;
            log::debug!("Apply {}={} to config", key, value);
            config = chunk.eval()?;
        }
        Ok(config)
    }

    /// Check for logical conflicts in the config
    pub fn check_consistency(&self) -> anyhow::Result<()> {
        validate_hotkey_height_percent(&self.macos_global_hotkey_height_percent)
            .map_err(anyhow::Error::msg)?;
        validate_hotkey_animation_duration_ms(&self.macos_global_hotkey_animation_duration_ms)
            .map_err(anyhow::Error::msg)?;
        self.check_domain_consistency()?;
        Ok(())
    }

    fn check_domain_consistency(&self) -> anyhow::Result<()> {
        let mut domains = HashMap::new();

        let mut check_domain = |name: &str, kind: &str| {
            if let Some(exists) = domains.get(name) {
                anyhow::bail!(
                    "{kind} with name \"{name}\" conflicts with \
                     another existing {exists} with the same name"
                );
            }
            domains.insert(name.to_string(), kind.to_string());
            Ok(())
        };

        for d in &self.unix_domains {
            check_domain(&d.name, "unix domain")?;
        }
        if let Some(domains) = &self.ssh_domains {
            for d in domains {
                check_domain(&d.name, "ssh domain")?;
            }
        }
        for d in &self.exec_domains {
            check_domain(&d.name, "exec domain")?;
        }
        if let Some(domains) = &self.wsl_domains {
            for d in domains {
                check_domain(&d.name, "wsl domain")?;
            }
        }
        for d in &self.tls_clients {
            check_domain(&d.name, "tls domain")?;
        }
        Ok(())
    }

    pub fn default_config() -> Self {
        Self::default().compute_extra_defaults(None)
    }

    pub fn key_bindings(&self) -> KeyTables {
        let mut tables = KeyTables::default();

        for k in &self.keys {
            let (key, mods) = k
                .key
                .key
                .resolve(self.key_map_preference)
                .normalize_shift(k.key.mods);
            tables.default.insert(
                (key, mods),
                KeyTableEntry {
                    action: k.action.clone(),
                },
            );
        }

        for (name, keys) in &self.key_tables {
            let mut table = KeyTable::default();
            for k in keys {
                let (key, mods) = k
                    .key
                    .key
                    .resolve(self.key_map_preference)
                    .normalize_shift(k.key.mods);
                table.insert(
                    (key, mods),
                    KeyTableEntry {
                        action: k.action.clone(),
                    },
                );
            }
            tables.by_name.insert(name.to_string(), table);
        }

        tables
    }

    pub fn mouse_bindings(
        &self,
    ) -> HashMap<(MouseEventTrigger, MouseEventTriggerMods), KeyAssignment> {
        let mut map = HashMap::new();

        for m in &self.mouse_bindings {
            map.insert((m.event.clone(), m.mods), m.action.clone());
        }

        map
    }

    /// In some cases we need to compute expanded values based
    /// on those provided by the user.  This is where we do that.
    pub fn compute_extra_defaults(&self, config_path: Option<&Path>) -> Self {
        let mut cfg = self.clone();

        // Convert any relative font dirs to their config file relative locations
        if let Some(config_dir) = config_path.as_ref().and_then(|p| p.parent()) {
            for font_dir in &mut cfg.font_dirs {
                if !font_dir.is_absolute() {
                    let dir = config_dir.join(&font_dir);
                    *font_dir = dir;
                }
            }

            if let Some(path) = &self.window_background_image {
                if !path.is_absolute() {
                    cfg.window_background_image.replace(config_dir.join(path));
                }
            }
        }

        // Add some reasonable default font rules
        let reduced = self.font.reduce_first_font_to_family();

        let italic = reduced.make_italic();

        let bold = reduced.make_bold();
        let bold_italic = bold.make_italic();

        // Half intensity keeps the base weight: the renderer already dims
        // the foreground color, and synthesizing a lighter weight resolves
        // a real Thin/Light face on families that ship one, which reads as
        // broken rendering rather than as dim text (#481). The bundled font
        // stack encodes the same choice in its explicit font_rules.
        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(wezterm_term::Intensity::Half),
            font: italic.clone(),
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(false),
            intensity: Some(wezterm_term::Intensity::Half),
            font: reduced,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(false),
            intensity: Some(wezterm_term::Intensity::Bold),
            font: bold,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(wezterm_term::Intensity::Bold),
            font: bold_italic,
            ..Default::default()
        });

        cfg.font_rules.push(StyleRule {
            italic: Some(true),
            intensity: Some(wezterm_term::Intensity::Normal),
            font: italic,
            ..Default::default()
        });

        // Only scan color scheme directories from disk when the user
        // references a scheme not already defined inline.  This avoids
        // directory enumeration + TOML parsing on every startup for users
        // who don't use custom .toml color scheme files.
        if let Some(scheme_name) = cfg.color_scheme.clone() {
            if !cfg.color_schemes.contains_key(scheme_name.as_str()) {
                let dirs = cfg.compute_color_scheme_dirs();
                // Fast path: try to load just the matching .toml file by name
                // before falling back to a full directory scan.
                if !cfg.try_load_single_color_scheme(&scheme_name, &dirs) {
                    cfg.load_color_schemes(&dirs).ok();
                }
            }
        }

        if let Some(scheme) = cfg.color_scheme.as_ref() {
            match cfg.resolve_color_scheme() {
                None => {
                    log::error!(
                        "Your configuration specifies color_scheme=\"{}\" \
                        but that scheme was not found",
                        scheme
                    );
                }
                Some(p) => {
                    cfg.resolved_palette = p;
                }
            }
        }

        if let Some(colors) = &cfg.colors {
            cfg.resolved_palette = cfg.resolved_palette.overlay_with(colors);
        }

        if let Some(bg) = BackgroundLayer::with_legacy(self) {
            cfg.background.insert(0, bg);
        }

        cfg
    }

    fn compute_color_scheme_dirs(&self) -> Vec<PathBuf> {
        let mut paths = self.color_scheme_dirs.clone();
        for dir in CONFIG_DIRS.iter() {
            paths.push(dir.join("colors"));
        }
        if cfg!(windows) {
            // See commentary re: portable tools above!
            if let Ok(exe_name) = std::env::current_exe() {
                if let Some(exe_dir) = exe_name.parent() {
                    paths.insert(0, exe_dir.join("colors"));
                }
            }
        }
        paths
    }

    /// Try to load a single color scheme by name from the given directories.
    /// Returns true if the scheme was found and loaded.
    fn try_load_single_color_scheme(&mut self, scheme_name: &str, paths: &[PathBuf]) -> bool {
        let file_name = format!("{}.toml", scheme_name);
        for dir in paths {
            let path = dir.join(&file_name);
            if path.is_file() {
                match std::fs::read_to_string(&path)
                    .context("reading color scheme")
                    .and_then(|s| ColorSchemeFile::from_toml_str(&s).context("parsing TOML"))
                {
                    Ok(scheme) => {
                        let name = scheme
                            .metadata
                            .name
                            .unwrap_or_else(|| scheme_name.to_string());
                        self.color_schemes.insert(name, scheme.colors);
                        if self.color_schemes.contains_key(scheme_name) {
                            return true;
                        }
                    }
                    Err(err) => {
                        log::error!(
                            "Color scheme in `{}` failed to load: {:#}",
                            path.display(),
                            err
                        );
                    }
                }
            }
        }
        false
    }

    fn load_color_schemes(&mut self, paths: &[PathBuf]) -> anyhow::Result<()> {
        fn extract_scheme_name(name: &str) -> Option<&str> {
            if name.ends_with(".toml") {
                let len = name.len();
                Some(&name[..len - 5])
            } else {
                None
            }
        }

        fn load_scheme(path: &Path) -> anyhow::Result<ColorSchemeFile> {
            let s = std::fs::read_to_string(path)?;
            ColorSchemeFile::from_toml_str(&s).context("parsing TOML")
        }

        for colors_dir in paths {
            if let Ok(dir) = std::fs::read_dir(colors_dir) {
                for entry in dir {
                    if let Ok(entry) = entry {
                        if let Some(name) = entry.file_name().to_str() {
                            if let Some(scheme_name) = extract_scheme_name(name) {
                                if self.color_schemes.contains_key(scheme_name) {
                                    // This scheme has already been defined
                                    continue;
                                }

                                let path = entry.path();
                                match load_scheme(&path) {
                                    Ok(scheme) => {
                                        let name = scheme
                                            .metadata
                                            .name
                                            .unwrap_or_else(|| scheme_name.to_string());
                                        log::trace!(
                                            "Loaded color scheme `{}` from {}",
                                            name,
                                            path.display()
                                        );
                                        self.color_schemes.insert(name, scheme.colors);
                                    }
                                    Err(err) => {
                                        log::error!(
                                            "Color scheme in `{}` failed to load: {:#}",
                                            path.display(),
                                            err
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn resolve_color_scheme(&self) -> Option<Palette> {
        let scheme_name = self.color_scheme.as_ref()?;

        if let Some(palette) = self.color_schemes.get(scheme_name) {
            Some(palette.clone())
        } else {
            crate::COLOR_SCHEMES.get(scheme_name)
        }
    }

    pub fn initial_size(&self, dpi: u32, cell_pixel_dims: Option<(usize, usize)>) -> TerminalSize {
        // If we aren't passed the actual values, guess at a plausible
        // default set of pixel dimensions.
        // This is based on "typical" 10 point font at "normal"
        // pixel density.
        // This will get filled in by the gui layer, but there is
        // an edge case where we emit an iTerm image escape in
        // the software update banner through the mux layer before
        // the GUI has had a chance to update the pixel dimensions
        // when running under X11.
        // This is a bit gross.
        let (cell_pixel_width, cell_pixel_height) = cell_pixel_dims.unwrap_or((8, 16));

        TerminalSize {
            rows: self.initial_rows as usize,
            cols: self.initial_cols as usize,
            pixel_width: cell_pixel_width * self.initial_cols as usize,
            pixel_height: cell_pixel_height * self.initial_rows as usize,
            dpi,
        }
    }

    pub fn build_prog(
        &self,
        prog: Option<Vec<&OsStr>>,
        default_prog: Option<&Vec<String>>,
        default_cwd: Option<&PathBuf>,
    ) -> anyhow::Result<CommandBuilder> {
        let mut cmd = match prog {
            Some(args) => {
                let mut args = args.iter();
                let mut cmd = CommandBuilder::new(args.next().expect("executable name"));
                cmd.args(args);
                cmd
            }
            None => {
                if let Some(prog) = default_prog {
                    let mut args = prog.iter();
                    let mut cmd = CommandBuilder::new(args.next().expect("executable name"));
                    cmd.args(args);
                    cmd
                } else {
                    CommandBuilder::new_default_prog()
                }
            }
        };

        self.apply_cmd_defaults(&mut cmd, None, default_cwd);

        Ok(cmd)
    }

    pub fn apply_cmd_defaults(
        &self,
        cmd: &mut CommandBuilder,
        default_prog: Option<&Vec<String>>,
        default_cwd: Option<&PathBuf>,
    ) {
        // Apply `default_cwd` only if `cwd` is not already set, allows `--cwd`
        // option to take precedence
        if let (None, Some(cwd)) = (cmd.get_cwd(), default_cwd) {
            cmd.cwd(cwd);
        }

        if let Some(default_prog) = default_prog {
            if cmd.is_default_prog() {
                cmd.replace_default_prog(default_prog);
            }
        }

        // Augment WSLENV so that TERM related environment propagates
        // across the win32/wsl boundary
        let mut wsl_env = std::env::var("WSLENV").ok();

        // If we are running as an appimage, we will have "$APPIMAGE"
        // and "$APPDIR" set in the wezterm process. These will be
        // propagated to the child processes. Since some apps (including
        // wezterm) use these variables to detect if they are running in
        // an appimage, those child processes will be misconfigured.
        // Ensure that they are unset.
        // https://docs.appimage.org/packaging-guide/environment-variables.html#id2
        cmd.env_remove("APPIMAGE");
        cmd.env_remove("APPDIR");
        cmd.env_remove("OWD");

        for (k, v) in &self.set_environment_variables {
            if k == "WSLENV" {
                wsl_env.replace(v.clone());
            } else {
                cmd.env(k, v);
            }
        }

        if !smart_tab_env_is_explicit(cmd) {
            match self.smart_tab_mode {
                SmartTabMode::Off => cmd.env(KAKU_SMART_TAB_DISABLE, "1"),
                SmartTabMode::SuggestionFirst => cmd.env(KAKU_TAB_ACCEPT_SUGGEST_FIRST, "1"),
                SmartTabMode::CompletionFirst => {}
            }
        }

        if wsl_env.is_some() || cfg!(windows) || crate::version::running_under_wsl() {
            let mut wsl_env = wsl_env.unwrap_or_default();
            if !wsl_env.is_empty() {
                wsl_env.push(':');
            }
            wsl_env.push_str("TERM:COLORTERM:TERM_PROGRAM:TERM_PROGRAM_VERSION");
            cmd.env("WSLENV", wsl_env);
        }

        #[cfg(unix)]
        cmd.umask(umask::UmaskSaver::saved_umask());
        cmd.env("TERM", &self.term);
        if self.term == "kaku" {
            if let Some(terminfo_dir) = bundled_terminfo_dir() {
                if let Some(terminfo_dirs) =
                    merged_terminfo_dirs(std::env::var_os("TERMINFO_DIRS"), &terminfo_dir)
                {
                    cmd.env("TERMINFO_DIRS", terminfo_dirs);
                }
            }
        }
        cmd.env("COLORTERM", "truecolor");
        // TERM_PROGRAM and TERM_PROGRAM_VERSION are an emerging
        // de-facto standard for identifying the terminal.
        cmd.env("TERM_PROGRAM", "Kaku");
        cmd.env("TERM_PROGRAM_VERSION", crate::wezterm_version());
        // Sync East Asian Ambiguous width with go-runewidth (used by bubbletea/lipgloss
        // Go TUI programs). Without this, go-runewidth auto-detects CJK locale and treats
        // ambiguous-width chars as wide=2, while Kaku defaults to narrow=1, causing
        // character misalignment and missing text in Go TUI apps.
        cmd.env(
            "RUNEWIDTH_EASTASIAN",
            if self.treat_east_asian_ambiguous_width_as_wide {
                "1"
            } else {
                "0"
            },
        );

        // Recompute COLORFGBG from the final resolved palette so any user
        // overrides to color_scheme are reflected in spawned child processes.
        if let Some(bg) = self.resolved_palette.background.as_ref() {
            cmd.env(
                "COLORFGBG",
                if crate::color::is_light_color(bg) {
                    "0;15"
                } else {
                    "15;0"
                },
            );
        }
    }
}

fn default_check_for_updates() -> bool {
    cfg!(not(feature = "distro-defaults"))
}

fn default_pane_select_fg_color() -> RgbaColor {
    SrgbaTuple(0.75, 0.75, 0.75, 1.0).into()
}

fn default_pane_select_bg_color() -> RgbaColor {
    SrgbaTuple(0., 0., 0., 0.5).into()
}

fn default_pane_select_font_size() -> f64 {
    36.0
}

fn default_split_thickness() -> f32 {
    2.0
}

fn default_integrated_title_buttons() -> Vec<IntegratedTitleButton> {
    use IntegratedTitleButton::*;
    vec![Hide, Maximize, Close]
}

fn default_char_select_font_size() -> f64 {
    18.0
}

fn default_char_select_fg_color() -> RgbaColor {
    SrgbaTuple(0.75, 0.75, 0.75, 1.0).into()
}

fn default_char_select_bg_color() -> RgbaColor {
    (0x33, 0x33, 0x33).into()
}

fn default_command_palette_font_size() -> f64 {
    14.0
}

fn default_command_palette_fg_color() -> RgbaColor {
    SrgbaTuple(0.75, 0.75, 0.75, 1.0).into()
}

fn default_command_palette_bg_color() -> RgbaColor {
    (0x33, 0x33, 0x33).into()
}

fn default_swallow_mouse_click_on_window_focus() -> bool {
    cfg!(target_os = "macos")
}

fn default_mux_output_parser_coalesce_delay_ms() -> u64 {
    3
}

fn default_mux_synchronized_output_timeout_ms() -> u64 {
    1000
}

fn default_mux_output_parser_buffer_size() -> usize {
    128 * 1024
}

fn default_ratelimit_line_prefetches_per_second() -> u32 {
    50
}

fn default_cursor_blink_rate() -> u64 {
    800
}

fn default_text_blink_rate() -> u64 {
    500
}

fn default_text_blink_rate_rapid() -> u64 {
    250
}

fn default_swap_backspace_and_delete() -> bool {
    // cfg!(target_os = "macos")
    // See: https://github.com/wezterm/wezterm/issues/88
    false
}

fn default_scrollback_lines() -> usize {
    3500
}

const MAX_SCROLLBACK_LINES: usize = 999_999_999;
fn validate_scrollback_lines(value: &usize) -> Result<(), String> {
    if *value > MAX_SCROLLBACK_LINES {
        return Err(format!(
            "Illegal value {value} for scrollback_lines; it must be <= {MAX_SCROLLBACK_LINES}!"
        ));
    }
    Ok(())
}

fn default_initial_rows() -> u16 {
    24
}

fn default_initial_cols() -> u16 {
    80
}

pub fn default_hyperlink_rules() -> Vec<hyperlink::Rule> {
    vec![
        // First handle URLs wrapped with punctuation (i.e. brackets)
        // e.g. [http://foo] (http://foo) <http://foo>
        hyperlink::Rule::with_highlight(r"\((\w+://[\x21-\x7e]+)\)", "$1", 1).unwrap(),
        hyperlink::Rule::with_highlight(r"\[(\w+://[\x21-\x7e]+)\]", "$1", 1).unwrap(),
        hyperlink::Rule::with_highlight(r"<(\w+://[\x21-\x7e]+)>", "$1", 1).unwrap(),
        // Then handle URLs not wrapped in brackets that
        // 1) have a balanced ending parenthesis or
        hyperlink::Rule::new(hyperlink::CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
        // 2) include terminating _, / or - characters, if any
        hyperlink::Rule::new(hyperlink::GENERIC_HYPERLINK_PATTERN, "$0").unwrap(),
        // implicit mailto link
        hyperlink::Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
        // Bare domains without an explicit scheme: www.-prefixed hosts, and
        // hosts ending in a curated TLD allowlist. These must come before the
        // file-path rule: on equal-length overlaps (github.com/tw93/kaku) the
        // earlier rule wins, and the web interpretation is the useful one.
        // Emails and scheme'd URLs are longer matches and keep priority.
        // The allowlist deliberately excludes TLDs that collide with common
        // file suffixes (sh, md, rs, py, js, go, cc, so, in, pl, pm, ml, tf,
        // zip, mov, app) so deploy.sh or dist/Kaku.app never become web
        // links; .app in particular is wall-to-wall macOS bundle names in a
        // terminal. Lookarounds are ASCII-only so domains adjacent to CJK
        // text stay clickable, and a sentence-final dot stays out of the
        // match.
        hyperlink::Rule::new(
            r"(?i)(?<![0-9a-z._-])www\.[0-9a-z-]+(?:\.[0-9a-z-]+)+(?::\d+)?(?:/[\x21-\x7e]*[_/a-zA-Z0-9-]|/)?(?!\.?[0-9a-z_-])",
            "https://$0",
        )
        .unwrap(),
        // The TLD group is case-sensitive lowercase (real domains render
        // lowercase in terminals) so namespace-style identifiers such as
        // `System.Net` never match, and a match directly followed by `(` is
        // rejected so method calls like `model.to(device)` / `df.info()`
        // stay plain text.
        hyperlink::Rule::new(
            r"(?i)(?<![0-9a-z._-])(?:[0-9a-z][0-9a-z-]*\.)+(?-i:com|net|org|edu|gov|io|dev|ai|fun|xyz|me|im|tv|to|co|info|biz|tech|site|online|cloud|blog|store|link|live|news|cn|jp|kr|uk|de|fr|us|ca|au|br|ru|nl|se|ch|hk|tw|sg)(?::\d+)?(?:/[\x21-\x7e]*[_/a-zA-Z0-9-]|/)?(?!\.?[0-9a-z_-]|\()",
            "https://$0",
        )
        .unwrap(),
        // File paths: support absolute paths, common relative prefixes, and
        // bare relative paths like `kaku/src/main.rs`.
        // Supports file:line and file:line:col formats.
        //
        // The trailing character class is intentionally restricted to ASCII
        // word/`/`/`-` so that trailing punctuation (`.`, `,`, `;`, `:`, `!`,
        // `?`), CJK particles (e.g. Korean `에`/`을`), or matched wrappers
        // (backticks, quotes) attached after a path are excluded from the
        // match via regex backtracking. The leading boundary class likewise
        // admits backticks and straight quotes so a path wrapped in those
        // characters is still detected.
        hyperlink::Rule::with_highlight(
            r#"(^|[\s\(\[<`'"])((?:~|\.{1,2}|[[:alnum:]_.-]+)?/[^\s\)\]\}>`'"]*[a-zA-Z0-9_/-])"#,
            "file://$2",
            2,
        )
        .unwrap(),
    ]
}

fn default_harfbuzz_features() -> Vec<String> {
    ["kern", "liga", "clig"]
        .iter()
        .map(|&s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::default_hyperlink_rules;
    use std::sync::Arc;
    use termwiz::hyperlink::{Hyperlink, Rule, RuleMatch};

    fn hotkey_fixture(
        source: &str,
        overrides: &wezterm_dynamic::Value,
        hotkey: bool,
    ) -> super::Config {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "kaku-hotkey-config-{}-{}.lua",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&path, source).unwrap();
        let loaded = super::Config::try_load(
            &super::PathPossibility::required(path.clone()),
            overrides,
            hotkey,
        );
        let _ = std::fs::remove_file(super::Config::bytecode_cache_path(&path));
        std::fs::remove_file(path).unwrap();
        loaded.unwrap().unwrap().config.unwrap()
    }

    fn hotkey_overrides(source: &str) -> wezterm_dynamic::Value {
        let lua = mlua::Lua::new();
        luahelper::lua_value_to_dynamic(lua.load(source).eval().unwrap()).unwrap()
    }

    fn assert_hotkey_padding(config: &super::Config, expected: [f32; 4]) {
        let context = crate::DimensionContext {
            dpi: 192.0,
            pixel_max: 800.0,
            pixel_cell: 16.0,
        };
        let p = &config.window_padding;
        assert_eq!(
            [p.left, p.right, p.top, p.bottom].map(|d| d.evaluate_as_pixels(context)),
            expected
        );
    }

    #[test]
    fn hotkey_inheritance_missing_nil_and_empty_inherit_base() {
        use wezterm_dynamic::ToDynamic;
        for padding in [
            "",
            "macos_global_hotkey_padding = nil,",
            "macos_global_hotkey_padding = {},",
        ] {
            let config = hotkey_fixture(&format!(
                "return {{window_padding={{left='21px',right='22px',top='23px',bottom='24px'}}, {padding}}}"
            ), &wezterm_dynamic::Value::Null, true);
            assert_hotkey_padding(&config, [21.0, 22.0, 23.0, 24.0]);
            if padding.is_empty() {
                let wezterm_dynamic::Value::Object(fields) = config.to_dynamic() else {
                    panic!("object")
                };
                assert_eq!(
                    fields.get(&wezterm_dynamic::Value::String(
                        "macos_global_hotkey_padding".into()
                    )),
                    Some(&wezterm_dynamic::Value::Null)
                );
            }
        }
    }

    #[test]
    fn hotkey_inheritance_partial_nil_zero_preserves_units() {
        let config = hotkey_fixture(
            "return {window_padding={left='21px',right='1%',top='3pt',bottom='2cell'}, macos_global_hotkey_padding={left=0,top=nil}}",
            &wezterm_dynamic::Value::Null, true);
        assert_hotkey_padding(&config, [0.0, 8.0, 8.0, 32.0]);
        let config = hotkey_fixture(
            "return {window_padding={left='21px',right='22px',top='23px',bottom='24px'}, macos_global_hotkey_padding={left='2cell',right='1%',top='3pt',bottom='0px'}}",
            &wezterm_dynamic::Value::Null, true);
        assert_hotkey_padding(&config, [32.0, 8.0, 8.0, 0.0]);
    }

    #[test]
    fn hotkey_inheritance_ordinary_is_unchanged() {
        let config = hotkey_fixture(
            "return {window_padding={left=21,right=22,top=23,bottom=24},macos_global_hotkey_padding={left=0}}",
            &wezterm_dynamic::Value::Null, false);
        assert_hotkey_padding(&config, [21.0, 22.0, 23.0, 24.0]);
    }

    #[test]
    fn hotkey_inheritance_clear_and_reload_recompute_base() {
        for (left, override_value, expected) in
            [(21, "{left=0}", 0.0), (21, "nil", 21.0), (31, "{}", 31.0)]
        {
            let config = hotkey_fixture(&format!(
                "return {{window_padding={{left={left},right=22,top=23,bottom=24}},macos_global_hotkey_padding={override_value}}}"
            ), &wezterm_dynamic::Value::Null, true);
            assert_hotkey_padding(&config, [expected, 22.0, 23.0, 24.0]);
        }
    }

    #[test]
    fn hotkey_inheritance_appearance_and_palette_derive_once() {
        use wezterm_dynamic::ToDynamic;
        let source = "return {font_size=13,color_scheme='Kaku Dark',window_background_image='bg.png',macos_global_hotkey_appearance={font_size=19,color_scheme='Kaku Light',colors={cursor_bg='#123456'},enable_tab_bar=false}}";
        let ordinary = hotkey_fixture(source, &wezterm_dynamic::Value::Null, false);
        for _ in 0..3 {
            let hotkey = hotkey_fixture(source, &wezterm_dynamic::Value::Null, true);
            assert_eq!(ordinary.font_size, 13.0);
            assert_eq!(hotkey.font_size, 19.0);
            assert!(!hotkey.enable_tab_bar);
            assert_eq!(hotkey.color_scheme.as_deref(), Some("Kaku Light"));
            assert_ne!(
                hotkey.resolved_palette.to_dynamic(),
                ordinary.resolved_palette.to_dynamic()
            );
            let light = hotkey_fixture(
                "return {color_scheme='Kaku Light',colors={cursor_bg='#123456'}}",
                &wezterm_dynamic::Value::Null,
                false,
            );
            assert_eq!(
                hotkey.resolved_palette.to_dynamic(),
                light.resolved_palette.to_dynamic()
            );
            assert_eq!(hotkey.font_rules.len(), ordinary.font_rules.len());
            assert_eq!(hotkey.background.len(), ordinary.background.len());
        }
    }

    #[test]
    fn hotkey_inheritance_explicit_window_overrides_win() {
        let config = hotkey_fixture(
            "return {font_size=13,window_padding={left=21,right=22,top=23,bottom=24},macos_global_hotkey_padding={left=0},macos_global_hotkey_appearance={font_size=19,enable_tab_bar=false}}",
            &hotkey_overrides("return {font_size=27,enable_tab_bar=true,window_padding={left=1,right=2,top=3,bottom=4}}"), true);
        assert_eq!(config.font_size, 27.0);
        assert!(config.enable_tab_bar);
        assert_hotkey_padding(&config, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn hotkey_inheritance_default_fallback_applies_window_overrides() {
        let loaded = super::Config::try_default_hotkey(&hotkey_overrides(
            "return {font_size=27,window_padding={left=1,right=2,top=3,bottom=4}}",
        ))
        .unwrap();
        let config = loaded.config.unwrap();
        assert_eq!(config.font_size, 27.0);
        assert_hotkey_padding(&config, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn hotkey_inheritance_runtime_layers_and_plain_fields_have_ordered_priority() {
        let source = "return {font_size=13,window_padding={left=21,right=22,top=23,bottom=24},macos_global_hotkey_padding={left=16,top=8},macos_global_hotkey_appearance={font_size=17,enable_tab_bar=false}}";
        let layer = hotkey_overrides("return {macos_global_hotkey_padding={left=0},macos_global_hotkey_appearance={font_size=19}}" );
        let config = hotkey_fixture(source, &layer, true);
        assert_hotkey_padding(&config, [0.0, 22.0, 23.0, 24.0]);
        assert_eq!(config.font_size, 19.0);
        assert!(config.enable_tab_bar);
        let overrides = hotkey_overrides("return {macos_global_hotkey_padding={left=0},macos_global_hotkey_appearance={font_size=19,enable_tab_bar=false},font_size=27,enable_tab_bar=true,window_padding={left=1,right=2,top=3,bottom=4}}" );
        let config = hotkey_fixture(source, &overrides, true);
        assert_hotkey_padding(&config, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(config.font_size, 27.0);
        assert!(config.enable_tab_bar);
        let ordinary = hotkey_fixture(source, &layer, false);
        assert_hotkey_padding(&ordinary, [21.0, 22.0, 23.0, 24.0]);
        assert_eq!(ordinary.font_size, 13.0);
    }

    #[test]
    fn hotkey_inheritance_runtime_empty_and_null_layers_restore_base() {
        use wezterm_dynamic::Value;
        let source = "return {font_size=13,window_padding={left=21,right=22,top=23,bottom=24},macos_global_hotkey_padding={left=16,top=8},macos_global_hotkey_appearance={font_size=17}}";
        for cleared in [Value::Null, Value::Object(Default::default())] {
            let mut fields = wezterm_dynamic::Object::default();
            fields.insert(
                Value::String("macos_global_hotkey_padding".into()),
                cleared.clone(),
            );
            fields.insert(
                Value::String("macos_global_hotkey_appearance".into()),
                cleared,
            );
            let config = hotkey_fixture(source, &Value::Object(fields), true);
            assert_hotkey_padding(&config, [21.0, 22.0, 23.0, 24.0]);
            assert_eq!(config.font_size, 13.0);
            assert!(config.macos_global_hotkey_appearance.font_size.is_none());
        }
        let inherited = hotkey_fixture(source, &Value::Null, true);
        assert_hotkey_padding(&inherited, [16.0, 22.0, 8.0, 24.0]);
        assert_eq!(inherited.font_size, 17.0);
    }

    #[test]
    fn hotkey_inheritance_partial_colors_overlay_base_before_new_scheme_derivation() {
        use wezterm_dynamic::ToDynamic;
        let source = "return {color_scheme='Kaku Dark',colors={foreground='#112233',selection_bg='#abcdef'},macos_global_hotkey_appearance={color_scheme='Kaku Light',colors={cursor_bg='#445566'}}}";
        let expected = hotkey_fixture("return {color_scheme='Kaku Light',colors={foreground='#112233',selection_bg='#abcdef',cursor_bg='#445566'}}", &wezterm_dynamic::Value::Null,false);
        for _ in 0..3 {
            let config = hotkey_fixture(source, &wezterm_dynamic::Value::Null, true);
            assert_eq!(
                config.colors.as_ref().unwrap().to_dynamic(),
                expected.colors.as_ref().unwrap().to_dynamic()
            );
            assert_eq!(
                config.resolved_palette.to_dynamic(),
                expected.resolved_palette.to_dynamic()
            );
        }
        let config = hotkey_fixture(source,&hotkey_overrides("return {macos_global_hotkey_appearance={color_scheme='Kaku Light',colors={cursor_bg='#445566'}}}"),true);
        assert_eq!(
            config.resolved_palette.to_dynamic(),
            expected.resolved_palette.to_dynamic()
        );
    }

    #[test]
    fn hotkey_inheritance_all_typed_appearance_fields_apply_and_nil_is_empty() {
        use wezterm_dynamic::{ToDynamic, Value};
        let fields = "font=wezterm.font('Menlo'),font_rules={},font_size=19,line_height=1.4,color_scheme='Kaku Light',colors={cursor_bg='#112233'},window_background_opacity=0.7,text_background_opacity=0.8,macos_window_background_blur=12,window_frame={border_left_width='3px'},enable_tab_bar=false,hide_tab_bar_if_only_one_tab=false,tab_bar_at_bottom=true,use_fancy_tab_bar=false,enable_scroll_bar=true";
        let expected = hotkey_fixture(
            &format!("local wezterm=require('wezterm');return {{{fields}}}"),
            &Value::Null,
            false,
        );
        let hotkey = hotkey_fixture(&format!("local wezterm=require('wezterm');return {{macos_global_hotkey_appearance={{{fields}}}}}"),&Value::Null,true);
        let Value::Object(actual) = hotkey.to_dynamic() else {
            panic!("object")
        };
        let Value::Object(expected) = expected.to_dynamic() else {
            panic!("object")
        };
        assert_eq!(
            super::MacosGlobalHotkeyAppearance::possible_field_names().len(),
            15
        );
        for key in super::MacosGlobalHotkeyAppearance::possible_field_names() {
            assert_eq!(actual.get_by_str(key), expected.get_by_str(key), "{key}");
        }
        for layer in ["nil", "{}"] {
            let config = hotkey_fixture(
                &format!("return {{font_size=13,macos_global_hotkey_appearance={layer}}}"),
                &Value::Null,
                true,
            );
            assert_eq!(config.font_size, 13.0);
            assert!(config.macos_global_hotkey_appearance.font_size.is_none());
        }
    }

    #[test]
    fn hotkey_inheritance_whitelist_rejects_nonappearance_fields() {
        let lua = mlua::Lua::new();
        for field in [
            "keys={}",
            "default_prog={'sh'}",
            "window_padding={left=0}",
            "macos_global_hotkey_follow_mouse=true",
        ] {
            let raw = lua
                .load(format!(
                    "return {{macos_global_hotkey_appearance={{{field}}}}}"
                ))
                .eval()
                .unwrap();
            assert!(
                super::Config::finish_hotkey_config(&lua, raw, &wezterm_dynamic::Value::Null, None)
                    .is_err(),
                "accepted {}",
                field
            );
        }
    }

    #[test]
    fn global_hotkey_behavior_defaults_validation_and_roundtrip() {
        use wezterm_dynamic::{FromDynamic, ToDynamic, Value};
        let defaults = super::Config::default();
        assert_eq!(
            defaults.macos_global_hotkey_mode,
            super::MacosGlobalHotkeyMode::Drawer
        );
        assert_eq!(defaults.macos_global_hotkey_height_percent, 100.0 / 3.0);
        assert_eq!(defaults.macos_global_hotkey_animation_duration_ms, 200);
        assert!(defaults.macos_global_hotkey_hide_on_focus_loss);
        assert_eq!(defaults.macos_global_hotkey_extend_behind_notch, None);
        for mode in ["Drawer", "Ordinary"] {
            let value = hotkey_overrides(&format!("return {{macos_global_hotkey_mode='{mode}',macos_global_hotkey_height_percent=100,macos_global_hotkey_animation_duration_ms=0,macos_global_hotkey_hide_on_focus_loss=false,macos_global_hotkey_extend_behind_notch=false}}"));
            let config = super::Config::from_dynamic(&value, Default::default()).unwrap();
            config.check_consistency().unwrap();
            assert_eq!(
                config.macos_global_hotkey_mode.to_dynamic(),
                Value::String(mode.into())
            );
            assert_eq!(config.macos_global_hotkey_extend_behind_notch, Some(false));
            assert!(!config.macos_global_hotkey_hide_on_focus_loss);
            let roundtrip =
                super::Config::from_dynamic(&config.to_dynamic(), Default::default()).unwrap();
            assert_eq!(roundtrip.to_dynamic(), config.to_dynamic());
        }
        for height in [0.0, -1.0, 100.1, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let mut fields = wezterm_dynamic::Object::default();
            fields.insert(
                Value::String("macos_global_hotkey_height_percent".into()),
                Value::F64(height.into()),
            );
            assert!(
                super::Config::from_dynamic(&Value::Object(fields), Default::default()).is_err(),
                "accepted height {}",
                height
            );
        }
        for height in [0.1, 100.0 / 3.0, 100.0] {
            let mut config = super::Config::default();
            config.macos_global_hotkey_height_percent = height;
            assert!(config.check_consistency().is_ok());
        }
        for duration in [0, 200, 10_000, 10_001] {
            let mut fields = wezterm_dynamic::Object::default();
            fields.insert(
                Value::String("macos_global_hotkey_animation_duration_ms".into()),
                Value::U64(duration),
            );
            assert_eq!(
                super::Config::from_dynamic(&Value::Object(fields), Default::default()).is_ok(),
                duration <= 10_000
            );
        }
    }

    #[test]
    fn hotkey_inheritance_loader_and_public_api_wiring() {
        let source = include_str!("config.rs");
        let loader = source
            .split_once("    fn try_load(\n")
            .unwrap()
            .1
            .split_once("    pub(crate) fn apply_overrides_obj_to")
            .unwrap()
            .0;
        let cli = loader
            .find("Config::apply_overrides_to(&lua, config)")
            .unwrap();
        let hotkey = loader
            .find("Self::apply_hotkey_to(&lua, config, overrides)")
            .unwrap();
        let window = loader
            .find("Config::apply_overrides_obj_to(&lua, config, overrides)")
            .unwrap();
        let parse = loader.find("Config::from_lua(config, &lua)").unwrap();
        let derive = loader.find("cfg.compute_extra_defaults(Some(p))").unwrap();
        assert!(cli < hotkey && hotkey < window && window < parse && parse < derive);
        let api = include_str!("lib.rs")
            .split_once("pub fn hotkey_config(")
            .unwrap()
            .1
            .split_once("pub fn reload()")
            .unwrap()
            .0;
        assert!(api.contains("Config::load_hotkey_with_overrides(overrides)"));
        assert!(source.contains("Self::load_with_window_overrides(overrides, true)"));
        assert!(source.contains("Self::try_default_hotkey(overrides)"));
    }

    #[test]
    fn global_hotkey_follow_mouse_defaults_to_false_and_accepts_boolean() {
        use wezterm_dynamic::{FromDynamic, ToDynamic, Value};

        assert!(!super::Config::default().macos_global_hotkey_follow_mouse);
        for enabled in [false, true] {
            let mut fields = wezterm_dynamic::Object::default();
            fields.insert(
                Value::String("macos_global_hotkey_follow_mouse".into()),
                Value::Bool(enabled),
            );
            let config =
                super::Config::from_dynamic(&Value::Object(fields), Default::default()).unwrap();
            assert_eq!(config.macos_global_hotkey_follow_mouse, enabled);
            let roundtrip =
                super::Config::from_dynamic(&config.to_dynamic(), Default::default()).unwrap();
            assert_eq!(roundtrip.macos_global_hotkey_follow_mouse, enabled);
        }
    }

    #[test]
    fn bundled_lua_with_hotkey_follow_mouse_compiles() {
        let source = include_str!("../../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        mlua::Lua::new()
            .load(source)
            .into_function()
            .expect("bundled Lua must stay within the parser's locals/register limits");
    }

    #[test]
    fn bundled_resize_hook_preserves_hotkey_overrides_and_ordinary_cleanup() {
        let source = include_str!("../../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let start = source.find("local resize_state_by_window =").unwrap();
        let end = start
            + source[start..]
                .find("local function extract_path_from_cwd")
                .unwrap();
        // Execute the shipped hook, without loading unrelated startup or filesystem hooks.
        let fixture = format!(
            r#"
            {}
            os.clock = function() return 10 end
            local padding = {{ left = '16px', right = '16px', top = '8px', bottom = '8px' }}
            local appearance = {{ font_size = 20, window_background_opacity = 0.85 }}
            local window = {{
              dims = {{ pixel_width = 3456, pixel_height = 746, is_hotkey_window = true }},
              reads = 0, writes = 0,
              overrides = {{
                window_padding = padding,
                hide_tab_bar_if_only_one_tab = false,
                window_content_alignment = {{ horizontal = 'Center' }},
                macos_global_hotkey_appearance = appearance,
                font_size = 18,
              }},
            }}
            function window:get_dimensions() return self.dims end
            function window:is_focused() return true end
            function window:get_config_overrides()
              assert(not self.dims.is_hotkey_window, 'drawer overrides must not be read')
              self.reads = self.reads + 1
              return self.overrides
            end
            function window:set_config_overrides(overrides)
              self.writes = self.writes + 1
              self.overrides = overrides
            end
            for _, fullscreen in ipairs({{ false, true }}) do
              update_window_config(window, fullscreen, nil)
              window.dims.pixel_height = window.dims.pixel_height + 1
              update_window_config(window, fullscreen, nil)
              assert(window.reads == 0 and window.writes == 0)
              assert(window.overrides.window_padding == padding)
              assert(window.overrides.hide_tab_bar_if_only_one_tab == false)
              assert(window.overrides.window_content_alignment.horizontal == 'Center')
              assert(window.overrides.macos_global_hotkey_appearance == appearance)
              assert(window.overrides.font_size == 18)
            end
            for _, fullscreen in ipairs({{ false, true }}) do
              window.dims.is_hotkey_window = false
              window.overrides.window_padding = padding
              window.overrides.hide_tab_bar_if_only_one_tab = false
              window.overrides.window_content_alignment = {{ horizontal = 'Center' }}
              local writes = window.writes
              update_window_config(window, fullscreen, nil)
              assert(window.writes == writes + 1)
              assert(window.overrides.window_padding == nil)
              assert(window.overrides.hide_tab_bar_if_only_one_tab == nil)
              assert(window.overrides.window_content_alignment == nil)
              assert(window.overrides.macos_global_hotkey_appearance == appearance)
              assert(window.overrides.font_size == 18)
              update_window_config(window, fullscreen, nil)
              window.dims.pixel_height = window.dims.pixel_height + 1
              update_window_config(window, fullscreen, nil)
              assert(window.writes == writes + 1)
            end
            "#,
            &source[start..end]
        );
        mlua::Lua::new().load(&fixture).exec().unwrap();
    }

    #[test]
    fn bundled_reload_hook_preserves_hotkey_and_updates_ordinary_theme() {
        let source = include_str!("../../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let start = source.find("wezterm.on('window-config-reloaded',").unwrap();
        let end = start
            + source[start..]
                .find("config.bold_brightens_ansi_colors =")
                .unwrap();
        let fixture = format!(
            r#"
            local reload, syncs, layouts = nil, 0, 0
            local wezterm = {{ on = function(event, callback)
              assert(event == 'window-config-reloaded')
              reload = callback
            end }}
            local config = {{ color_scheme = 'Kaku Dark', window_frame = {{}} }}
            local window_font_theme = {{}}
            local window_has_managed_font_override = {{}}
            local window_has_managed_window_frame_override = {{}}
            local user_has_custom_font, user_has_custom_font_rules = false, false
            local user_has_custom_window_frame = false
            local function resolve_kaku_color_scheme(scheme) return scheme end
            local function sync_claude_code_theme() syncs = syncs + 1 end
            local function build_font_config(light) return {{ light = light }}, {{ light }} end
            local function window_frame_matches_theme(frame, scheme)
              return frame.scheme == scheme
            end
            local function build_managed_window_frame(scheme) return {{ scheme = scheme }} end
            local function update_window_config() layouts = layouts + 1 end
            {}
            local function window(hotkey)
              local w = {{ writes = 0, overrides = {{
                font = 'original-font', font_rules = 'original-rules',
                window_frame = {{ scheme = 'original-frame' }},
                font_size = 20, window_padding = {{ bottom = '8px' }},
              }} }}
              function w:get_dimensions() return {{ is_hotkey_window = hotkey }} end
              function w:get_config_overrides()
                assert(not hotkey, 'drawer reload must not read overrides')
                return self.overrides
              end
              function w:set_config_overrides(overrides)
                self.writes = self.writes + 1
                self.overrides = overrides
              end
              return w
            end
            local drawer, ordinary = window(true), window(false)
            local frame, padding = drawer.overrides.window_frame, drawer.overrides.window_padding
            reload(drawer, nil)
            assert(drawer.writes == 0 and syncs == 0 and layouts == 0)
            reload(ordinary, nil)
            assert(ordinary.writes == 1 and syncs == 1 and layouts == 1)
            assert(ordinary.overrides.font.light == false)
            assert(ordinary.overrides.font_rules[1] == false)
            assert(ordinary.overrides.window_frame.scheme == 'Kaku Dark')
            reload(ordinary, nil)
            assert(ordinary.writes == 1)
            config.color_scheme = 'Kaku Light'
            reload(ordinary, nil)
            assert(ordinary.writes == 2 and ordinary.overrides.font.light == true)
            assert(ordinary.overrides.window_frame.scheme == 'Kaku Light')
            user_has_custom_font, user_has_custom_window_frame = true, true
            reload(ordinary, nil)
            assert(ordinary.writes == 3)
            assert(ordinary.overrides.font == nil and ordinary.overrides.font_rules == nil)
            assert(ordinary.overrides.window_frame == nil)
            assert(ordinary.overrides.font_size == 20)
            assert(ordinary.overrides.window_padding.bottom == '8px')
            local previous_syncs, previous_layouts = syncs, layouts
            reload(drawer, nil)
            assert(drawer.writes == 0 and syncs == previous_syncs and layouts == previous_layouts)
            assert(drawer.overrides.font == 'original-font')
            assert(drawer.overrides.font_rules == 'original-rules')
            assert(drawer.overrides.window_frame == frame and drawer.overrides.window_padding == padding)
            assert(drawer.overrides.font_size == 20)
            "#,
            &source[start..end]
        );
        mlua::Lua::new().load(&fixture).exec().unwrap();
    }

    #[test]
    fn global_hotkey_padding_preserves_explicit_compact_padding() {
        let source = "return {window_padding={left=21,right=22,top=23,bottom=24},macos_global_hotkey_padding={left='16px',right='16px',top='8px',bottom='8px'}}";
        let hotkey = hotkey_fixture(source, &wezterm_dynamic::Value::Null, true);
        let ordinary = hotkey_fixture(source, &wezterm_dynamic::Value::Null, false);
        assert_hotkey_padding(&hotkey, [16.0, 16.0, 8.0, 8.0]);
        assert_hotkey_padding(&ordinary, [21.0, 22.0, 23.0, 24.0]);
    }

    #[test]
    fn global_hotkey_padding_accepts_custom_units_and_roundtrips() {
        use wezterm_dynamic::{FromDynamic, ToDynamic, Value};

        let mut padding = wezterm_dynamic::Object::default();
        for (side, unit) in [
            ("left", "2cell"),
            ("right", "1%"),
            ("top", "3pt"),
            ("bottom", "0px"),
        ] {
            padding.insert(Value::String(side.into()), Value::String(unit.into()));
        }
        let mut fields = wezterm_dynamic::Object::default();
        fields.insert(
            Value::String("macos_global_hotkey_padding".into()),
            Value::Object(padding),
        );
        let config =
            super::Config::from_dynamic(&Value::Object(fields), Default::default()).unwrap();
        let roundtrip =
            super::Config::from_dynamic(&config.to_dynamic(), Default::default()).unwrap();
        let context = crate::DimensionContext {
            dpi: 192.0,
            pixel_max: 800.0,
            pixel_cell: 16.0,
        };
        for config in [config, roundtrip] {
            let padding = config
                .macos_global_hotkey_padding
                .unwrap()
                .resolve(&config.window_padding);
            assert_eq!(padding.left.evaluate_as_pixels(context), 32.0);
            assert_eq!(padding.right.evaluate_as_pixels(context), 8.0);
            assert_eq!(padding.top.evaluate_as_pixels(context), 8.0);
            assert_eq!(padding.bottom.evaluate_as_pixels(context), 0.0);
            assert_eq!(
                config.window_padding.to_dynamic(),
                super::Config::default().window_padding.to_dynamic()
            );
        }
    }

    #[test]
    fn global_hotkey_padding_partial_override_inherits_other_sides() {
        use wezterm_dynamic::{FromDynamic, Value};
        let defaults = super::Config::default().window_padding;
        for side in ["left", "right", "top", "bottom"] {
            let mut padding = wezterm_dynamic::Object::default();
            padding.insert(Value::String(side.into()), Value::String("0px".into()));
            let mut fields = wezterm_dynamic::Object::default();
            fields.insert(
                Value::String("macos_global_hotkey_padding".into()),
                Value::Object(padding),
            );
            let config =
                super::Config::from_dynamic(&Value::Object(fields), Default::default()).unwrap();
            let context = crate::DimensionContext {
                dpi: 96.0,
                pixel_max: 800.0,
                pixel_cell: 20.0,
            };
            let padding = config
                .macos_global_hotkey_padding
                .unwrap()
                .resolve(&config.window_padding);
            for (name, actual, default) in [
                ("left", padding.left, defaults.left),
                ("right", padding.right, defaults.right),
                ("top", padding.top, defaults.top),
                ("bottom", padding.bottom, defaults.bottom),
            ] {
                assert_eq!(
                    actual.evaluate_as_pixels(context),
                    if side == name {
                        0.0
                    } else {
                        default.evaluate_as_pixels(context)
                    },
                    "overriding {side} changed {name}"
                );
            }
        }
    }

    #[test]
    fn half_intensity_font_rules_keep_base_weight() {
        let config = super::Config::default();
        let computed = config.compute_extra_defaults(None);
        let base_weights: Vec<_> = config.font.font.iter().map(|attr| attr.weight).collect();
        let half_rules: Vec<_> = computed
            .font_rules
            .iter()
            .filter(|rule| rule.intensity == Some(wezterm_term::Intensity::Half))
            .collect();
        assert!(
            !half_rules.is_empty(),
            "expected synthesized half-intensity rules"
        );
        for rule in half_rules {
            for (attr, base) in rule.font.font.iter().zip(&base_weights) {
                assert_eq!(
                    attr.weight, *base,
                    "half-intensity text must keep the base weight; dimming is color-only"
                );
            }
        }
    }

    #[test]
    fn file_hyperlink_rule_matches_bare_relative_paths() {
        let rules = default_hyperlink_rules();

        assert!(Rule::match_hyperlinks("kaku/src/kaku_theme.rs", &rules)
            .into_iter()
            .any(|m| m.range == (0..22)
                && m.link == Arc::new(Hyperlink::new_implicit("file://kaku/src/kaku_theme.rs"))));
    }

    #[test]
    fn file_hyperlink_rule_does_not_override_urls() {
        let rules = default_hyperlink_rules();

        assert_eq!(
            Rule::match_hyperlinks("https://example.com/kaku/src/kaku_theme.rs", &rules)
                .into_iter()
                .next(),
            Some(RuleMatch {
                range: 0..42,
                link: Arc::new(Hyperlink::new_implicit(
                    "https://example.com/kaku/src/kaku_theme.rs",
                )),
            })
        );
    }

    /// Helper: find the file:// hyperlink match in `text`, returning the
    /// resolved URI. Asserts exactly one file match exists.
    fn file_uri(text: &str) -> String {
        let rules = default_hyperlink_rules();
        let matches: Vec<_> = Rule::match_hyperlinks(text, &rules)
            .into_iter()
            .filter(|m| m.link.uri().starts_with("file://"))
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one file:// match in {text:?}, got {matches:?}",
        );
        matches.into_iter().next().unwrap().link.uri().to_string()
    }

    #[test]
    fn file_hyperlink_trims_trailing_ascii_punctuation() {
        // Sentence-final punctuation should not be swallowed into the link.
        assert_eq!(file_uri("see docs/foo.md."), "file://docs/foo.md");
        assert_eq!(file_uri("see docs/foo.md,"), "file://docs/foo.md");
        assert_eq!(file_uri("see docs/foo.md;"), "file://docs/foo.md");
        assert_eq!(file_uri("is docs/foo.md?"), "file://docs/foo.md");
        assert_eq!(file_uri("yes docs/foo.md!"), "file://docs/foo.md");
        assert_eq!(file_uri("path: docs/foo.md: here"), "file://docs/foo.md");
    }

    #[test]
    fn file_hyperlink_trims_trailing_korean_particles() {
        // A Korean particle attached to a path (no space) is a common
        // pattern; the particle must be excluded from the match.
        assert_eq!(
            file_uri("docs/foo.md\u{C5D0} \u{C800}\u{C7A5}"),
            "file://docs/foo.md"
        );
        assert_eq!(
            file_uri("\u{C774} docs/foo.md\u{B97C} \u{C5F4}\u{C5B4}"),
            "file://docs/foo.md"
        );
        assert_eq!(
            file_uri("docs/foo.md\u{C740} \u{C5C5}"),
            "file://docs/foo.md"
        );
    }

    #[test]
    fn file_hyperlink_handles_wrapping_delimiters() {
        // Backtick/quote wrappers commonly appear around paths in prose or
        // markdown rendered to the terminal; both sides must be stripped.
        assert_eq!(file_uri("see `docs/foo.md` now"), "file://docs/foo.md");
        assert_eq!(file_uri("see 'docs/foo.md' now"), "file://docs/foo.md");
        assert_eq!(file_uri("see \"docs/foo.md\" now"), "file://docs/foo.md");
        assert_eq!(file_uri("see (docs/foo.md) now"), "file://docs/foo.md");
    }

    #[test]
    fn file_hyperlink_preserves_line_and_column() {
        // file:line and file:line:col suffixes must still be captured so
        // that the click handler can jump to the right position.
        assert_eq!(file_uri("see docs/foo.md:42 now"), "file://docs/foo.md:42");
        assert_eq!(
            file_uri("see docs/foo.md:42:10 now"),
            "file://docs/foo.md:42:10"
        );
        assert_eq!(
            file_uri("see docs/foo.md:42:10."),
            "file://docs/foo.md:42:10"
        );
    }

    #[test]
    fn file_hyperlink_handles_absolute_paths_with_particles() {
        // Regression: the motivating bug - an absolute path followed by
        // the Korean locative particle.
        assert_eq!(
            file_uri(
                "\u{C800}\u{C7A5}: /Users/x/Code/proj/docs/plans/2026-04-18-v8-plan.md\u{C5D0} \u{C0DD}\u{C131}"
            ),
            "file:///Users/x/Code/proj/docs/plans/2026-04-18-v8-plan.md"
        );
    }

    #[test]
    fn file_hyperlink_is_extension_neutral() {
        // The match logic is purely about the last character being ASCII
        // word/`/`/`-`, so arbitrary source-file extensions are handled
        // the same way `.md` is. This test pins that invariant so a
        // future change cannot accidentally re-introduce an extension
        // bias.
        assert_eq!(file_uri("src/main.rs\u{B294}"), "file://src/main.rs");
        assert_eq!(file_uri("open src/main.rs."), "file://src/main.rs");
        assert_eq!(file_uri("see `src/lib.rs` ok"), "file://src/lib.rs");
        assert_eq!(file_uri("pkg/util.py\u{C5D0}"), "file://pkg/util.py");
        assert_eq!(file_uri("see app/index.ts."), "file://app/index.ts");
        assert_eq!(file_uri("cmd/server.go:128:"), "file://cmd/server.go:128");
        assert_eq!(file_uri("data/out.json,"), "file://data/out.json");
        assert_eq!(file_uri("infra/.env.local."), "file://infra/.env.local");
        // Extensionless files (Makefile-style) still work because the
        // last character is an ASCII letter.
        assert_eq!(file_uri("see ./Makefile."), "file://./Makefile");
        assert_eq!(
            file_uri("see docker/Dockerfile,"),
            "file://docker/Dockerfile"
        );
    }

    #[test]
    fn bare_domain_hyperlinks_get_https_scheme() {
        let rules = default_hyperlink_rules();
        let uri = |text: &str| -> Option<String> {
            Rule::match_hyperlinks(text, &rules)
                .into_iter()
                .find(|m| m.link.uri().starts_with("https://"))
                .map(|m| m.link.uri().to_string())
        };

        assert_eq!(uri("visit kaku.fun."), Some("https://kaku.fun".to_string()));
        assert_eq!(
            uri("\u{53BB} kaku.fun\u{3002}"),
            Some("https://kaku.fun".to_string())
        );
        assert_eq!(
            uri("www.example.org rocks"),
            Some("https://www.example.org".to_string())
        );
        assert_eq!(
            uri("released at github.com/tw93/kaku today"),
            Some("https://github.com/tw93/kaku".to_string())
        );
        assert_eq!(
            uri("dev server on demo.example.com:8080/index"),
            Some("https://demo.example.com:8080/index".to_string())
        );
    }

    #[test]
    fn bare_domain_beats_file_rule_on_equal_overlap() {
        let rules = default_hyperlink_rules();
        // Both the bare-domain rule and the file-path rule match this whole
        // span. After the length sort the earlier rule comes first, and the
        // first match is the one whose link wins when applied to cells.
        let first = Rule::match_hyperlinks("github.com/tw93/kaku", &rules)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.link.uri(), "https://github.com/tw93/kaku");
    }

    #[test]
    fn bare_domain_skips_file_suffix_lookalike_tlds() {
        let rules = default_hyperlink_rules();
        for text in [
            "run deploy.sh now",
            "see README.md",
            "building main.rs",
            "loaded libfoo.so",
            "regenerated Makefile.in",
            "open dist/Kaku.app",
            "plain Kaku.app name",
            // Method calls and namespace identifiers, not domains.
            "tensor model.to(device) done",
            "print df.info() output",
            "using System.Net;",
            "import System.IO.Path",
            "call foo.De() here",
        ] {
            assert!(
                !Rule::match_hyperlinks(text, &rules)
                    .into_iter()
                    .any(|m| m.link.uri().starts_with("https://")),
                "{:?} must not produce a web link",
                text
            );
        }
    }

    #[test]
    fn email_keeps_priority_over_bare_domain() {
        let rules = default_hyperlink_rules();
        let first = Rule::match_hyperlinks("mail user@github.com now", &rules)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(first.link.uri(), "mailto:user@github.com");
    }

    #[test]
    fn deprecated_language_field_still_loads() {
        // V0.11.0 shipped `config.language`; the i18n revert (b4d779a) removed
        // the field. It must remain a tolerated deprecated field so a kaku.lua
        // carrying `config.language` still loads on upgrade with a warning
        // instead of failing config validation with a hard error.
        use std::collections::BTreeMap;
        use wezterm_dynamic::{FromDynamic, FromDynamicOptions, UnknownFieldAction, Value};

        let mut map: BTreeMap<Value, Value> = BTreeMap::new();
        map.insert(
            Value::String("language".to_string()),
            Value::String("zh-CN".to_string()),
        );
        let value = Value::Object(map.into());

        let result = super::Config::from_dynamic(
            &value,
            FromDynamicOptions {
                unknown_fields: UnknownFieldAction::Deny,
                deprecated_fields: UnknownFieldAction::Warn,
            },
        );
        assert!(
            result.is_ok(),
            "config carrying a deprecated `language` must still load: {:?}",
            result
        );
    }

    #[test]
    fn smart_tab_mode_parses_snake_case_strings() {
        use wezterm_dynamic::{FromDynamic, FromDynamicOptions, Value};

        let options = FromDynamicOptions::default();

        // snake_case variants (written by kaku config TUI)
        let val = Value::String("completion_first".into());
        assert_eq!(
            super::SmartTabMode::from_dynamic(&val, options).unwrap(),
            super::SmartTabMode::CompletionFirst
        );

        let val = Value::String("suggestion_first".into());
        assert_eq!(
            super::SmartTabMode::from_dynamic(&val, options).unwrap(),
            super::SmartTabMode::SuggestionFirst
        );

        let val = Value::String("off".into());
        assert_eq!(
            super::SmartTabMode::from_dynamic(&val, options).unwrap(),
            super::SmartTabMode::Off
        );

        // PascalCase variants (alternative accepted form)
        let val = Value::String("CompletionFirst".into());
        assert_eq!(
            super::SmartTabMode::from_dynamic(&val, options).unwrap(),
            super::SmartTabMode::CompletionFirst
        );

        let val = Value::String("Off".into());
        assert_eq!(
            super::SmartTabMode::from_dynamic(&val, options).unwrap(),
            super::SmartTabMode::Off
        );

        // Invalid value should error
        let val = Value::String("invalid_mode".into());
        assert!(super::SmartTabMode::from_dynamic(&val, options).is_err());
    }

    #[test]
    fn smart_tab_mode_field_loads_without_unknown_field_error() {
        use std::collections::BTreeMap;
        use wezterm_dynamic::{FromDynamic, FromDynamicOptions, UnknownFieldAction, Value};

        let mut map: BTreeMap<Value, Value> = BTreeMap::new();
        map.insert(
            Value::String("smart_tab_mode".to_string()),
            Value::String("off".to_string()),
        );
        let value = Value::Object(map.into());

        let result = super::Config::from_dynamic(
            &value,
            FromDynamicOptions {
                unknown_fields: UnknownFieldAction::Deny,
                deprecated_fields: UnknownFieldAction::Warn,
            },
        );
        assert!(
            result.is_ok(),
            "config with smart_tab_mode must load without error: {:?}",
            result
        );
        let config = result.unwrap();
        assert_eq!(config.smart_tab_mode, super::SmartTabMode::Off);
    }

    #[test]
    fn close_confirmation_parses_bool_and_string_values() {
        use wezterm_dynamic::{FromDynamic, FromDynamicOptions, Value};

        let options = FromDynamicOptions::default();

        assert_eq!(
            super::CloseConfirmation::from_dynamic(&Value::Bool(false), options).unwrap(),
            super::CloseConfirmation::NeverPrompt
        );
        assert_eq!(
            super::CloseConfirmation::from_dynamic(&Value::Bool(true), options).unwrap(),
            super::CloseConfirmation::AlwaysPrompt
        );
        assert_eq!(
            super::CloseConfirmation::from_dynamic(&Value::String("NeverPrompt".into()), options)
                .unwrap(),
            super::CloseConfirmation::NeverPrompt
        );
        assert_eq!(
            super::CloseConfirmation::from_dynamic(&Value::String("SmartPrompt".into()), options)
                .unwrap(),
            super::CloseConfirmation::SmartPrompt
        );
        assert_eq!(
            super::CloseConfirmation::from_dynamic(&Value::String("AlwaysPrompt".into()), options)
                .unwrap(),
            super::CloseConfirmation::AlwaysPrompt
        );
        assert!(
            super::CloseConfirmation::from_dynamic(&Value::String("invalid".into()), options)
                .is_err()
        );
    }

    #[test]
    fn close_confirmation_defaults_to_smart_prompt() {
        let config = super::Config::default();

        assert_eq!(
            config.tab_close_confirmation,
            super::CloseConfirmation::SmartPrompt
        );
        assert_eq!(
            config.pane_close_confirmation,
            super::CloseConfirmation::SmartPrompt
        );
    }

    #[test]
    fn text_min_contrast_ratio_defaults_to_readable_text() {
        let config = super::Config::default();

        assert_eq!(config.text_min_contrast_ratio, Some(3.0));
    }

    #[test]
    fn smart_tab_mode_defaults_to_suggestion_first() {
        let config = super::Config::default();

        assert_eq!(config.smart_tab_mode, super::SmartTabMode::SuggestionFirst);
    }

    #[test]
    fn foreground_process_tab_titles_default_to_off() {
        let config = super::Config::default();

        assert!(!config.tab_title_show_foreground_process);
        assert!(!config.tab_title_use_pane_title);
    }

    #[test]
    fn close_confirmation_policy_matches_prompt_modes() {
        use super::CloseConfirmation::{AlwaysPrompt, NeverPrompt, SmartPrompt};

        assert!(!NeverPrompt.should_prompt(true, || false));
        assert!(SmartPrompt.should_prompt(true, || false));
        assert!(!SmartPrompt.should_prompt(true, || true));
        assert!(!SmartPrompt.should_prompt(false, || false));
        assert!(AlwaysPrompt.should_prompt(false, || true));
    }

    fn smart_tab_test_command() -> portable_pty::CommandBuilder {
        let mut cmd = portable_pty::CommandBuilder::new_default_prog();
        cmd.env_remove(super::KAKU_SMART_TAB_DISABLE);
        cmd.env_remove(super::KAKU_TAB_ACCEPT_SUGGEST_FIRST);
        cmd
    }

    #[test]
    fn smart_tab_off_sets_disable_env_var() {
        let mut config = super::Config::default();
        config.smart_tab_mode = super::SmartTabMode::Off;

        let mut cmd = smart_tab_test_command();
        config.apply_cmd_defaults(&mut cmd, None, None);

        assert_eq!(
            cmd.get_env("KAKU_SMART_TAB_DISABLE"),
            Some(std::ffi::OsStr::new("1")),
            "SmartTabMode::Off must set KAKU_SMART_TAB_DISABLE=1"
        );
        assert_eq!(
            cmd.get_env("KAKU_TAB_ACCEPT_SUGGEST_FIRST"),
            None,
            "SmartTabMode::Off must not set KAKU_TAB_ACCEPT_SUGGEST_FIRST"
        );
    }

    #[test]
    fn smart_tab_suggestion_first_sets_accept_env_var() {
        let mut config = super::Config::default();
        config.smart_tab_mode = super::SmartTabMode::SuggestionFirst;

        let mut cmd = smart_tab_test_command();
        config.apply_cmd_defaults(&mut cmd, None, None);

        assert_eq!(
            cmd.get_env("KAKU_TAB_ACCEPT_SUGGEST_FIRST"),
            Some(std::ffi::OsStr::new("1")),
            "SmartTabMode::SuggestionFirst must set KAKU_TAB_ACCEPT_SUGGEST_FIRST=1"
        );
        assert_eq!(
            cmd.get_env("KAKU_SMART_TAB_DISABLE"),
            None,
            "SmartTabMode::SuggestionFirst must not set KAKU_SMART_TAB_DISABLE"
        );
    }

    #[test]
    fn smart_tab_completion_first_sets_no_env_var() {
        let mut config = super::Config::default();
        config.smart_tab_mode = super::SmartTabMode::CompletionFirst;

        let mut cmd = smart_tab_test_command();
        config.apply_cmd_defaults(&mut cmd, None, None);

        assert_eq!(
            cmd.get_env("KAKU_SMART_TAB_DISABLE"),
            None,
            "SmartTabMode::CompletionFirst must not set KAKU_SMART_TAB_DISABLE"
        );
        assert_eq!(
            cmd.get_env("KAKU_TAB_ACCEPT_SUGGEST_FIRST"),
            None,
            "SmartTabMode::CompletionFirst must not set KAKU_TAB_ACCEPT_SUGGEST_FIRST"
        );
    }

    #[test]
    fn smart_tab_respects_existing_disable_env_var() {
        let mut config = super::Config::default();
        config.smart_tab_mode = super::SmartTabMode::SuggestionFirst;

        let mut cmd = smart_tab_test_command();
        cmd.env(super::KAKU_SMART_TAB_DISABLE, "1");
        config.apply_cmd_defaults(&mut cmd, None, None);

        assert_eq!(
            cmd.get_env(super::KAKU_SMART_TAB_DISABLE),
            Some(std::ffi::OsStr::new("1"))
        );
        assert_eq!(cmd.get_env(super::KAKU_TAB_ACCEPT_SUGGEST_FIRST), None);
    }

    #[test]
    fn smart_tab_respects_existing_suggestion_first_env_var() {
        let mut config = super::Config::default();
        config.smart_tab_mode = super::SmartTabMode::Off;

        let mut cmd = smart_tab_test_command();
        cmd.env(super::KAKU_TAB_ACCEPT_SUGGEST_FIRST, "1");
        config.apply_cmd_defaults(&mut cmd, None, None);

        assert_eq!(cmd.get_env(super::KAKU_SMART_TAB_DISABLE), None);
        assert_eq!(
            cmd.get_env(super::KAKU_TAB_ACCEPT_SUGGEST_FIRST),
            Some(std::ffi::OsStr::new("1"))
        );
    }

    #[test]
    fn colorfgbg_matches_final_resolved_palette() {
        // (background rgb, expected COLORFGBG, stale value pre-seeded via
        // set_environment_variables that the palette must override — mirrors
        // the bundled kaku.lua line that hard-codes COLORFGBG from the scheme
        // name and may disagree with the final resolved background.)
        let cases = [
            ((232, 240, 232), "0;15", "15;0"),
            ((21, 20, 27), "15;0", "0;15"),
        ];

        for (background, expected, stale_override) in cases {
            let mut config = super::Config::default();
            config.resolved_palette.background = Some(background.into());
            config
                .set_environment_variables
                .insert("COLORFGBG".to_string(), stale_override.into());

            let mut cmd = portable_pty::CommandBuilder::new_default_prog();
            cmd.env_remove("COLORFGBG");
            config.apply_cmd_defaults(&mut cmd, None, None);

            assert_eq!(
                cmd.get_env("COLORFGBG"),
                Some(std::ffi::OsStr::new(expected)),
                "COLORFGBG should be derived from the final resolved palette, \
                 overriding any value set via set_environment_variables"
            );
        }
    }
}

fn default_term() -> String {
    // WezTerm sets `wezterm` here, but `kaku` causes SSH issues since its
    // terminfo doesn't exist on remote servers. So we default to `xterm-256color`.
    "xterm-256color".into()
}

fn bundled_terminfo_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }

    if let Ok(exe_name) = std::env::current_exe() {
        if let Some(contents_dir) = exe_name.parent().and_then(|p| p.parent()) {
            let terminfo_dir = contents_dir.join("Resources").join("terminfo");
            if terminfo_dir.is_dir() {
                return Some(terminfo_dir);
            }
        }
    }

    None
}

fn merged_terminfo_dirs(existing: Option<OsString>, first: &Path) -> Option<OsString> {
    let mut paths = vec![first.to_path_buf()];
    if let Some(existing) = existing {
        for path in std::env::split_paths(&existing) {
            if path != first {
                paths.push(path);
            }
        }
    }
    std::env::join_paths(paths).ok()
}

fn default_font_size() -> f64 {
    12.0
}

pub(crate) fn compute_cache_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::cache_dir() {
        return Ok(runtime.join("kaku"));
    }

    Ok(crate::HOME_DIR.join(".local/share/kaku"))
}

pub(crate) fn compute_data_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::data_dir() {
        return Ok(runtime.join("kaku"));
    }

    Ok(crate::HOME_DIR.join(".local/share/kaku"))
}

pub(crate) fn compute_runtime_dir() -> anyhow::Result<PathBuf> {
    if let Some(runtime) = dirs_next::runtime_dir() {
        return Ok(runtime.join("kaku"));
    }

    Ok(crate::HOME_DIR.join(".local/share/kaku"))
}

pub fn pki_dir() -> anyhow::Result<PathBuf> {
    compute_runtime_dir().map(|d| d.join("pki"))
}

pub fn default_read_timeout() -> Duration {
    Duration::from_secs(60)
}

pub fn default_write_timeout() -> Duration {
    Duration::from_secs(60)
}

pub fn default_local_echo_threshold_ms() -> Option<u64> {
    Some(100)
}

fn default_bypass_mouse_reporting_modifiers() -> Modifiers {
    Modifiers::SHIFT
}

fn default_gui_startup_args() -> Vec<String> {
    vec!["start".to_string()]
}

fn default_pane_encoding() -> PaneEncoding {
    PaneEncoding::Utf8
}

// Coupled with term/src/config.rs:TerminalConfiguration::unicode_version
fn default_unicode_version() -> u8 {
    14
}

fn default_mux_env_remove() -> Vec<String> {
    vec![
        "SSH_AUTH_SOCK".to_string(),
        "SSH_CLIENT".to_string(),
        "SSH_CONNECTION".to_string(),
    ]
}

fn default_anim_fps() -> u8 {
    10
}

const fn default_text_min_contrast_ratio() -> Option<f32> {
    Some(3.0)
}

fn default_max_fps() -> u64 {
    60
}

fn default_tiling_desktop_environments() -> Vec<String> {
    [
        "X11 LG3D",
        "X11 Qtile",
        "X11 awesome",
        "X11 bspwm",
        "X11 dwm",
        "X11 i3",
        "X11 xmonad",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_stateless_process_list() -> Vec<String> {
    [
        "bash",
        "sh",
        "zsh",
        "fish",
        "tmux",
        "nu",
        "nu.exe",
        "cmd.exe",
        "pwsh.exe",
        "powershell.exe",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_status_update_interval() -> u64 {
    1_000
}

fn default_quit_when_all_windows_are_closed() -> bool {
    #[cfg(target_os = "macos")]
    {
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

fn default_alternate_buffer_wheel_scroll_speed() -> u8 {
    3
}

fn default_num_alphabet() -> String {
    // Note: vi motion keys are intentionally excluded from this alphabet
    "1234567890abcdefghilmnopqrstuvwxyz".to_string()
}

fn default_alphabet() -> String {
    "asdfqwerzxcvjklmiuopghtybn".to_string()
}

fn default_word_boundary() -> String {
    " \t\n{[}]()\"'`".to_string()
}

fn default_enq_answerback() -> String {
    "".to_string()
}

fn default_tab_max_width() -> usize {
    16
}

fn default_update_interval() -> u64 {
    10800
}

fn default_prefer_egl() -> bool {
    // MetalANGLE via EGL is the preferred path on macOS in general, but
    // older Intel Macs can abort during startup inside the bundled ANGLE
    // stack. Keep EGL opt-in there and preserve the safer CGL fallback.
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        return false;
    }
    !cfg!(windows)
}

fn default_clean_exits() -> Vec<u32> {
    vec![]
}

fn default_inactive_pane_hsb() -> HsbTransform {
    HsbTransform::default()
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, Default)]
pub enum DefaultCursorStyle {
    #[default]
    BlinkingBar,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    SteadyBar,
}

impl DefaultCursorStyle {
    pub fn effective_shape(self, shape: CursorShape) -> CursorShape {
        match shape {
            CursorShape::Default => match self {
                Self::BlinkingBlock => CursorShape::BlinkingBlock,
                Self::SteadyBlock => CursorShape::SteadyBlock,
                Self::BlinkingUnderline => CursorShape::BlinkingUnderline,
                Self::SteadyUnderline => CursorShape::SteadyUnderline,
                Self::BlinkingBar => CursorShape::BlinkingBar,
                Self::SteadyBar => CursorShape::SteadyBar,
            },
            _ => shape,
        }
    }
}

const fn linear_ease() -> EasingFunction {
    EasingFunction::Linear
}

const fn default_half_cell() -> Dimension {
    Dimension::Cells(0.5)
}

const fn default_reverse_video_cursor_min_contrast() -> f32 {
    2.5
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug)]
pub struct WindowPadding {
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub left: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub top: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub right: Dimension,
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_half_cell")]
    pub bottom: Dimension,
}

/// A missing side inherits the ordinary window's padding, including its unit.
#[derive(Debug, Clone, Copy, Default, FromDynamic, ToDynamic)]
pub struct HotkeyWindowPadding {
    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub left: Option<Dimension>,
    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub right: Option<Dimension>,
    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub top: Option<Dimension>,
    #[dynamic(try_from = "crate::units::OptPixelUnit", default)]
    pub bottom: Option<Dimension>,
}

impl HotkeyWindowPadding {
    pub fn resolve(&self, base: &WindowPadding) -> WindowPadding {
        WindowPadding {
            left: self.left.unwrap_or(base.left),
            right: self.right.unwrap_or(base.right),
            top: self.top.unwrap_or(base.top),
            bottom: self.bottom.unwrap_or(base.bottom),
        }
    }
}

/// Only window appearance can be overridden; session and input settings stay shared.
#[derive(Debug, Clone, Default, FromDynamic, ToDynamic)]
pub struct MacosGlobalHotkeyAppearance {
    #[dynamic(default)]
    pub font: Option<TextStyle>,
    #[dynamic(default)]
    pub font_rules: Option<Vec<StyleRule>>,
    #[dynamic(default)]
    pub font_size: Option<f64>,
    #[dynamic(default)]
    pub line_height: Option<f64>,
    #[dynamic(default)]
    pub color_scheme: Option<String>,
    #[dynamic(default)]
    pub colors: Option<Palette>,
    #[dynamic(default)]
    pub window_background_opacity: Option<f32>,
    #[dynamic(default)]
    pub text_background_opacity: Option<f32>,
    #[dynamic(default)]
    pub macos_window_background_blur: Option<i64>,
    #[dynamic(default)]
    pub window_frame: Option<WindowFrameConfig>,
    #[dynamic(default)]
    pub enable_tab_bar: Option<bool>,
    #[dynamic(default)]
    pub hide_tab_bar_if_only_one_tab: Option<bool>,
    #[dynamic(default)]
    pub tab_bar_at_bottom: Option<bool>,
    #[dynamic(default)]
    pub use_fancy_tab_bar: Option<bool>,
    #[dynamic(default)]
    pub enable_scroll_bar: Option<bool>,
}

impl From<Option<MacosGlobalHotkeyAppearance>> for MacosGlobalHotkeyAppearance {
    fn from(appearance: Option<Self>) -> Self {
        appearance.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum MacosGlobalHotkeyMode {
    #[default]
    Drawer,
    Ordinary,
}

impl Default for WindowPadding {
    fn default() -> Self {
        Self {
            left: default_half_cell(),
            right: default_half_cell(),
            top: default_half_cell(),
            bottom: default_half_cell(),
        }
    }
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, Default)]
pub struct WindowContentAlignment {
    pub horizontal: HorizontalWindowContentAlignment,
    pub vertical: VerticalWindowContentAlignment,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum HorizontalWindowContentAlignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalWindowContentAlignment {
    #[default]
    Top,
    Center,
    Bottom,
}

/// Behavior of the mouse wheel while a left-button terminal selection drag is
/// in progress. See `Config::selection_wheel_scroll_behavior`.
#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionWheelScrollBehavior {
    /// Scroll the viewport and stretch the selection so its endpoint tracks
    /// the cursor under the new viewport. This is the macOS `NSTextView`
    /// idiom and the new Kaku default.
    #[default]
    Extend,
    /// Scroll the viewport but do not update the selection endpoint.
    ScrollOnly,
    /// Drop the wheel event. Equivalent to Kaku v0.10 and earlier behavior.
    Ignore,
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewlineCanon {
    // FIXME: also allow deserialziing from bool
    None,
    LineFeed,
    CarriageReturn,
    CarriageReturnAndLineFeed,
}

#[derive(FromDynamic, ToDynamic, Clone, Copy, Debug, Default)]
pub enum WindowCloseConfirmation {
    #[default]
    AlwaysPrompt,
    NeverPrompt,
    /// Only prompt when a window still has a stateful process running
    /// (anything outside `skip_close_confirmation_for_processes_named`).
    /// Quits silently when every pane is at a bare shell prompt.
    SmartPrompt,
}

#[derive(Debug, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseConfirmation {
    NeverPrompt,
    #[default]
    SmartPrompt,
    AlwaysPrompt,
}

impl CloseConfirmation {
    pub fn should_prompt(
        self,
        action_confirm: bool,
        can_close_without_prompting: impl FnOnce() -> bool,
    ) -> bool {
        match self {
            Self::NeverPrompt => false,
            Self::SmartPrompt => action_confirm && !can_close_without_prompting(),
            Self::AlwaysPrompt => true,
        }
    }
}

impl FromDynamic for CloseConfirmation {
    fn from_dynamic(
        value: &wezterm_dynamic::Value,
        options: wezterm_dynamic::FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        match String::from_dynamic(value, options) {
            Ok(s) => match s.as_str() {
                "NeverPrompt" | "never_prompt" => Ok(Self::NeverPrompt),
                "SmartPrompt" | "smart_prompt" => Ok(Self::SmartPrompt),
                "AlwaysPrompt" | "always_prompt" => Ok(Self::AlwaysPrompt),
                other => Err(wezterm_dynamic::Error::Message(format!(
                    "`{other}` is not a valid CloseConfirmation, use one of \
                     `NeverPrompt`, `SmartPrompt`, or `AlwaysPrompt`"
                ))),
            },
            Err(err) => match bool::from_dynamic(value, options) {
                Ok(false) => Ok(Self::NeverPrompt),
                Ok(true) => Ok(Self::AlwaysPrompt),
                Err(_) => Err(err),
            },
        }
    }
}

struct PathPossibility {
    path: PathBuf,
    is_required: bool,
}
impl PathPossibility {
    pub fn required(path: PathBuf) -> PathPossibility {
        PathPossibility {
            path,
            is_required: true,
        }
    }
    pub fn optional(path: PathBuf) -> PathPossibility {
        PathPossibility {
            path,
            is_required: false,
        }
    }
}

/// Behavior when the program spawned by wezterm terminates
#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitBehavior {
    /// Close the associated pane
    #[default]
    Close,
    /// Close the associated pane if the process was successful
    CloseOnCleanExit,
    /// Hold the pane until it is explicitly closed
    Hold,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitBehaviorMessaging {
    #[default]
    Verbose,
    Brief,
    Terse,
    None,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq)]
pub enum DroppedFileQuoting {
    /// No quoting is performed, the file name is passed through as-is
    None,
    /// Backslash escape only spaces, leaving all other characters as-is
    SpacesOnly,
    /// Use POSIX style shell word escaping
    Posix,
    /// Use Windows style shell word escaping
    Windows,
    /// Always double quote the file name
    WindowsAlwaysQuoted,
}

impl Default for DroppedFileQuoting {
    fn default() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::SpacesOnly
        }
    }
}

impl DroppedFileQuoting {
    pub fn escape(self, s: &str) -> String {
        match self {
            Self::None => s.to_string(),
            Self::SpacesOnly => s.replace(" ", "\\ "),
            // https://docs.rs/shlex/latest/shlex/fn.quote.html
            Self::Posix => shlex::try_quote(s)
                .unwrap_or_else(|_| "".into())
                .into_owned(),
            Self::Windows => {
                let chars_need_quoting = [' ', '\t', '\n', '\x0b', '\"'];
                if s.chars().any(|c| chars_need_quoting.contains(&c)) {
                    format!("\"{}\"", s)
                } else {
                    s.to_string()
                }
            }
            Self::WindowsAlwaysQuoted => format!("\"{}\"", s),
        }
    }
}

fn default_glyph_cache_image_cache_size() -> usize {
    256
}

fn default_shape_cache_size() -> usize {
    1024
}

fn default_line_state_cache_size() -> usize {
    1024
}

fn default_line_quad_cache_size() -> usize {
    1024
}

fn default_line_to_ele_shape_cache_size() -> usize {
    1024
}

#[derive(Debug, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoldBrightening {
    /// Bold doesn't influence palette selection
    No,
    /// Bold Shifts palette from 0-7 to 8-15 and preserves bold font
    #[default]
    BrightAndBold,
    /// Bold Shifts palette from 0-7 to 8-15 and removes bold intensity
    BrightOnly,
}

impl FromDynamic for BoldBrightening {
    fn from_dynamic(
        value: &wezterm_dynamic::Value,
        options: wezterm_dynamic::FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        match String::from_dynamic(value, options) {
            Ok(s) => match s.as_str() {
                "No" => Ok(Self::No),
                "BrightAndBold" => Ok(Self::BrightAndBold),
                "BrightOnly" => Ok(Self::BrightOnly),
                s => Err(wezterm_dynamic::Error::Message(format!(
                    "`{s}` is not valid, use one of `No`, `BrightAndBold` or `BrightOnly`"
                ))),
            },
            Err(err) => match bool::from_dynamic(value, options) {
                Ok(true) => Ok(Self::BrightAndBold),
                Ok(false) => Ok(Self::No),
                Err(_) => Err(err),
            },
        }
    }
}

const KAKU_SMART_TAB_DISABLE: &str = "KAKU_SMART_TAB_DISABLE";
const KAKU_TAB_ACCEPT_SUGGEST_FIRST: &str = "KAKU_TAB_ACCEPT_SUGGEST_FIRST";

fn smart_tab_env_is_explicit(cmd: &CommandBuilder) -> bool {
    cmd.get_env(KAKU_SMART_TAB_DISABLE).is_some()
        || cmd.get_env(KAKU_TAB_ACCEPT_SUGGEST_FIRST).is_some()
}

/// Controls how the Tab key behaves in zsh inside Kaku sessions.
#[derive(Debug, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmartTabMode {
    /// Tab shows the completion list; use arrow keys to accept autosuggestions.
    CompletionFirst,
    /// Tab accepts autosuggestions when available, falls back to completion.
    #[default]
    SuggestionFirst,
    /// Disables Smart Tab entirely, restoring native zsh Tab behavior.
    Off,
}

impl FromDynamic for SmartTabMode {
    fn from_dynamic(
        value: &wezterm_dynamic::Value,
        options: wezterm_dynamic::FromDynamicOptions,
    ) -> Result<Self, wezterm_dynamic::Error> {
        let s = String::from_dynamic(value, options)?;
        match s.as_str() {
            "completion_first" | "CompletionFirst" => Ok(Self::CompletionFirst),
            "suggestion_first" | "SuggestionFirst" => Ok(Self::SuggestionFirst),
            "off" | "Off" => Ok(Self::Off),
            other => Err(wezterm_dynamic::Error::Message(format!(
                "`{other}` is not a valid SmartTabMode, use one of \
                 `completion_first`, `suggestion_first`, or `off`"
            ))),
        }
    }
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImePreeditRendering {
    /// IME preedit is rendered by WezTerm itself
    #[default]
    Builtin,
    /// IME preedit shifts the rest of the row for display only.
    BuiltinInsert,
    /// IME preedit is rendered by system
    System,
}

#[derive(Debug, FromDynamic, ToDynamic, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationHandling {
    #[default]
    AlwaysShow,
    NeverShow,
    SuppressFromFocusedPane,
    SuppressFromFocusedTab,
    SuppressFromFocusedWindow,
}

fn validate_row_or_col(value: &u16) -> Result<(), String> {
    if *value < 1 {
        Err("initial_cols and initial_rows must be non-zero".to_string())
    } else {
        Ok(())
    }
}

fn validate_line_height(value: &f64) -> Result<(), String> {
    if *value <= 0.0 {
        Err(format!(
            "Illegal value {value} for line_height; it must be positive and greater than zero!"
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_domain_name(name: &str) -> Result<(), String> {
    if name == "local" {
        Err(format!(
            "\"{name}\" is a built-in domain and cannot be redefined"
        ))
    } else if name == "" {
        Err("the empty string is an invalid domain name".to_string())
    } else {
        Ok(())
    }
}

/// <https://github.com/wezterm/wezterm/pull/2435>
/// <https://github.com/wezterm/wezterm/issues/2771>
/// <https://github.com/wezterm/wezterm/issues/2630>
fn default_macos_forward_mods() -> Modifiers {
    Modifiers::SHIFT
}

fn default_macos_global_hotkey() -> Option<KeyNoAction> {
    Some(KeyNoAction {
        key: DeferredKeyCode::try_from("K").expect("default global hotkey key to parse"),
        mods: Modifiers::CTRL | Modifiers::ALT | Modifiers::SUPER,
    })
}

fn default_hotkey_height_percent() -> f64 {
    100.0 / 3.0
}

fn validate_hotkey_height_percent(value: &f64) -> Result<(), String> {
    if !value.is_finite() || *value <= 0.0 || *value > 100.0 {
        Err(
            "macos_global_hotkey_height_percent must be finite and greater than 0 through 100"
                .into(),
        )
    } else {
        Ok(())
    }
}

fn default_hotkey_animation_duration_ms() -> u64 {
    200
}

fn validate_hotkey_animation_duration_ms(value: &u64) -> Result<(), String> {
    if *value > 10_000 {
        Err("macos_global_hotkey_animation_duration_ms must not exceed 10000".into())
    } else {
        Ok(())
    }
}

fn default_colr_rasterizer() -> FontRasterizerSelection {
    FontRasterizerSelection::Harfbuzz
}
