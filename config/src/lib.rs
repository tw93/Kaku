//! Configuration for the gui portion of the terminal
#![allow(clippy::comparison_to_empty)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::from_over_into)]
#![allow(clippy::large_const_arrays)]
#![allow(clippy::manual_contains)]
#![allow(clippy::manual_flatten)]
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::needless_question_mark)]
#![allow(clippy::needless_return)]
#![allow(clippy::never_loop)]
#![allow(clippy::new_without_default)]
#![allow(clippy::redundant_static_lifetimes)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::single_match)]
#![allow(clippy::to_string_trait_impl)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::wrong_self_convention)]

use anyhow::{anyhow, bail, Context, Error};
use lazy_static::lazy_static;
use mlua::Lua;
use ordered_float::NotNan;
use parking_lot::RwLock;
use smol::prelude::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs::{DirBuilder, OpenOptions};
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wezterm_dynamic::{FromDynamic, FromDynamicOptions, ToDynamic, UnknownFieldAction, Value};
use wezterm_term::UnicodeVersion;

mod background;
mod bell;
mod cell;
mod color;
mod config;
mod daemon;
mod exec_domain;
mod font;
mod frontend;
pub mod keyassignment;
mod keys;
pub mod lua;
pub mod meta;
pub mod proxy;
mod scheme_data;
mod serial;
mod ssh;
mod terminal;
mod tls;
mod units;
mod unix;
mod version;
pub mod window;

pub use crate::config::*;
pub use background::*;
pub use bell::*;
pub use cell::*;
pub use color::*;
pub use daemon::*;
pub use exec_domain::*;
pub use font::*;
pub use frontend::*;
pub use keys::*;
pub use serial::*;
pub use ssh::*;
pub use terminal::*;
pub use tls::*;
pub use units::*;
pub use unix::*;
pub use version::*;

type ErrorCallback = fn(&str);

lazy_static! {
    pub static ref HOME_DIR: PathBuf = dirs_next::home_dir().expect("can't find HOME dir");
    pub static ref CONFIG_DIRS: Vec<PathBuf> = config_dirs();
    pub static ref RUNTIME_DIR: PathBuf = compute_runtime_dir().unwrap();
    pub static ref DATA_DIR: PathBuf = compute_data_dir().unwrap();
    pub static ref CACHE_DIR: PathBuf = compute_cache_dir().unwrap();
    static ref CONFIG: Configuration = Configuration::new();
    static ref CONFIG_FILE_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);
    static ref CONFIG_SKIP: AtomicBool = AtomicBool::new(false);
    static ref CONFIG_OVERRIDES: Mutex<Vec<(String, String)>> = Mutex::new(vec![]);
    static ref SHOW_ERROR: Mutex<Option<ErrorCallback>> =
        Mutex::new(Some(|e| log::error!("{}", e)));
    static ref LUA_PIPE: LuaPipe = LuaPipe::new();
    pub static ref COLOR_SCHEMES: ColorSchemeRegistry = ColorSchemeRegistry::new();
}

thread_local! {
    static LUA_CONFIG: RefCell<Option<LuaConfigState>> = const { RefCell::new(None) };
}

fn toml_table_has_numeric_keys(t: &toml::value::Table) -> bool {
    t.keys().all(|k| k.parse::<isize>().is_ok())
}

fn json_object_has_numeric_keys(t: &serde_json::Map<String, serde_json::Value>) -> bool {
    t.keys().all(|k| k.parse::<isize>().is_ok())
}

fn toml_to_dynamic(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(s) => s.to_dynamic(),
        toml::Value::Integer(n) => n.to_dynamic(),
        toml::Value::Float(n) => n.to_dynamic(),
        toml::Value::Boolean(b) => b.to_dynamic(),
        toml::Value::Datetime(d) => d.to_string().to_dynamic(),
        toml::Value::Array(a) => a
            .iter()
            .map(toml_to_dynamic)
            .collect::<Vec<_>>()
            .to_dynamic(),
        // Allow `colors.indexed` to be passed through with actual integer keys
        toml::Value::Table(t) if toml_table_has_numeric_keys(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.parse::<isize>().unwrap().to_dynamic(), toml_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
        toml::Value::Table(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (Value::String(k.to_string()), toml_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
    }
}

fn json_to_dynamic(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => b.to_dynamic(),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_dynamic()
            } else if let Some(i) = n.as_u64() {
                i.to_dynamic()
            } else if let Some(f) = n.as_f64() {
                f.to_dynamic()
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => s.to_dynamic(),
        serde_json::Value::Array(a) => a
            .iter()
            .map(json_to_dynamic)
            .collect::<Vec<_>>()
            .to_dynamic(),
        // Allow `colors.indexed` to be passed through with actual integer keys
        serde_json::Value::Object(t) if json_object_has_numeric_keys(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.parse::<isize>().unwrap().to_dynamic(), json_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
        serde_json::Value::Object(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (Value::String(k.to_string()), json_to_dynamic(v)))
                .collect::<BTreeMap<_, _>>()
                .into(),
        ),
    }
}

pub fn build_default_schemes() -> HashMap<String, Palette> {
    let mut color_schemes = HashMap::new();
    for (scheme_name, data) in scheme_data::SCHEMES.iter() {
        let scheme_name = scheme_name.to_string();
        let scheme = match ColorSchemeFile::from_toml_str(data) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Failed to parse bundled color scheme {scheme_name:?}: {e:#}");
                continue;
            }
        };
        color_schemes.insert(scheme_name, scheme.colors.clone());
        for alias in scheme.metadata.aliases {
            color_schemes.insert(alias, scheme.colors.clone());
        }
    }
    color_schemes
}

/// Scheme names that Kaku defines itself. Settings lists them under their fixed
/// labels, so [`user_color_schemes`] never reports them.
const KAKU_OWN_SCHEMES: [&str; 3] = ["Kaku Dark", "Kaku Light", "Kaku Theme"];

/// Names of the color schemes the user provides.
///
/// Discovers `<config-dir>/colors/*.toml` for every config directory, every
/// directory listed in `color_scheme_dirs`, and the schemes the user's Lua
/// config defines directly in `color_schemes`. Only names are collected; the
/// palettes stay lazily loaded.
///
/// The scheme name is the file's `metadata.name`, falling back to the file
/// name without the `.toml` suffix, which is how the config loader resolves a
/// named scheme.
pub fn user_color_schemes() -> Vec<String> {
    let config = configuration();
    let mut dirs = config.color_scheme_dirs.clone();
    for dir in CONFIG_DIRS.iter() {
        dirs.push(dir.join("colors"));
    }
    collect_user_color_schemes(&dirs, &config.color_schemes)
}

fn collect_user_color_schemes(dirs: &[PathBuf], defined: &HashMap<String, Palette>) -> Vec<String> {
    let mut names = Vec::new();

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            // A missing colors directory is the common case.
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(stem) = file_name.strip_suffix(".toml") else {
                continue;
            };
            if !path.is_file() {
                continue;
            }

            let scheme = std::fs::read_to_string(&path)
                .context("reading color scheme")
                .and_then(|s| ColorSchemeFile::from_toml_str(&s).context("parsing TOML"));
            match scheme {
                Ok(scheme) => names.push(scheme.metadata.name.unwrap_or_else(|| stem.to_string())),
                Err(err) => {
                    // One broken file must not hide the other themes.
                    log::debug!(
                        "color scheme `{}` is not usable for the theme list: {:#}",
                        path.display(),
                        err
                    );
                }
            }
        }
    }

    for name in defined.keys() {
        if !KAKU_OWN_SCHEMES.contains(&name.as_str()) {
            names.push(name.clone());
        }
    }

    names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
    names.dedup();
    names.retain(|name| !name.trim().is_empty());
    names
}

/// Lazy-loading color scheme registry
/// Loads color schemes on-demand instead of eagerly loading all 1001 schemes at startup
pub struct ColorSchemeRegistry {
    loaded: RwLock<HashMap<String, Palette>>,
}

impl ColorSchemeRegistry {
    pub fn new() -> Self {
        Self {
            loaded: RwLock::new(HashMap::new()),
        }
    }

    /// Get a color scheme by name, loading it on-demand if not already cached
    /// Returns an owned Palette for API compatibility
    pub fn get(&self, name: &str) -> Option<Palette> {
        self.get_internal(name)
    }

    fn get_internal(&self, name: &str) -> Option<Palette> {
        // Fast path: check if already loaded
        {
            let loaded = self.loaded.read();
            if let Some(palette) = loaded.get(name) {
                return Some(palette.clone());
            }
        }

        // Slow path: search and load from scheme_data
        self.load_scheme(name)
    }

