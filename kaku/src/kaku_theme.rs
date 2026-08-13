use config::{configuration, ConfigHandle};
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use wezterm_term::color::{ColorPalette, SrgbaTuple};

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub primary: SrgbaTuple,
    pub secondary: SrgbaTuple,
    pub accent: SrgbaTuple,
    pub error: SrgbaTuple,
    pub text: SrgbaTuple,
    pub muted: SrgbaTuple,
    pub bg: SrgbaTuple,
    pub is_light: bool,
}

#[derive(Clone, Copy)]
struct CachedTheme {
    palette: ThemePalette,
    appearance_sensitive: bool,
    appearance_is_dark: Option<bool>,
}

static THEME_CACHE: Mutex<Option<(usize, CachedTheme)>> = Mutex::new(None);
#[cfg(target_os = "macos")]
static APPEARANCE_CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
#[cfg(target_os = "macos")]
const APPEARANCE_CACHE_TTL: Duration = Duration::from_secs(1);

fn rgb(hex: &str) -> SrgbaTuple {
    let hex = hex.trim_start_matches('#');
    let parse = |range| {
        hex.get(range)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .unwrap_or(0) as f32
            / 255.0
    };

    SrgbaTuple(parse(0..2), parse(2..4), parse(4..6), 1.0)
}

#[derive(Debug, Deserialize)]
struct CanonicalFile {
    themes: CanonicalThemes,
}