    fn load_scheme(&self, name: &str) -> Option<Palette> {
        for (scheme_name, data) in scheme_data::SCHEMES.iter() {
            let is_primary = *scheme_name == name;
            // Skip TOML parsing entirely when neither the primary name nor any alias
            // can possibly match. For primary-name lookups this is a no-op; for alias
            // lookups we parse once and reuse the same result for both the alias check
            // and the data extraction, so the TOML is never parsed twice.
            if !is_primary {
                let Ok(scheme) = ColorSchemeFile::from_toml_str(data) else {
                    continue;
                };
                if !scheme.metadata.aliases.iter().any(|a| a == name) {
                    continue;
                }
                // Alias matched: use the already-parsed scheme directly.
                let palette = scheme.colors.clone();
                let mut loaded = self.loaded.write();
                loaded.insert(scheme_name.to_string(), palette.clone());
                for alias in &scheme.metadata.aliases {
                    loaded.insert(alias.clone(), palette.clone());
                }
                return Some(palette);
            }

            // Primary name matched: parse once.
            if let Ok(scheme) = ColorSchemeFile::from_toml_str(data) {
                let palette = scheme.colors.clone();
                let mut loaded = self.loaded.write();
                loaded.insert(scheme_name.to_string(), palette.clone());
                for alias in &scheme.metadata.aliases {
                    loaded.insert(alias.clone(), palette.clone());
                }
                return Some(palette);
            }
            return None;
        }

        None
    }

    /// Get all available scheme names (without loading them)
    pub fn available_schemes() -> Vec<&'static str> {
        scheme_data::SCHEMES.iter().map(|(name, _)| *name).collect()
    }

    /// Clone all loaded schemes (for backward compatibility with .clone())
    /// Note: This will eagerly load ALL schemes if called before any are cached
    pub fn clone(&self) -> HashMap<String, Palette> {
        let loaded = self.loaded.read();

        // If nothing is loaded yet, load everything (backward compatibility)
        if loaded.is_empty() {
            drop(loaded);
            return build_default_schemes();
        }

        loaded.clone()
    }
}

/// Latest-wins pipe for Lua context.
/// Only keeps the most recent Lua context, discarding older ones.
struct LuaPipe {
    latest: Mutex<Option<mlua::Lua>>,
}
impl LuaPipe {
    pub fn new() -> Self {
        Self {
            latest: Mutex::new(None),
        }
    }

    /// Store a new Lua context, replacing any previous one.
    pub fn send(&self, lua: mlua::Lua) {
        *self.latest.lock().unwrap() = Some(lua);
    }

    /// Take the latest Lua context if available.
    pub fn try_recv(&self) -> Option<mlua::Lua> {
        self.latest.lock().unwrap().take()
    }
}

/// The implementation is only slightly crazy...
/// `Lua` is Send but !Sync.
/// We take care to reference this only from the main thread of
/// the application.
/// We also need to take care to keep this `lua` alive if a long running
/// future is outstanding while a config reload happens.
/// We have to use `Rc` to manage its lifetime, but due to some issues
/// with rust's async lifetime tracking we need to indirectly schedule
/// some of the futures to avoid it thinking that the generated future
/// in the async block needs to be Send.
///
/// A further complication is that config reloading tends to happen in
/// a background filesystem watching thread.
///
/// The result of all these constraints is that the LuaPipe struct above
/// is used as a channel to transport newly loaded lua configs to the
/// main thread.
///
/// The main thread pops the loaded configs to obtain the latest one
/// and updates LuaConfigState
struct LuaConfigState {
    lua: Option<Rc<mlua::Lua>>,
}

impl LuaConfigState {
    /// Consume any lua contexts sent to us via the
    /// config loader until we end up with the most
    /// recent one being referenced by LUA_CONFIG.
    fn update_to_latest(&mut self) {
        if let Some(lua) = LUA_PIPE.try_recv() {
            self.lua.replace(Rc::new(lua));
        }
    }

    /// Take a reference on the latest generation of the lua context
    fn get_lua(&self) -> Option<Rc<mlua::Lua>> {
        self.lua.as_ref().map(Rc::clone)
    }
}

pub fn designate_this_as_the_main_thread() {
    LUA_CONFIG.with(|lc| {
        let mut lc = lc.borrow_mut();
        if lc.is_none() {
            lc.replace(LuaConfigState { lua: None });
        }
    });
}

#[must_use = "Cancels the subscription when dropped"]
pub struct ConfigSubscription(usize);

impl Drop for ConfigSubscription {
    fn drop(&mut self) {
        CONFIG.unsub(self.0);
    }
}

pub fn subscribe_to_config_reload<F>(subscriber: F) -> ConfigSubscription
where
    F: Fn() -> bool + 'static + Send + Sync,
{
    ConfigSubscription(CONFIG.subscribe(subscriber))
}

/// Spawn a future that will run with an optional Lua state from the most
/// recently loaded lua configuration.
/// The `func` argument is passed the lua state and must return a Future.
///
/// This function MUST only be called from the main thread.
/// In exchange for the caller checking for this, the parameters to
/// this method are not required to be Send.
///
/// Calling this function from a secondary thread will panic.
/// You should use `with_lua_config` if you are triggering a
/// call from a secondary thread.
pub async fn with_lua_config_on_main_thread<F, RETF, RET>(func: F) -> anyhow::Result<RET>
where
    F: FnOnce(Option<Rc<mlua::Lua>>) -> RETF,
    RETF: Future<Output = anyhow::Result<RET>>,
{
    let lua = LUA_CONFIG.with(|lc| {
        let mut lc = lc.borrow_mut();
        let lc = lc.as_mut().expect(
            "with_lua_config_on_main_thread not called
             from main thread, use with_lua_config instead!",
        );
        lc.update_to_latest();
        lc.get_lua()
    });

    func(lua).await
}

pub fn run_immediate_with_lua_config<F, RET>(func: F) -> anyhow::Result<RET>
where
    F: FnOnce(Option<Rc<mlua::Lua>>) -> anyhow::Result<RET>,
{
    let lua = LUA_CONFIG.with(|lc| {
        let mut lc = lc.borrow_mut();
        let lc = lc.as_mut().expect(
            "with_lua_config_on_main_thread not called
             from main thread, use with_lua_config instead!",
        );
        lc.update_to_latest();
        lc.get_lua()
    });

    func(lua)
}

fn schedule_with_lua<F, RETF, RET>(func: F) -> promise::spawn::Task<anyhow::Result<RET>>
where
    F: 'static,
    RET: 'static,
    F: Fn(Option<Rc<mlua::Lua>>) -> RETF,
    RETF: Future<Output = anyhow::Result<RET>>,
{
    promise::spawn::spawn(async move { with_lua_config_on_main_thread(func).await })
}

/// Spawn a future that will run with an optional Lua state from the most
/// recently loaded lua configuration.
/// The `func` argument is passed the lua state and must return a Future.
pub async fn with_lua_config<F, RETF, RET>(func: F) -> anyhow::Result<RET>
where
    F: Fn(Option<Rc<mlua::Lua>>) -> RETF,
    RETF: Future<Output = anyhow::Result<RET>> + Send + 'static,
    F: Send + 'static,
    RET: Send + 'static,
{
    promise::spawn::spawn_into_main_thread(async move { schedule_with_lua(func).await }).await
}

fn default_config_with_overrides_applied() -> anyhow::Result<Config> {
    // Cause the default config to be re-evaluated with the overrides applied
    let lua = lua::make_lua_context(Path::new("override")).context("make_lua_context")?;
    let table = mlua::Value::Table(lua.create_table()?);
    let config = Config::apply_overrides_to(&lua, table).context("apply_overrides_to")?;

    let dyn_config = luahelper::lua_value_to_dynamic(config)?;

    let cfg: Config = Config::from_dynamic(
        &dyn_config,
        FromDynamicOptions {
            unknown_fields: UnknownFieldAction::Deny,
            deprecated_fields: UnknownFieldAction::Warn,
        },
    )
    .context("Error converting lua value from overrides to Config struct")?;

    cfg.check_consistency().context("check_consistency")?;

    Ok(cfg)
}

pub fn common_init(
    config_file: Option<&OsString>,
    overrides: &[(String, String)],
    skip_config: bool,
) -> anyhow::Result<()> {
    if let Some(config_file) = config_file {
        set_config_file_override(Path::new(config_file));
    } else if skip_config {
        CONFIG_SKIP.store(true, Ordering::Relaxed);
    }

    set_config_overrides(overrides).context("common_init: set_config_overrides")?;
    reload();
    Ok(())
}

pub fn defer_watchers_until_enabled() {
    CONFIG.defer_watchers_until_enabled();
}

pub fn enable_deferred_watchers() {
    CONFIG.enable_deferred_watchers();
}

pub fn assign_error_callback(cb: ErrorCallback) {
    let mut factory = SHOW_ERROR.lock().unwrap();
    factory.replace(cb);
}

pub fn show_error(err: &str) {
    let factory = SHOW_ERROR.lock().unwrap();
    if let Some(cb) = factory.as_ref() {
        cb(err)
    }
}

pub fn create_user_owned_dirs(p: &Path) -> anyhow::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    {
        builder.mode(0o700);
    }

    builder.create(p)?;
    Ok(())
}

pub fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path)
            .map(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
            .unwrap_or(false);
    }
}

pub fn user_config_path() -> PathBuf {
    CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| HOME_DIR.join(".config").join("kaku"))
        .join("kaku.lua")
}

fn effective_config_file_path_from(
    config_file_override: Option<PathBuf>,
    loaded_config_file: Option<OsString>,
    default_path: PathBuf,
) -> PathBuf {
    if let Some(path) = config_file_override {
        return path;
    }
    if let Some(path) = loaded_config_file {
        return path.into();
    }
    default_path
}

/// Returns the currently effective config file path.
///
/// Priority:
/// 1) explicit `--config-file` override
/// 2) path of the loaded config (`KAKU_CONFIG_FILE`)
/// 3) default user config path
pub fn effective_config_file_path() -> PathBuf {
    let config_file_override = CONFIG_FILE_OVERRIDE.lock().unwrap().clone();
    effective_config_file_path_from(
        config_file_override,
        std::env::var_os("KAKU_CONFIG_FILE"),
        user_config_path(),
    )
}

pub fn ensure_user_config_exists() -> anyhow::Result<PathBuf> {
    let config_path = user_config_path();
    ensure_config_exists_at_path(&config_path)
}

pub fn ensure_config_exists_at_path(config_path: &std::path::Path) -> anyhow::Result<PathBuf> {
    if config_path.exists() {
        let metadata = std::fs::metadata(config_path)
            .with_context(|| format!("stat user config path {}", config_path.display()))?;
        if metadata.is_file() {
            return Ok(config_path.to_path_buf());
        }
        bail!(
            "user config path exists but is not a regular file: {}",
            config_path.display()
        );
    }

    let parent = config_path
        .parent()
        .ok_or_else(|| anyhow!("invalid config path: {}", config_path.display()))?;
    create_user_owned_dirs(parent).context("create config directory")?;

    write_new_file_atomic(config_path, minimal_user_config_template().as_bytes())
        .context("write minimal user config file")?;
    Ok(config_path.to_path_buf())
}

/// Atomically writes data to a file using a temporary file and rename.
///
/// This function ensures that the file is either completely written or not modified at all,
/// preventing partial writes from the *application's* perspective (process crash). It creates
/// a temporary file in the same directory as the target, writes the data, flushes OS buffers
/// (not necessarily to disk), then renames the temp file to the target atomically.
///
/// Note: This does not guarantee durability against power failures (no fsync/sync_all).
/// For config files this is usually acceptable; use sync_all if you need full durability.
///
/// # Arguments
/// * `path` - The target file path
/// * `data` - The bytes to write
///
/// # Returns
/// * `Ok(())` on success
/// * `Err` if any step fails (directory creation, temp file creation, write, flush, or rename)
///
/// # Retry Logic
/// If the temp file already exists (e.g., from a previous interrupted attempt), it will
/// retry with a different suffix up to 8 times before giving up.
fn write_new_file_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid atomic write path: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid atomic write file name: {}", path.display()))?;

    // Note: unwrap_or_default is safe here because PID + attempt counter ensure
    // uniqueness even if the system clock is before UNIX_EPOCH (which would
    // only happen with severe clock misconfiguration).
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();

    let mut last_err = None;
    for attempt in 0..8 {
        let tmp_path = parent.join(format!(
            ".{}.tmp-{}-{}-{}",
            file_name, pid, now_nanos, attempt
        ));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(mut file) => {
                if let Err(err) = (|| -> std::io::Result<()> {
                    file.write_all(data)?;
                    file.flush()?;
                    Ok(())
                })() {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(err).with_context(|| {
                        format!("write temporary config file {}", tmp_path.display())
                    });
                }

                if let Err(err) = std::fs::rename(&tmp_path, path) {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(err).with_context(|| {
                        format!(
                            "move temporary config file {} into place at {}",
                            tmp_path.display(),
                            path.display()
                        )
                    });
                }

                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_err = Some(err);
                continue;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("create temporary config file {}", tmp_path.display())
                });
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary config file path collision",
        )
    }))
    .with_context(|| format!("allocate temporary config file for {}", path.display()))
}

fn minimal_user_config_template() -> &'static str {
    r#"local wezterm = require 'wezterm'

local function resolve_bundled_config()
  local resource_dir = wezterm.executable_dir:gsub('MacOS/?$', 'Resources')
  local bundled = resource_dir .. '/kaku.lua'
  local f = io.open(bundled, 'r')
  if f then
    f:close()
    return bundled
  end

  local dev_bundled = wezterm.executable_dir .. '/../../assets/macos/Kaku.app/Contents/Resources/kaku.lua'
  f = io.open(dev_bundled, 'r')
  if f then
    f:close()
    return dev_bundled
  end

  local app_bundled = '/Applications/Kaku.app/Contents/Resources/kaku.lua'
  f = io.open(app_bundled, 'r')
  if f then
    f:close()
    return app_bundled
  end

  local home = os.getenv('HOME') or ''
  local home_bundled = home .. '/Applications/Kaku.app/Contents/Resources/kaku.lua'
  f = io.open(home_bundled, 'r')
  if f then
    f:close()
    return home_bundled
  end

  return nil
end

local config = {}
local bundled = resolve_bundled_config()

if bundled then
  local ok, loaded = pcall(dofile, bundled)
  if ok and type(loaded) == 'table' then
    config = loaded
  else
    wezterm.log_error('Kaku: failed to load bundled defaults from ' .. bundled)
  end
else
  wezterm.log_error('Kaku: bundled defaults not found')
end

-- New installs start in Kaku Dark. Pick another theme in Settings, or change this line:
-- 'Kaku Light', or 'Auto' to follow macOS appearance.
config.color_scheme = 'Kaku Dark'

-- User overrides:
-- Kaku intentionally keeps WezTerm-compatible Lua API names
-- for maximum compatibility, so `wezterm.*` here is expected.
-- Full API docs: https://wezfurlong.org/wezterm/config/lua/
-- Kaku options:  https://github.com/tw93/Kaku/blob/main/docs/configuration.md
--
-- Changes apply automatically when you save this file.
-- Every example below differs from the default, so uncommenting it takes effect.
--
-- 1) Font family and size (default: JetBrains Mono, size auto 15/17)
-- config.font = wezterm.font('Menlo')
-- config.font_size = 16.0
-- config.line_height = 1.1  -- default 1.28; use 1.0-1.1 if QR codes or TUI charts look stretched
--
-- 2) Color scheme (a fixed scheme disables light/dark auto switching)
-- config.color_scheme = 'Catppuccin Mocha'
--
-- 3) Window size and padding (default: 110x22, 40px sides, 0 bottom)
-- config.initial_cols = 120
-- config.initial_rows = 30
-- config.window_padding = { left = '24px', right = '24px', top = '40px', bottom = '20px' }
--
-- 4) Window transparency and blur
-- config.window_background_opacity = 0.95            -- opacity, 0.0 to 1.0
-- config.macos_window_background_blur = 20           -- blur radius, integer 0 to 100
--
-- 5) Selection and quit behavior
-- config.copy_on_select = false                      -- default true
-- config.window_close_confirmation = 'NeverPrompt'   -- default 'SmartPrompt'
--
-- 6) Default shell/program (default: your login shell)
-- config.default_prog = { '/opt/homebrew/bin/fish', '-l' }
--
-- 7) Cursor and scrollback
-- config.default_cursor_style = 'SteadyBlock'        -- default 'BlinkingBar'
-- config.cursor_blink_rate = 0                       -- 0 disables blinking; default 500
-- config.scrollback_lines = 20000                    -- default 10000
-- config.file_link_editor = 'zed'                    -- opens path[:line[:column]]
--
-- 8) Tab bar
-- config.tab_bar_at_bottom = false                   -- default true
-- config.hide_tab_bar_if_only_one_tab = false        -- default true
-- config.tab_title_show_basename_only = true
-- config.tab_title_show_foreground_process = true    -- show "dirname·codex" while commands run
--
-- 9) Tab key completion (default 'suggestion_first': Tab accepts the inline suggestion)
-- config.smart_tab_mode = 'completion_first'
--
-- 10) Working directory inheritance (default: all true)
-- config.window_inherit_working_directory = false
-- config.tab_inherit_working_directory = false
-- config.split_pane_inherit_working_directory = false
--
-- 11) Split pane
-- config.split_pane_gap = 6                          -- default 2
-- config.inactive_pane_hsb = { hue = 0.95, saturation = 1.0, brightness = 0.9 }
--
-- 12) Fullscreen (set false if you use yabai/AeroSpace tiling)
-- config.native_macos_fullscreen_mode = false
--
-- 13) Add a key binding (defaults: docs/keybindings.md)
-- table.insert(config.keys, {
--   key = 'k',
--   mods = 'CMD|SHIFT',
--   action = wezterm.action.ClearScrollback('ScrollbackAndViewport'),
-- })
--
-- AI assistant settings (provider, model, API key) live in
-- ~/.config/kaku/assistant.toml, not in this file.

return config
"#
}

fn xdg_config_home_from(home_dir: &Path, xdg_config_home: Option<OsString>) -> PathBuf {
    // Normalize empty env values to "unset" to preserve HOME/.config fallback behavior.
    xdg_config_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".config"))
        .join("kaku")
}