#[derive(Debug, Deserialize)]
struct CanonicalThemes {
    kaku_dark: CanonicalTheme,
    kaku_light: CanonicalTheme,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CanonicalTheme {
    semantic: CanonicalSemantic,
    terminal: CanonicalTerminal,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CanonicalSemantic {
    background: String,
    foreground: String,
    foreground_soft: String,
    muted: String,
    primary: String,
    red: String,
    green: String,
    yellow: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CanonicalTerminal {
    foreground: String,
    background: String,
    cursor: String,
}

fn canonical_file() -> CanonicalFile {
    toml::from_str(include_str!("../../assets/themes/kaku-palette.toml"))
        .expect("canonical Kaku palette must be valid TOML")
}

fn opaque(color: SrgbaTuple) -> SrgbaTuple {
    SrgbaTuple(color.0, color.1, color.2, 1.0)
}

fn blend(base: SrgbaTuple, overlay: SrgbaTuple, amount: f32) -> SrgbaTuple {
    let amount = amount.clamp(0.0, 1.0);
    SrgbaTuple(
        base.0 + (overlay.0 - base.0) * amount,
        base.1 + (overlay.1 - base.1) * amount,
        base.2 + (overlay.2 - base.2) * amount,
        1.0,
    )
}

fn luminance(color: SrgbaTuple) -> f32 {
    0.299 * color.0 + 0.587 * color.1 + 0.114 * color.2
}

fn is_light_color(color: SrgbaTuple) -> bool {
    luminance(color) > 0.5
}

fn color_distance(a: SrgbaTuple, b: SrgbaTuple) -> f32 {
    let dr = a.0 - b.0;
    let dg = a.1 - b.1;
    let db = a.2 - b.2;
    (dr * dr + dg * dg + db * db).sqrt()
}

fn has_enough_separation(bg: SrgbaTuple, color: SrgbaTuple) -> bool {
    // Stricter thresholds ensure readable contrast in both light and dark modes.
    // Requires both sufficient color distance AND luminance difference.
    color_distance(bg, color) >= 0.22 && (luminance(bg) - luminance(color)).abs() >= 0.15
}

fn visible_fallback_anchor(bg: SrgbaTuple, text: SrgbaTuple) -> SrgbaTuple {
    let text = opaque(text);
    if has_enough_separation(bg, text) {
        return text;
    }

    if is_light_color(bg) {
        rgb("#000000")
    } else {
        rgb("#FFFFFF")
    }
}

fn nudge_toward_separation(
    bg: SrgbaTuple,
    candidate: SrgbaTuple,
    anchor: SrgbaTuple,
) -> SrgbaTuple {
    for amount in [0.25, 0.4, 0.55, 0.7, 0.85, 1.0] {
        let adjusted = blend(candidate, anchor, amount);
        if has_enough_separation(bg, adjusted) {
            return adjusted;
        }
    }

    anchor
}

fn pick_visible(bg: SrgbaTuple, text: SrgbaTuple, candidates: &[SrgbaTuple]) -> SrgbaTuple {
    for candidate in candidates {
        let candidate = opaque(*candidate);
        if has_enough_separation(bg, candidate) {
            return candidate;
        }
    }

    let fallback = opaque(candidates[0]);
    // Push the fallback toward a known readable anchor instead of blending it
    // back into the background, which only reduces separation further.
    let anchor = visible_fallback_anchor(bg, text);
    nudge_toward_separation(bg, fallback, anchor)
}

fn pick_muted(bg: SrgbaTuple, text: SrgbaTuple, candidate: SrgbaTuple) -> SrgbaTuple {
    let candidate = opaque(candidate);
    if has_enough_separation(bg, candidate) {
        candidate
    } else {
        blend(bg, text, if is_light_color(bg) { 0.42 } else { 0.5 })
    }
}

fn approx_eq(a: SrgbaTuple, b: SrgbaTuple) -> bool {
    (a.0 - b.0).abs() <= 0.01 && (a.1 - b.1).abs() <= 0.01 && (a.2 - b.2).abs() <= 0.01
}

fn palette_matches_builtin(
    palette: &ColorPalette,
    bg: SrgbaTuple,
    text: SrgbaTuple,
    cursor_bg: SrgbaTuple,
) -> bool {
    approx_eq(opaque(palette.background), bg)
        && approx_eq(opaque(palette.foreground), text)
        && approx_eq(opaque(palette.cursor_bg), cursor_bg)
}

fn dark_palette() -> ThemePalette {
    let theme = canonical_file().themes.kaku_dark;
    ThemePalette {
        primary: rgb(&theme.semantic.primary),
        secondary: rgb(&theme.semantic.green),
        accent: rgb(&theme.semantic.yellow),
        error: rgb(&theme.semantic.red),
        text: rgb(&theme.semantic.foreground),
        muted: rgb(&theme.semantic.muted),
        bg: rgb(&theme.semantic.background),
        is_light: false,
    }
}

fn light_palette() -> ThemePalette {
    let theme = canonical_file().themes.kaku_light;
    ThemePalette {
        primary: rgb(&theme.semantic.primary),
        secondary: rgb(&theme.semantic.green),
        accent: rgb(&theme.semantic.yellow),
        error: rgb(&theme.semantic.red),
        text: rgb(&theme.semantic.foreground),
        muted: rgb(&theme.semantic.muted),
        bg: rgb(&theme.semantic.background),
        is_light: true,
    }
}

fn cached_theme(palette: ThemePalette) -> CachedTheme {
    CachedTheme {
        palette,
        appearance_sensitive: false,
        appearance_is_dark: None,
    }
}

fn appearance_sensitive_theme(palette: ThemePalette, is_dark: bool) -> CachedTheme {
    CachedTheme {
        palette,
        appearance_sensitive: true,
        appearance_is_dark: Some(is_dark),
    }
}

fn builtin_kaku_theme(config: &ConfigHandle) -> Option<CachedTheme> {
    let dark = dark_palette();
    let light = light_palette();
    let dark_terminal_text = rgb("#D5D4D6");
    let light_terminal_text = rgb("#100F0F");
    let light_cursor = rgb("#343331");

    match config.color_scheme.as_deref() {
        Some("Kaku Dark") | Some("Kaku Theme") => {
            // The color_scheme field might hold "Kaku Dark" either because:
            //   (a) the user explicitly chose Kaku Dark, OR
            //   (b) the user chose Auto, but the TUI process evaluated the Lua
            //       expression with wezterm.gui=nil, which always falls back to
            //       'Kaku Dark'.
            // Disambiguate by checking the raw config file.
            if config_file_has_auto_color_scheme(config) {
                let is_dark = is_macos_dark_mode();
                if is_dark {
                    return Some(appearance_sensitive_theme(dark, true));
                } else {
                    return Some(appearance_sensitive_theme(light, false));
                }
            }
            return Some(cached_theme(dark));
        }
        Some("Kaku Light") => return Some(cached_theme(light)),
        _ => {}
    }

    let palette: ColorPalette = config.resolved_palette.clone().into();
    if palette_matches_builtin(&palette, dark.bg, dark_terminal_text, dark.primary) {
        Some(cached_theme(dark))
    } else if palette_matches_builtin(&palette, light.bg, light_terminal_text, light_cursor) {
        Some(cached_theme(light))
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColorSchemeSelection {
    Light,
    Dark,
    Auto,
    Other,
}

fn color_scheme_assignment_rhs(trimmed_line: &str) -> Option<&str> {
    let rest = trimmed_line.strip_prefix("config.color_scheme")?;
    let rest = rest.trim_start();
    let rhs = rest.strip_prefix('=')?;
    Some(rhs.trim_start())
}

fn rhs_starts_with_quoted_literal(rhs: &str, value: &str) -> bool {
    let single = format!("'{value}'");
    let double = format!("\"{value}\"");
    rhs.starts_with(&single) || rhs.starts_with(&double)
}

fn parse_color_scheme_selection_line(line: &str) -> Option<ColorSchemeSelection> {
    let trimmed = line.trim();
    if trimmed.starts_with("--") {
        return None;
    }

    let rhs = color_scheme_assignment_rhs(trimmed)?;
    if rhs_starts_with_quoted_literal(rhs, "Kaku Light") {
        return Some(ColorSchemeSelection::Light);
    }
    if rhs_starts_with_quoted_literal(rhs, "Kaku Dark")
        || rhs_starts_with_quoted_literal(rhs, "Kaku Theme")
    {
        return Some(ColorSchemeSelection::Dark);
    }
    if rhs.contains("get_appearance") || rhs.contains("resolve_kaku_color_scheme") {
        return Some(ColorSchemeSelection::Auto);
    }
    Some(ColorSchemeSelection::Other)
}

fn color_scheme_selection_from_content(content: &str) -> Option<ColorSchemeSelection> {
    content
        .lines()
        .filter_map(parse_color_scheme_selection_line)
        .last()
}

/// Returns true when the user's color scheme intent is Auto.
/// This covers two cases:
///   1. User config explicitly assigns an appearance-based expression (get_appearance).
///   2. User config has no color_scheme assignment at all, meaning the bundled
///      default (Auto) is in effect.
fn config_file_has_auto_color_scheme(_config: &ConfigHandle) -> bool {
    let path = config::effective_config_file_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    matches!(
        color_scheme_selection_from_content(&content),
        Some(ColorSchemeSelection::Auto) | None
    )
}

/// Detects whether macOS is currently running in Dark Mode by reading the
/// `AppleInterfaceStyle` value from the global user defaults.
/// Returns true for Dark, false for Light (or when detection is unavailable).
#[cfg(target_os = "macos")]
fn is_macos_dark_mode() -> bool {
    let now = Instant::now();
    let mut cache = APPEARANCE_CACHE.lock().unwrap();
    if let Some((checked_at, is_dark)) = *cache {
        if now.duration_since(checked_at) < APPEARANCE_CACHE_TTL {
            return is_dark;
        }
    }

    use std::process::Command;
    // `defaults read -g AppleInterfaceStyle` prints "Dark" in dark mode and
    // exits with a non-zero code (key not found) in light mode.
    let is_dark = match Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.trim().eq_ignore_ascii_case("dark")
        }
        Err(_) => false, // if the command fails, fall back to light/false
    };
    *cache = Some((now, is_dark));
    is_dark
}

#[cfg(not(target_os = "macos"))]
fn is_macos_dark_mode() -> bool {
    false
}

fn theme_from_config(config: &ConfigHandle) -> CachedTheme {
    if let Some(theme) = builtin_kaku_theme(config) {
        return theme;
    }

    let palette: ColorPalette = config.resolved_palette.clone().into();
    let bg = opaque(palette.background);
    let text = opaque(palette.foreground);
    let is_light = is_light_color(bg);

    let primary = pick_visible(
        bg,
        text,
        &[palette.colors.0[13], palette.colors.0[5], palette.cursor_bg],
    );
    let secondary = pick_visible(
        bg,
        text,
        &[
            palette.colors.0[10],
            palette.colors.0[6],
            palette.colors.0[2],
        ],
    );
    let accent = pick_visible(
        bg,
        text,
        &[
            palette.colors.0[11],
            palette.colors.0[3],
            palette.cursor_border,
        ],
    );
    let error = pick_visible(
        bg,
        text,
        &[
            palette.colors.0[9],
            palette.colors.0[1],
            palette.cursor_border,
        ],
    );
    let muted = pick_muted(bg, text, palette.colors.0[8]);
    cached_theme(ThemePalette {
        primary,
        secondary,
        accent,
        error,
        text,
        muted,
        bg,
        is_light,
    })
}

fn is_current_theme_cache_hit(
    cached_generation: usize,
    generation: usize,
    theme: CachedTheme,
    current_appearance_is_dark: Option<bool>,
) -> bool {
    if cached_generation != generation {
        return false;
    }
    if !theme.appearance_sensitive {
        return true;
    }
    theme.appearance_is_dark == current_appearance_is_dark
}

fn current_theme() -> CachedTheme {
    let config = configuration();
    let generation = config.generation();

    let mut cached = THEME_CACHE.lock().unwrap();
    if let Some((cached_generation, theme)) = *cached {
        let current_appearance_is_dark = if theme.appearance_sensitive {
            Some(is_macos_dark_mode())
        } else {
            None
        };
        if is_current_theme_cache_hit(
            cached_generation,
            generation,
            theme,
            current_appearance_is_dark,
        ) {
            return theme;
        }
    }

    let theme = theme_from_config(&config);
    *cached = Some((generation, theme));
    theme
}

pub fn current_theme_palette() -> ThemePalette {
    current_theme().palette
}

pub(crate) fn config_for_theme_commands() -> ConfigHandle {
    configuration()
}

#[cfg(test)]
mod tests {
    use super::{
        appearance_sensitive_theme, cached_theme, color_distance,
        color_scheme_selection_from_content, dark_palette, has_enough_separation,
        is_current_theme_cache_hit, luminance, parse_color_scheme_selection_line, pick_visible,
        rgb, ColorSchemeSelection,
    };

    #[test]
    fn ignores_non_assignment_lines() {
        assert_eq!(
            parse_color_scheme_selection_line("local x = 'get_appearance'"),
            None
        );
    }

    #[test]
    fn ignores_comment_lines() {
        assert_eq!(
            parse_color_scheme_selection_line(
                "-- config.color_scheme = (wezterm.gui and wezterm.gui.get_appearance())"
            ),
            None
        );
    }

    #[test]
    fn detects_light_dark_and_auto_assignments() {
        assert_eq!(
            parse_color_scheme_selection_line("config.color_scheme = 'Kaku Light'"),
            Some(ColorSchemeSelection::Light)
        );
        assert_eq!(
            parse_color_scheme_selection_line("config.color_scheme = \"Kaku Dark\""),
            Some(ColorSchemeSelection::Dark)
        );
        assert_eq!(
            parse_color_scheme_selection_line(
                "config.color_scheme = (wezterm.gui and wezterm.gui.get_appearance() or 'Dark'):find('Dark') and 'Kaku Dark' or 'Kaku Light'"
            ),
            Some(ColorSchemeSelection::Auto)
        );
        // The bundled kaku.lua uses resolve_kaku_color_scheme which
        // internally delegates to get_appearance. The Rust-side detection
        // must recognize this wrapper as Auto too.
        assert_eq!(
            parse_color_scheme_selection_line(
                "config.color_scheme = resolve_kaku_color_scheme(config.color_scheme)"
            ),
            Some(ColorSchemeSelection::Auto)
        );
    }

    #[test]
    fn marks_unknown_assignment_as_other() {
        assert_eq!(
            parse_color_scheme_selection_line("config.color_scheme = some_runtime_value"),
            Some(ColorSchemeSelection::Other)
        );
    }

    #[test]
    fn last_color_scheme_assignment_wins() {
        let content = "\
config.color_scheme = (wezterm.gui and wezterm.gui.get_appearance() or 'Dark'):find('Dark') and 'Kaku Dark' or 'Kaku Light'
config.color_scheme = 'Kaku Light'
";

        assert_eq!(
            color_scheme_selection_from_content(content),
            Some(ColorSchemeSelection::Light)
        );
    }

    #[test]
    fn last_unknown_assignment_overrides_earlier_auto() {
        let content = "\
config.color_scheme = (wezterm.gui and wezterm.gui.get_appearance() or 'Dark'):find('Dark') and 'Kaku Dark' or 'Kaku Light'
config.color_scheme = some_runtime_value
";

        assert_eq!(
            color_scheme_selection_from_content(content),
            Some(ColorSchemeSelection::Other)
        );
    }

    #[test]
    fn stable_cache_hit_requires_matching_generation() {
        let theme = cached_theme(dark_palette());
        assert!(is_current_theme_cache_hit(4, 4, theme, None));
        assert!(!is_current_theme_cache_hit(4, 5, theme, None));
    }

    #[test]
    fn appearance_sensitive_cache_hit_requires_matching_appearance() {
        let theme = appearance_sensitive_theme(dark_palette(), true);
        assert!(is_current_theme_cache_hit(7, 7, theme, Some(true)));
        assert!(!is_current_theme_cache_hit(7, 7, theme, Some(false)));
    }

    #[test]
    fn pick_visible_keeps_accessible_candidate() {
        let bg = rgb("#15141B");
        let text = rgb("#D5D4D6");
        let candidate = rgb("#8E6AD9");

        assert_eq!(pick_visible(bg, text, &[candidate]), candidate);
    }

    #[test]
    fn pick_visible_pushes_dark_theme_fallback_toward_readable_text() {
        let bg = rgb("#15141B");
        let text = rgb("#D5D4D6");
        let fallback = rgb("#23212E");

        assert!(!has_enough_separation(bg, fallback));

        let adjusted = pick_visible(bg, text, &[fallback]);

        assert!(has_enough_separation(bg, adjusted));
        assert!(color_distance(bg, adjusted) > color_distance(bg, fallback));
        assert!(
            (luminance(bg) - luminance(adjusted)).abs()
                > (luminance(bg) - luminance(fallback)).abs()
        );
    }

    #[test]
    fn pick_visible_pushes_light_theme_fallback_toward_readable_text() {
        let bg = rgb("#FFFCF0");
        let text = rgb("#403E3C");
        let fallback = rgb("#E7E0CB");

        assert!(!has_enough_separation(bg, fallback));

        let adjusted = pick_visible(bg, text, &[fallback]);

        assert!(has_enough_separation(bg, adjusted));
        assert!(color_distance(bg, adjusted) > color_distance(bg, fallback));
        assert!(
            (luminance(bg) - luminance(adjusted)).abs()
                > (luminance(bg) - luminance(fallback)).abs()
        );
    }

    #[test]
    fn pick_visible_uses_extreme_anchor_when_text_is_not_readable() {
        let bg = rgb("#333333");
        let text = rgb("#3D3D3D");
        let fallback = rgb("#404060");

        assert!(!has_enough_separation(bg, fallback));
        assert!(!has_enough_separation(bg, text));

        let adjusted = pick_visible(bg, text, &[fallback]);

        assert!(has_enough_separation(bg, adjusted));
        assert!(luminance(adjusted) > luminance(fallback));
    }
}