fn config_dirs_from(
    home_dir: &Path,
    xdg_config_home: Option<OsString>,
    #[cfg(unix)] xdg_config_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let mut dirs = vec![xdg_config_home_from(home_dir, xdg_config_home)];

    #[cfg(unix)]
    if let Some(d) = xdg_config_dirs.filter(|value| !value.is_empty()) {
        dirs.extend(
            std::env::split_paths(&d)
                // `XDG_CONFIG_DIRS` may contain empty segments (e.g. `::`).
                .filter(|path| !path.as_os_str().is_empty())
                .map(|path| path.join("kaku")),
        );
    }

    dirs
}

fn config_dirs() -> Vec<PathBuf> {
    config_dirs_from(
        &HOME_DIR,
        std::env::var_os("XDG_CONFIG_HOME"),
        #[cfg(unix)]
        std::env::var_os("XDG_CONFIG_DIRS"),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn builtin_kaku_scheme_mirrors_match_complete_bundled_palettes() {
        let source = include_str!("../../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let constants = source
            .split_once("local KAKU = {")
            .expect("Kaku constants")
            .1
            .split_once("\n}")
            .expect("end of Kaku constants")
            .0;
        let schemes = source
            .split_once("config.color_schemes = config.color_schemes or {}")
            .expect("Kaku schemes")
            .1
            .split_once("config.color_scheme = resolve_kaku_color_scheme")
            .expect("end of Kaku schemes")
            .0;
        // Evaluate only palette literals, without startup handlers or user config.
        let lua = mlua::Lua::new();
        let palettes: std::collections::HashMap<String, crate::Palette> = lua.load(format!(
            "local config = {{ color_schemes = {{}} }}\nlocal KAKU = {{{constants}\n}}\n{schemes}\nreturn config.color_schemes"
        )).eval().expect("bundled palette literals");
        for name in ["Kaku Dark", "Kaku Light", "Kaku Theme"] {
            let expected = palettes.get(name).expect("bundled Kaku palette");
            assert_eq!(
                crate::ColorSchemeRegistry::new().get(name),
                Some(expected.clone()),
                "{name}"
            );
        }
    }

    #[test]
    fn builtin_kaku_schemes_work_without_loading_bundled_defaults() {
        for (name, background) in [
            ("Kaku Dark", (0x15, 0x14, 0x1b)),
            ("Kaku Light", (0xff, 0xfc, 0xf0)),
            ("Kaku Theme", (0x15, 0x14, 0x1b)),
        ] {
            let mut config = crate::Config::default();
            config.color_scheme = Some(name.to_string());
            let palette = config.resolve_color_scheme().expect("built-in Kaku scheme");
            assert_eq!(palette.background, Some(background.into()));
            assert_eq!(
                config
                    .compute_extra_defaults(None)
                    .resolved_palette
                    .background,
                Some(background.into())
            );
            if name != "Kaku Theme" {
                assert!(crate::ColorSchemeRegistry::available_schemes().contains(&name));
            }
            assert_eq!(crate::build_default_schemes().get(name), Some(&palette));
        }
    }

    #[test]
    fn user_kaku_scheme_definition_takes_priority_over_builtin() {
        let mut config = crate::Config::default();
        config.color_scheme = Some("Kaku Light".to_string());
        let palette = crate::Palette {
            background: Some((0x12, 0x34, 0x56).into()),
            ..Default::default()
        };
        config
            .color_schemes
            .insert("Kaku Light".into(), palette.clone());
        assert_eq!(config.resolve_color_scheme(), Some(palette));
    }

    use super::*;

    #[test]
    fn empty_xdg_config_home_uses_default_home_config_dir() {
        let home = PathBuf::from("/tmp/kaku-home");
        let path = xdg_config_home_from(&home, Some(OsString::new()));
        assert_eq!(path, home.join(".config").join("kaku"));
    }

    #[test]
    fn missing_xdg_config_home_uses_default_home_config_dir() {
        let home = PathBuf::from("/tmp/kaku-home");
        let path = xdg_config_home_from(&home, None);
        assert_eq!(path, home.join(".config").join("kaku"));
    }

    #[test]
    fn valid_xdg_config_home_is_used() {
        let home = PathBuf::from("/tmp/kaku-home");
        let path = xdg_config_home_from(&home, Some(OsString::from("/custom/config")));
        assert_eq!(path, PathBuf::from("/custom/config").join("kaku"));
    }

    #[cfg(unix)]
    #[test]
    fn empty_xdg_config_dirs_entries_are_ignored() {
        let home = PathBuf::from("/tmp/kaku-home");
        let dirs = config_dirs_from(
            &home,
            Some(OsString::new()),
            Some(OsString::from("/etc/xdg::/usr/local/etc/xdg")),
        );
        assert_eq!(
            dirs,
            vec![
                home.join(".config").join("kaku"),
                PathBuf::from("/etc/xdg").join("kaku"),
                PathBuf::from("/usr/local/etc/xdg").join("kaku"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_xdg_config_dirs_returns_primary_only() {
        let home = PathBuf::from("/tmp/kaku-home");
        let dirs = config_dirs_from(&home, Some(OsString::from("/custom/config")), None);
        assert_eq!(dirs, vec![PathBuf::from("/custom/config").join("kaku")]);
    }

    #[cfg(unix)]
    #[test]
    fn empty_xdg_config_dirs_returns_primary_only() {
        let home = PathBuf::from("/tmp/kaku-home");
        let dirs = config_dirs_from(
            &home,
            Some(OsString::from("/custom/config")),
            Some(OsString::new()),
        );
        assert_eq!(dirs, vec![PathBuf::from("/custom/config").join("kaku")]);
    }

    #[test]
    fn effective_config_file_path_prefers_override() {
        let path = effective_config_file_path_from(
            Some(PathBuf::from("/override/kaku.lua")),
            Some(OsString::from("/loaded/kaku.lua")),
            PathBuf::from("/default/kaku.lua"),
        );
        assert_eq!(path, PathBuf::from("/override/kaku.lua"));
    }

    #[test]
    fn effective_config_file_path_uses_loaded_when_no_override() {
        let path = effective_config_file_path_from(
            None,
            Some(OsString::from("/loaded/kaku.lua")),
            PathBuf::from("/default/kaku.lua"),
        );
        assert_eq!(path, PathBuf::from("/loaded/kaku.lua"));
    }

    #[test]
    fn effective_config_file_path_falls_back_to_default() {
        let path = effective_config_file_path_from(None, None, PathBuf::from("/default/kaku.lua"));
        assert_eq!(path, PathBuf::from("/default/kaku.lua"));
    }

    #[test]
    fn bundled_kaku_lua_defaults_missing_theme_to_appearance() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains(
                "if not scheme or scheme == '' then\n    return resolve_appearance_color_scheme()"
            ),
            "bundled kaku.lua should resolve a missing color_scheme via appearance"
        );
    }

    #[test]
    fn minimal_user_config_starts_new_installs_in_dark() {
        let content = minimal_user_config_template();

        // Only a freshly generated config pins Kaku Dark. Existing installs
        // already have a user config without this line, so they keep the
        // bundled default that follows macOS appearance.
        assert!(
            content.contains("\nconfig.color_scheme = 'Kaku Dark'\n"),
            "generated user config should start new installs in Kaku Dark"
        );
        assert!(
            content.contains("'Kaku Light'") && content.contains("'Auto'"),
            "generated user config should name the other theme choices"
        );
    }

    #[test]
    fn minimal_user_config_documents_inactive_pane_hue() {
        let content = minimal_user_config_template();

        assert!(
            content.contains(
                "config.inactive_pane_hsb = { hue = 0.95, saturation = 1.0, brightness = 0.9 }"
            ),
            "generated user config should expose every inactive-pane HSB component"
        );
    }

    #[test]
    fn bundled_kaku_lua_uses_config_for_remember_last_cwd() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("return config.remember_last_cwd ~= false"),
            "bundled kaku.lua should read remember_last_cwd from the parsed config table"
        );
    }

    #[test]
    fn bundled_kaku_lua_multi_pane_titles_keep_full_segments() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content
                .contains("-- Multi-pane path: render each pane's cwd, active segment highlighted")
                && content.contains(r"local sep = '\u{2219}'")
                && content.contains("local sep_width = wezterm.column_width(sep)")
                && content.contains("local w = (#segs - 1) * sep_width")
                && content
                    .contains("local avail = budget - math.max(0, #segments - 1) * sep_width"),
            "bundled kaku.lua should keep the multi-pane tab title path"
        );
        assert!(
            !content.contains("local w = (#segs - 1) * 3")
                && !content.contains("local avail = budget - (#segments - 1) * 3"),
            "multi-pane width accounting must follow the rendered separator width"
        );
        assert!(
            !content.contains("Trim non-active segments"),
            "multi-pane tab titles should keep pane context until the width budget requires truncation"
        );
    }

    #[test]
    fn bundled_kaku_lua_tab_titles_fit_narrow_width_budgets() -> anyhow::Result<()> {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let source = std::fs::read_to_string(&bundled)?;
        let lua = crate::lua::make_lua_context(&bundled)?;
        let wezterm: mlua::Table = lua.load("return require 'wezterm'").eval()?;
        wezterm.set(
            "column_width",
            lua.create_function(|_, text: String| {
                Ok(termwiz::cell::unicode_column_width(&text, None))
            })?,
        )?;
        wezterm.set(
            "truncate_right",
            lua.create_function(|_, (text, limit): (String, usize)| {
                let mut truncated = String::new();
                let mut width = 0;
                for ch in text.chars() {
                    let ch_width = termwiz::cell::unicode_column_width(&ch.to_string(), None);
                    if width + ch_width > limit {
                        break;
                    }
                    truncated.push(ch);
                    width += ch_width;
                }
                Ok(truncated)
            })?,
        )?;
        smol::block_on(
            lua.load(&source)
                .set_name(bundled.to_string_lossy())
                .eval_async::<mlua::Value>(),
        )?;

        // (pane_count, tab_index, show_index, same_dir)
        for (pane_count, tab_index, show_index, same_dir) in [
            (1, 0, false, false),
            (1, 0, true, false),
            (4, 0, true, false),
            (4, 9, true, false),
            // every pane in one directory: the segments merge into a single one,
            // which is the shape a split tab takes when it loses its number
            (3, 1, true, true),
        ] {
            // A split tab whose panes all sit in one directory merges into one
            // segment, so the fixture gets a single shared cwd as well.
            let pane_cwd = if same_dir {
                "'file:///Users/test/apps/psygo'".to_string()
            } else {
                "('file:///Users/test/projects/repository-' .. i)".to_string()
            };
            let fixture = format!(
                r#"
local panes = {{}}
for i = 1, {pane_count} do
  panes[i] = {{
    pane_id = i,
    pane_index = i - 1,
    is_active = i == {pane_count},
    is_zoomed = false,
    title = 'pane-' .. i,
    current_working_dir = {pane_cwd},
    user_vars = {{}},
  }}
end
local tab = {{
  tab_id = 1,
  tab_index = {tab_index},
  is_active = false,
  active_pane = panes[{pane_count}],
  panes = panes,
  tab_title = '',
  status = 'none',
  has_unread_bell = false,
}}
local effective_config = {{
  bell_tab_indicator = true,
  show_tab_index_in_tab_bar = {show_index},
  resolved_palette = {{
    tab_bar = {{
      background = '#15141b',
      active_tab = {{ fg_color = '#ffffff' }},
      inactive_tab = {{ fg_color = '#888888' }},
      inactive_tab_hover = {{ fg_color = '#aaaaaa' }},
    }},
  }},
}}
return tab, {{ tab }}, panes, effective_config
"#
            );
            let (tab, tabs, panes, config): (mlua::Table, mlua::Table, mlua::Table, mlua::Table) =
                lua.load(&fixture).eval()?;

            // 16 columns leaves room for the merged "apps/psygo" segment.
            for max_width in 1..=16 {
                let value = crate::lua::emit_sync_callback(
                    &lua,
                    (
                        "format-tab-title".to_string(),
                        (
                            tab.clone(),
                            tabs.clone(),
                            panes.clone(),
                            config.clone(),
                            false,
                            max_width,
                        ),
                    ),
                )?;
                let items = match value {
                    mlua::Value::Table(items) => items,
                    other => anyhow::bail!("expected format items, got {other:?}"),
                };
                let mut text = String::new();
                let mut last_text = None;
                for item in items.sequence_values::<mlua::Table>() {
                    let item = item?;
                    if let Some(part) = item.get::<_, Option<String>>("Text")? {
                        text.push_str(&part);
                        last_text = Some(part);
                    }
                }
                let rendered_width = termwiz::cell::unicode_column_width(&text, None);
                assert!(
                    rendered_width <= max_width,
                    "{} pane title rendered {} columns for a {}-column budget: {:?}",
                    pane_count,
                    rendered_width,
                    max_width,
                    text
                );
                // Regression: the multi-pane branch ignored
                // show_tab_index_in_tab_bar, so split tabs lost the index the
                // single-pane branch and the Rust fallback both draw.
                let index = format!("{}:", tab_index + 1);
                if !show_index {
                    assert!(
                        !text.contains(&index),
                        "show_tab_index_in_tab_bar = false must not draw {:?}: {:?}",
                        index,
                        text
                    );
                } else if pane_count == 1 {
                    // The single-pane path truncates one text run, so the first digit
                    // of the index survives as soon as the budget fits the leading cell.
                    if max_width >= 4 {
                        assert!(
                            text.starts_with(&format!(" {}", &index[..1])),
                            "single-pane title should keep the tab index in a {}-column budget: {:?}",
                            max_width,
                            text
                        );
                    }
                } else if max_width >= index.len() + 2 {
                    // Leading space + index + the trailing status cell.
                    assert!(
                        text.starts_with(&format!(" {index}")),
                        "multi-pane title should keep the tab index in a {}-column budget: {:?}",
                        max_width,
                        text
                    );
                    if max_width == index.len() + 2 {
                        // The index and the status cell are all that budget affords,
                        // so the segments are dropped whole rather than clipped.
                        assert_eq!(text, format!(" {index} "));
                    } else if same_dir && max_width >= index.len() + "apps/psygo".len() + 2 {
                        // Leading cell + index + the merged segment + the status cell.
                        assert_eq!(text, format!(" {index}apps/psygo "));
                    }
                } else {
                    assert!(
                        !text.contains(&index),
                        "a {}-column multi-pane title cannot afford {:?}: {:?}",
                        max_width,
                        index,
                        text
                    );
                }
                assert_eq!(
                    last_text.as_deref(),
                    Some(" "),
                    "the trailing status slot must survive a {max_width}-column budget"
                );
            }

            if pane_count == 1 {
                let render_status = |status: &str,
                                     has_unread_bell: bool|
                 -> anyhow::Result<(String, Option<String>)> {
                    tab.set("status", status)?;
                    tab.set("has_unread_bell", has_unread_bell)?;
                    let value = crate::lua::emit_sync_callback(
                        &lua,
                        (
                            "format-tab-title".to_string(),
                            (
                                tab.clone(),
                                tabs.clone(),
                                panes.clone(),
                                config.clone(),
                                false,
                                32,
                            ),
                        ),
                    )?;
                    let items = match value {
                        mlua::Value::Table(items) => items,
                        other => anyhow::bail!("expected format items, got {other:?}"),
                    };
                    let mut last_text = String::new();
                    let mut last_color = None;
                    for item in items.sequence_values::<mlua::Table>() {
                        let item = item?;
                        if let Some(foreground) =
                            item.get::<_, Option<mlua::Table>>("Foreground")?
                        {
                            last_color = foreground.get::<_, Option<String>>("Color")?;
                        }
                        if let Some(text) = item.get::<_, Option<String>>("Text")? {
                            last_text = text;
                        }
                    }
                    Ok((last_text, last_color))
                };

                assert_eq!(render_status("running", false)?.0, " ");
                assert_eq!(
                    render_status("attention", false)?,
                    ("\u{2022}".to_string(), Some("#daae76".to_string()))
                );
                assert_eq!(
                    render_status("running", true)?,
                    ("\u{2022}".to_string(), Some("#daae76".to_string()))
                );
                assert_eq!(render_status("none", false)?.0, " ");
            }
        }

        Ok(())
    }

    #[test]
    fn bundled_kaku_lua_pane_titles_are_opt_in_and_keep_manual_renames() -> anyhow::Result<()> {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let source = std::fs::read_to_string(&bundled)?;
        let lua = crate::lua::make_lua_context(&bundled)?;
        let wezterm: mlua::Table = lua.load("return require 'wezterm'").eval()?;
        wezterm.set(
            "column_width",
            lua.create_function(|_, text: String| {
                Ok(termwiz::cell::unicode_column_width(&text, None))
            })?,
        )?;
        smol::block_on(lua.load(&source).eval_async::<mlua::Value>())?;
        let tab_callbacks: mlua::Table =
            lua.named_registry_value("wezterm-event-format-tab-title")?;
        let window_callbacks: mlua::Table =
            lua.named_registry_value("wezterm-event-format-window-title")?;
        lua.globals().set(
            "test_tab_callback",
            tab_callbacks.get::<_, mlua::Function>(1)?,
        )?;
        lua.globals().set(
            "test_window_callback",
            window_callbacks.get::<_, mlua::Function>(1)?,
        )?;
        lua.load(r#"
local tab_callback = test_tab_callback
local window_callback = test_window_callback
local p1 = { pane_id = 1, title = 'test-title', current_working_dir = 'file:///tmp/project', user_vars = {} }
local p2 = { pane_id = 2, title = 'other-title', current_working_dir = 'file:///tmp/other', user_vars = {} }
local tab = { tab_id = 1, tab_index = 0, tab_title = '', active_pane = p1, panes = {p1}, status = 'none' }
local cfg = { resolved_palette = {tab_bar = {}}, tab_title_use_pane_title = true }
local function title_text()
  local text = ''
  for _, item in ipairs(tab_callback(tab, {tab}, tab.panes, cfg, false, 32)) do
    text = text .. (item.Text or '')
  end
  return text:match('^%s*(.-)%s*$')
end
assert(title_text() == 'test-title')
assert(window_callback(tab, p1, {tab}, tab.panes, cfg) == 'test-title')
tab.panes = {p1, p2}
assert(title_text() == 'test-title')
tab.tab_title = 'manual-name'
assert(title_text() == 'manual-name')
assert(window_callback(tab, p1, {tab}, tab.panes, cfg) == 'manual-name')
tab.tab_title = ''
tab.panes = {p1}
cfg.tab_title_use_pane_title = false
assert(title_text() ~= 'test-title')
cfg.tab_title_use_pane_title = true
p1.title = ''
assert(title_text() ~= '')
cfg.tab_title_use_pane_title = false
local fallback = title_text()
cfg.tab_title_use_pane_title = true
p1.title = 'kaku'
assert(title_text() == fallback)
tab.panes = {p1, p2}
cfg.tab_title_use_pane_title = false
local split_fallback = title_text()
cfg.tab_title_use_pane_title = true
assert(title_text() == split_fallback)
"#).exec()?;
        Ok(())
    }

    #[test]
    fn bundled_kaku_lua_foreground_process_tab_titles_default_to_off() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("config.tab_title_show_foreground_process = false")
                && content.contains("tab_title_show_foreground_process == true"),
            "foreground process tab titles should stay opt-in in bundled kaku.lua"
        );
    }

    #[test]
    fn bundled_kaku_lua_maps_claude_versioned_executables() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("is_version_like_process_name(name)")
                && content.contains("lowered:find('/claude/versions/', 1, true)")
                && content.contains("return 'claude'"),
            "bundled kaku.lua should title Claude Code versioned executables as claude"
        );
    }

    #[test]
    fn bundled_kaku_lazygit_dispatch_does_not_require_outer_pane_git_cwd() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");
        let launch_start = content
            .find("local function launch_lazygit(window, pane)")
            .expect("bundled kaku.lua should define launch_lazygit");
        let launch = &content[launch_start..];
        let launch_end = launch
            .find("\nlocal function launch_yazi")
            .expect("launch_lazygit should end before launch_yazi");
        let launch = &launch[..launch_end];

        assert!(
            !launch.contains("detect_git_repo_root(path)"),
            "lazygit dispatch must not reject an outer pane cwd; nested shells own their real PWD"
        );
        assert!(
            !launch.contains("kaku-toast-lazygit-not-git"),
            "lazygit dispatch must let lazygit report non-git directories itself"
        );
        assert!(
            launch.contains("wezterm.action.SendString(\"\\x15\" .. lazygit_cmd .. \"\\r\")"),
            "lazygit dispatch should retain the clear-line then command send sequence"
        );
    }

    #[test]
    fn bundled_kaku_lua_sets_colorfgbg_from_user_theme_scan() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("local initial_is_light_theme = is_user_light_theme()")
                && content.contains(
                    "config.set_environment_variables['COLORFGBG'] = initial_is_light_theme and '0;15' or '15;0'"
                ),
            "COLORFGBG must follow the user's intended theme, not the bundled pre-override color_scheme"
        );
    }

    #[test]
    fn bundled_kaku_lua_closes_fullscreen_last_window_on_cmd_w() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("local is_full_screen = dims and dims.is_full_screen")
                && content.contains("if should_close_tab or is_full_screen then"),
            "Cmd+W should close the last fullscreen tab instead of hiding the app"
        );
    }

    #[test]
    fn bundled_kaku_lua_defaults_close_confirmation_to_smart_prompt() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("config.tab_close_confirmation = 'SmartPrompt'")
                && content.contains("config.pane_close_confirmation = 'SmartPrompt'")
                && content.contains("config.window_close_confirmation = 'SmartPrompt'"),
            "bundled kaku.lua should default tab, pane, and window close confirmation to SmartPrompt"
        );
    }

    #[test]
    fn bundled_kaku_lua_defaults_smart_tab_to_suggestion_first() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("config.smart_tab_mode = 'suggestion_first'"),
            "bundled kaku.lua should default Smart Tab to suggestion-first"
        );
    }

    #[test]
    fn bundled_kaku_lua_enables_minimum_text_contrast() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("config.text_min_contrast_ratio = 3.0"),
            "bundled kaku.lua should guard against low-contrast terminal text"
        );
    }

    #[test]
    fn bundled_kaku_dark_maps_black_foregrounds_to_readable_text() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("ANSI_BLACK = '#c8c6cc'")
                && content.contains("[KAKU.ANSI_BLACK] = KAKU.BLACK")
                && content.contains("['#000000'] = KAKU.WHITE")
                && content.contains("['#15141b'] = KAKU.WHITE")
                && content.contains("['#1c1c1c'] = KAKU.WHITE"),
            "Kaku Dark should render Hermes black foregrounds as readable light text"
        );
    }

    #[test]
    fn bundled_kaku_light_keeps_claude_selection_visibly_blue() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/macos/Kaku.app/Contents/Resources/kaku.lua");
        let content = std::fs::read_to_string(&bundled).expect("read bundled kaku.lua");

        assert!(
            content.contains("['#205EA6'] = '#C9DDF0'"),
            "Kaku Light should not flatten Claude Code's blue selected row into the cream background"
        );
    }

    fn write_scheme_fixture(dir: &Path, file_name: &str, metadata_name: Option<&str>) {
        let metadata = metadata_name
            .map(|name| format!("\n[metadata]\nname = \"{name}\"\n"))
            .unwrap_or_default();
        std::fs::write(
            dir.join(file_name),
            format!(
                r##"
[colors]
foreground = "#e6e2d8"
background = "#101014"
ansi = ["#3a3646", "#e1505d", "#2bae85", "#e2c027", "#619ac3", "#c08eaf", "#57c3c2", "#cfc8bb"]
brights = ["#6b6478", "#ea586c", "#45b787", "#eed045", "#8fb2c9", "#d1c2d3", "#93d5dc", "#f4efe4"]
{metadata}"##
            ),
        )
        .expect("write color scheme fixture");
    }

    fn scratch_color_dir(label: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "kaku-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create color scheme fixture dir");
        dir
    }

    #[test]
    fn user_color_schemes_follow_files_and_config_without_hardcoding_names() {
        let dir = scratch_color_dir("user-schemes");

        // Named by metadata, not by the file name.
        write_scheme_fixture(&dir, "file-name.toml", Some("Zulu"));
        // Falls back to the file name when metadata.name is absent.
        write_scheme_fixture(&dir, "aurora.toml", None);
        // A broken file must not hide the others.
        std::fs::write(
            dir.join("broken.toml"),
            "[colors]\nbackground = '#000000'\n",
        )
        .expect("write broken fixture");
        // Non-TOML files and directories are ignored.
        std::fs::write(dir.join("notes.txt"), "not a scheme").expect("write text fixture");
        std::fs::create_dir(dir.join("nested.toml")).expect("create nested dir");

        let mut defined = HashMap::new();
        // Duplicate of a disk scheme: reported once.
        defined.insert("Zulu".to_string(), Palette::default());
        defined.insert("Inline Theme".to_string(), Palette::default());
        // Kaku's own entries are listed under their fixed labels instead.
        defined.insert("Kaku Dark".to_string(), Palette::default());
        defined.insert("Kaku Light".to_string(), Palette::default());
        defined.insert("Kaku Theme".to_string(), Palette::default());

        let names = super::collect_user_color_schemes(std::slice::from_ref(&dir), &defined);

        assert_eq!(names, vec!["aurora", "Inline Theme", "Zulu"]);

        std::fs::remove_dir_all(&dir).expect("clean fixture dir");
    }

    #[test]
    fn user_color_schemes_tolerate_missing_directories() {
        let names = super::collect_user_color_schemes(
            &[PathBuf::from("/nonexistent/kaku/colors")],
            &HashMap::new(),
        );
        assert!(names.is_empty());
    }
}

pub fn set_config_file_override(path: &Path) {
    CONFIG_FILE_OVERRIDE
        .lock()
        .unwrap()
        .replace(path.to_path_buf());
}

pub fn config_file_override() -> Option<PathBuf> {
    CONFIG_FILE_OVERRIDE.lock().unwrap().clone()
}

pub fn clear_config_file_override() {
    CONFIG_FILE_OVERRIDE.lock().unwrap().take();
}

pub fn set_config_overrides(items: &[(String, String)]) -> anyhow::Result<()> {
    *CONFIG_OVERRIDES.lock().unwrap() = items.to_vec();

    // Only validate overrides eagerly when override items were supplied.
    // This avoids creating an extra throwaway Lua VM on normal cold start.
    if !items.is_empty() {
        let _ = default_config_with_overrides_applied()?;
    }
    Ok(())
}

pub fn is_config_overridden() -> bool {
    CONFIG_SKIP.load(Ordering::Relaxed)
        || !CONFIG_OVERRIDES.lock().unwrap().is_empty()
        || CONFIG_FILE_OVERRIDE.lock().unwrap().is_some()
}

/// Discard the current configuration and replace it with
/// the default configuration
pub fn use_default_configuration() {
    CONFIG.use_defaults();
}

/// Use a config that doesn't depend on the user's
/// environment and is suitable for unit testing
pub fn use_test_configuration() {
    CONFIG.use_test();
}

pub fn use_this_configuration(config: Config) {
    CONFIG.use_this_config(config);
}

/// Returns a handle to the current configuration
pub fn configuration() -> ConfigHandle {
    CONFIG.get()
}

/// Returns a version of the config (loaded from the config file)
/// with some field overridden based on the supplied overrides object.
pub fn overridden_config(overrides: &wezterm_dynamic::Value) -> Result<ConfigHandle, Error> {
    CONFIG.overridden(overrides)
}

/// Load the hotkey appearance layer before explicit window overrides.
pub fn hotkey_config(overrides: &wezterm_dynamic::Value) -> Result<ConfigHandle, Error> {
    let generation = CONFIG.inner.lock().unwrap().generation;
    let loaded = Config::load_hotkey_with_overrides(overrides);
    Ok(ConfigHandle {
        config: Arc::new(loaded.config?),
        generation,
    })
}

pub fn reload() {
    CONFIG.reload();
}

/// If there was an error loading the preferred configuration,
/// return it, otherwise return the current configuration
pub fn configuration_result() -> Result<ConfigHandle, Error> {
    if let Some(error) = CONFIG.get_error() {
        bail!("{}", error);
    }
    Ok(CONFIG.get())
}

/// Returns the combined set of errors + warnings encountered
/// while loading the preferred configuration
pub fn configuration_warnings_and_errors() -> Vec<String> {
    CONFIG.get_warnings_and_errors()
}

struct ConfigInner {
    config: Arc<Config>,
    error: Option<String>,
    warnings: Vec<String>,
    generation: usize,
    watcher: Option<notify::RecommendedWatcher>,
    watched_paths: std::collections::HashSet<PathBuf>,
    // Set of file paths we care about, shared with the watcher thread so it
    // can filter parent-directory events down to just the target files.
    watched_files: Arc<std::sync::Mutex<std::collections::HashSet<PathBuf>>>,
    // Maps a watched file path to the parent directory physically registered
    // with the OS watcher (for unwatch bookkeeping).
    file_to_dir: HashMap<PathBuf, PathBuf>,
    // Reference-counts how many watched files live in each watched directory.
    // The directory is unwatched when its count reaches zero.
    dir_watch_count: HashMap<PathBuf, usize>,
    defer_watchers_until_enabled: bool,
    pending_watch_paths: Vec<PathBuf>,
    subscribers: HashMap<usize, Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl ConfigInner {
    fn new() -> Self {
        Self {
            config: Arc::new(Config::default_config()),
            error: None,
            warnings: vec![],
            generation: 0,
            watcher: None,
            watched_paths: std::collections::HashSet::new(),
            watched_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            file_to_dir: HashMap::new(),
            dir_watch_count: HashMap::new(),
            defer_watchers_until_enabled: false,
            pending_watch_paths: vec![],
            subscribers: HashMap::new(),
        }
    }

    fn subscribe<F>(&mut self, subscriber: F) -> usize
    where
        F: Fn() -> bool + 'static + Send + Sync,
    {
        static SUB_ID: AtomicUsize = AtomicUsize::new(0);
        let sub_id = SUB_ID.fetch_add(1, Ordering::Relaxed);
        self.subscribers.insert(sub_id, Arc::new(subscriber));
        sub_id
    }

    fn unsub(&mut self, sub_id: usize) {
        self.subscribers.remove(&sub_id);
    }

    /// Collect subscriber IDs and cloned callbacks for notification outside the lock.
    fn collect_subscribers_for_notify(&self) -> Vec<(usize, Arc<dyn Fn() -> bool + Send + Sync>)> {
        self.subscribers
            .iter()
            .map(|(k, v)| (*k, Arc::clone(v)))
            .collect()
    }

    fn remove_subscribers(&mut self, to_remove: &[usize]) {
        for sub_id in to_remove {
            self.subscribers.remove(sub_id);
        }
    }

    fn watch_path(&mut self, path: PathBuf) {
        if self.watcher.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            const DELAY: Duration = Duration::from_millis(100);
            // Share the watched-file set with the event thread so it can filter
            // parent-directory events to only the files we care about.
            let watched_files = Arc::clone(&self.watched_files);
            match notify::recommended_watcher(tx) {
                Ok(watcher) => {
                    std::thread::spawn(move || {
                        use notify::EventKind;

                        fn extract_path(
                            event: notify::Event,
                            watched_files: &std::sync::Mutex<std::collections::HashSet<PathBuf>>,
                        ) -> Vec<PathBuf> {
                            let paths = match event.kind {
                                EventKind::Modify(_)
                                | EventKind::Create(_)
                                | EventKind::Remove(_) => event.paths,
                                _ => return vec![],
                            };
                            // Filter to only the specific files we care about.
                            // This prevents unrelated files in the same directory
                            // from triggering spurious reloads.
                            if let Ok(set) = watched_files.lock() {
                                paths.into_iter().filter(|p| set.contains(p)).collect()
                            } else {
                                paths
                            }
                        }

                        while let Ok(event) = rx.recv() {
                            log::debug!("config watcher event: {:?}", event);
                            match event {
                                Ok(event) => {
                                    let mut paths = extract_path(event, &watched_files);
                                    if !paths.is_empty() {
                                        // Grace period to allow events to settle
                                        std::thread::sleep(DELAY);
                                        // Drain any other immediately ready events
                                        while let Ok(Ok(event)) = rx.try_recv() {
                                            paths.append(&mut extract_path(event, &watched_files));
                                        }
                                        paths.sort();
                                        paths.dedup();
                                        log::debug!("config paths {:?} changed, reloading", paths);
                                        reload();
                                    }
                                }
                                Err(err) => {
                                    log::warn!("config watcher error (forcing reload): {:#}", err);
                                    reload();
                                }
                            }
                        }
                    });
                    self.watcher.replace(watcher);
                }
                Err(err) => {
                    log::warn!(
                        "Failed to create filesystem watcher, \
                         automatic config reload will be unavailable: {:#}",
                        err
                    );
                    return;
                }
            }
        }
        // Skip paths already being watched to avoid duplicate registrations
        if self.watched_paths.contains(&path) {
            return;
        }

        if path.is_file() {
            // For regular files, watch the parent directory instead of the file
            // itself. This ensures atomic-rename saves (used by vim, VS Code,
            // JetBrains, etc.) are detected even though they replace the inode.
            // The event thread filters parent-directory events by file name.
            let dir = match path.parent() {
                Some(d) => d.to_path_buf(),
                None => path.clone(),
            };
            if let Ok(mut set) = self.watched_files.lock() {
                set.insert(path.clone());
            }
            self.file_to_dir.insert(path.clone(), dir.clone());
            let count = self.dir_watch_count.entry(dir.clone()).or_insert(0);
            *count += 1;
            let first_in_dir = *count == 1;
            if first_in_dir {
                if let Some(watcher) = self.watcher.as_mut() {
                    use notify::Watcher;
                    match watcher.watch(&dir, notify::RecursiveMode::NonRecursive) {
                        Ok(()) => {
                            log::trace!("watching config dir: {}", dir.display());
                        }
                        Err(err) => {
                            log::warn!("Failed to watch config dir {}: {:#}", dir.display(), err);
                            // Roll back registrations on failure
                            if let Ok(mut set) = self.watched_files.lock() {
                                set.remove(&path);
                            }
                            self.file_to_dir.remove(&path);
                            if let Some(c) = self.dir_watch_count.get_mut(&dir) {
                                *c -= 1;
                                if *c == 0 {
                                    self.dir_watch_count.remove(&dir);
                                }
                            }
                            return;
                        }
                    }
                }
            }
        } else {
            // For directories or non-existent paths, watch the path directly.
            if let Some(watcher) = self.watcher.as_mut() {
                use notify::Watcher;
                match watcher.watch(&path, notify::RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        log::trace!("watching config path: {}", path.display());
                    }
                    Err(err) => {
                        log::warn!("Failed to watch config path {}: {:#}", path.display(), err);
                        return;
                    }
                }
            }
        }
        self.watched_paths.insert(path);
    }

    /// Unwatch paths that are no longer in the active watch set.
    fn unwatch_stale_paths(&mut self, active_paths: &std::collections::HashSet<PathBuf>) {
        let stale: Vec<PathBuf> = self
            .watched_paths
            .iter()
            .filter(|p| !active_paths.contains(*p))
            .cloned()
            .collect();
        for path in stale {
            if let Some(dir) = self.file_to_dir.remove(&path) {
                // Was a file watch: remove from filter set, decrement dir refcount.
                if let Ok(mut set) = self.watched_files.lock() {
                    set.remove(&path);
                }
                if let Some(count) = self.dir_watch_count.get_mut(&dir) {
                    *count -= 1;
                    if *count == 0 {
                        self.dir_watch_count.remove(&dir);
                        if let Some(watcher) = self.watcher.as_mut() {
                            use notify::Watcher;
                            if let Err(err) = watcher.unwatch(&dir) {
                                log::warn!("Failed to unwatch dir {}: {:#}", dir.display(), err);
                            }
                        }
                    }
                }
            } else {
                // Was a direct path watch (directory or non-existent path).
                if let Some(watcher) = self.watcher.as_mut() {
                    use notify::Watcher;
                    if let Err(err) = watcher.unwatch(&path) {
                        log::warn!("Failed to unwatch {}: {:#}", path.display(), err);
                    }
                }
            }
            self.watched_paths.remove(&path);
        }
    }

    fn accumulate_watch_paths(lua: &Lua, watch_paths: &mut Vec<PathBuf>) {
        if let Ok(mlua::Value::Table(tbl)) = lua.named_registry_value("kaku-watch-paths") {
            for path in tbl.sequence_values::<String>() {
                if let Ok(path) = path {
                    watch_paths.push(PathBuf::from(path));
                }
            }
        }
    }

    /// Attempt to load the user's configuration.
    /// On success, clear any error and replace the current
    /// configuration.
    /// On failure, retain the existing configuration but
    /// replace any captured error message.
    /// Returns subscribers to notify (caller should invoke outside the lock).
    fn apply_loaded(
        &mut self,
        loaded: LoadedConfig,
    ) -> Vec<(usize, Arc<dyn Fn() -> bool + Send + Sync>)> {
        let LoadedConfig {
            config,
            file_name,
            lua,
            warnings,
        } = loaded;

        self.warnings = warnings;

        // Before we process the success/failure, extract and update
        // any paths that we should be watching
        let mut watch_paths = vec![];
        if let Some(path) = file_name {
            // Watch the config file itself to avoid unrelated changes in the
            // config directory (for example runtime state files) from
            // triggering reload loops.
            watch_paths.push(path.clone());
            if let Ok(real_path) = std::fs::canonicalize(&path) {
                if real_path != path {
                    watch_paths.push(real_path);
                }
            }
        }
        if let Some(lua) = &lua {
            ConfigInner::accumulate_watch_paths(lua, &mut watch_paths);
        }

        match config {
            Ok(config) => {
                self.config = Arc::new(config);
                self.error.take();
                self.generation += 1;

                // If we loaded a user config, publish this latest version of
                // the lua state to the LUA_PIPE.  This allows a subsequent
                // call to `with_lua_config` to reference this lua context
                // even though we are (probably) resolving this from a background
                // reloading thread.
                if let Some(lua) = lua {
                    LUA_PIPE.send(lua);
                }
                log::debug!("Reloaded configuration! generation={}", self.generation);
            }
            Err(err) => {
                let err = format!("{:#}", err);
                if self.generation > 0 {
                    // Only generate the message for an actual reload
                    show_error(&err);
                }
                self.error.replace(err);
            }
        }

        // Collect subscribers for notification outside the lock
        let subscribers = self.collect_subscribers_for_notify();

        self.pending_watch_paths.clear();
        if self.config.automatically_reload_config {
            if self.defer_watchers_until_enabled {
                self.pending_watch_paths = watch_paths;
            } else {
                let active: std::collections::HashSet<PathBuf> =
                    watch_paths.iter().cloned().collect();
                self.unwatch_stale_paths(&active);
                for path in watch_paths {
                    self.watch_path(path);
                }
            }
        } else {
            // Config reload disabled; drop all watchers
            let empty = std::collections::HashSet::new();
            self.unwatch_stale_paths(&empty);
        }

        subscribers
    }

    fn defer_watchers_until_enabled(&mut self) {
        self.defer_watchers_until_enabled = true;
    }

    fn enable_deferred_watchers(&mut self) {
        if !self.defer_watchers_until_enabled {
            return;
        }
        self.defer_watchers_until_enabled = false;

        if !self.config.automatically_reload_config {
            self.pending_watch_paths.clear();
            return;
        }

        let pending = std::mem::take(&mut self.pending_watch_paths);
        for path in pending {
            self.watch_path(path);
        }
    }

    /// Discard the current configuration and any recorded
    /// error message; replace them with the default
    /// configuration
    fn use_defaults(&mut self) {
        self.config = Arc::new(Config::default_config());
        self.error.take();
        self.generation += 1;
    }

    fn use_this_config(&mut self, cfg: Config) {
        self.config = Arc::new(cfg);
        self.error.take();
        self.generation += 1;
    }

    fn use_test(&mut self) {
        let mut config = Config::default_config();
        config.font_locator = FontLocatorSelection::ConfigDirsOnly;
        let exe_name = std::env::current_exe().unwrap();
        let exe_dir = exe_name.parent().unwrap();
        config.font_dirs.push(exe_dir.join("../../../assets/fonts"));
        // If we're building for a specific target, the dir
        // level is one deeper.
        #[cfg(target_os = "macos")]
        config
            .font_dirs
            .push(exe_dir.join("../../../../assets/fonts"));
        // Specify the same DPI used on non-mac systems so
        // that we have consistent values regardless of the
        // operating system that we're running tests on
        config.dpi.replace(96.0);
        self.config = Arc::new(config);
        self.error.take();
        self.generation += 1;
    }
}

pub struct Configuration {
    inner: Mutex<ConfigInner>,
    reload_epoch: AtomicUsize,
}

impl Configuration {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ConfigInner::new()),
            reload_epoch: AtomicUsize::new(0),
        }
    }

    /// Returns the effective configuration.
    pub fn get(&self) -> ConfigHandle {
        let inner = self.inner.lock().unwrap();
        ConfigHandle {
            config: Arc::clone(&inner.config),
            generation: inner.generation,
        }
    }

    /// Subscribe to config reload events
    fn subscribe<F>(&self, subscriber: F) -> usize
    where
        F: Fn() -> bool + 'static + Send + Sync,
    {
        let mut inner = self.inner.lock().unwrap();
        inner.subscribe(subscriber)
    }

    fn unsub(&self, sub_id: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.unsub(sub_id);
    }

    /// Reset the configuration to defaults
    pub fn use_defaults(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.use_defaults();
    }

    pub fn defer_watchers_until_enabled(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.defer_watchers_until_enabled();
    }

    pub fn enable_deferred_watchers(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.enable_deferred_watchers();
    }

    fn use_this_config(&self, cfg: Config) {
        let mut inner = self.inner.lock().unwrap();
        inner.use_this_config(cfg);
    }

    fn overridden(&self, overrides: &wezterm_dynamic::Value) -> Result<ConfigHandle, Error> {
        let generation = {
            let inner = self.inner.lock().unwrap();
            inner.generation
        };

        let config = Config::load_with_overrides(overrides);
        Ok(ConfigHandle {
            config: Arc::new(config.config?),
            generation,
        })
    }

    /// Use a config that doesn't depend on the user's
    /// environment and is suitable for unit testing
    pub fn use_test(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.use_test();
    }

    /// Reload the configuration
    pub fn reload(&self) {
        let reload_id = self.reload_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let loaded = Config::load();
        if self.reload_epoch.load(Ordering::Relaxed) != reload_id {
            return;
        }

        // Apply config and collect subscribers while holding the lock
        let subscribers = {
            let mut inner = self.inner.lock().unwrap();
            if self.reload_epoch.load(Ordering::Relaxed) != reload_id {
                return;
            }
            inner.apply_loaded(loaded)
        };

        // Notify subscribers outside the lock to avoid deadlock/reentrancy
        let to_remove: Vec<usize> = subscribers
            .into_iter()
            .filter_map(|(sub_id, notify)| if !notify() { Some(sub_id) } else { None })
            .collect();

        // Remove unsubscribed callbacks
        if !to_remove.is_empty() {
            let mut inner = self.inner.lock().unwrap();
            inner.remove_subscribers(&to_remove);
        }
    }

    /// Returns a copy of any captured error message.
    /// The error message is not cleared.
    pub fn get_error(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        inner.error.as_ref().cloned()
    }

    pub fn get_warnings_and_errors(&self) -> Vec<String> {
        let mut result = vec![];
        let inner = self.inner.lock().unwrap();
        if let Some(error) = &inner.error {
            result.push(error.clone());
        }
        for warning in &inner.warnings {
            result.push(warning.clone());
        }
        result
    }
}

#[derive(Clone, Debug)]
pub struct ConfigHandle {
    config: Arc<Config>,
    generation: usize,
}

impl ConfigHandle {
    /// Returns the generation number for the configuration,
    /// allowing consuming code to know whether the config
    /// has been reloading since they last derived some
    /// information from the configuration
    pub fn generation(&self) -> usize {
        self.generation
    }

    pub fn default_config() -> Self {
        Self {
            config: Arc::new(Config::default_config()),
            generation: 0,
        }
    }

    pub fn unicode_version(&self) -> UnicodeVersion {
        UnicodeVersion {
            version: self.config.unicode_version,
            ambiguous_are_wide: self.config.treat_east_asian_ambiguous_width_as_wide,
            cell_widths: CellWidth::compile_to_map(self.config.cell_widths.clone()),
        }
    }
}

impl std::ops::Deref for ConfigHandle {
    type Target = Config;
    fn deref(&self) -> &Config {
        &*self.config
    }
}

pub struct LoadedConfig {
    pub config: anyhow::Result<Config>,
    pub file_name: Option<PathBuf>,
    pub lua: Option<mlua::Lua>,
    pub warnings: Vec<String>,
}

fn default_one_point_oh_f64() -> f64 {
    1.0
}

fn default_one_point_oh() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}
