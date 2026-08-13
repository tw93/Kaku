//! Adapter seam for terminal-tool theme coordination.
//!
//! The registry owns discovery and dispatch. Concrete adapters own upstream
//! paths, precedence rules, and native file mutations.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterKind {
    Native,
    BuiltIn,
    AnsiInheritance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MutationSupport {
    NativeFiles,
    BuiltInIntegration,
    ReadOnlyInheritance,
}

impl AdapterKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Native => "adapter",
            Self::BuiltIn => "built_in",
            Self::AnsiInheritance => "ansi_inheritance",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct AdapterDescriptor {
    pub id: &'static str,
    pub kind: AdapterKind,
    pub display_name: &'static str,
    pub mutation: MutationSupport,
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterState {
    pub descriptor: AdapterDescriptor,
    pub status: &'static str,
    pub activation: &'static str,
    pub note: String,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum AdapterOperation {
    Preview,
    Apply,
    Remove,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct AdapterPlan {
    pub adapter_id: &'static str,
    pub operation: AdapterOperation,
    pub targets: Vec<PathBuf>,
    pub result: &'static str,
}

pub(crate) struct AdapterContext {
    pub effective_is_light: bool,
    pub takeover: Vec<String>,
}

fn state_root() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::HOME_DIR.join(".local/state"))
        .join("kaku/theme")
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hash: u64 = 1469598103934665603;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

fn record_state(adapter: &str, targets: &[PathBuf]) -> anyhow::Result<()> {
    let entries = targets
        .iter()
        .filter_map(|path| {
            std::fs::read(path)
                .ok()
                .map(|bytes| serde_json::json!({"path": path, "hash": hash_bytes(&bytes)}))
        })
        .collect::<Vec<_>>();
    let root = state_root();
    std::fs::create_dir_all(&root)?;
    atomic_write(
        &root.join(format!("{adapter}.json")),
        &(serde_json::to_string_pretty(
            &serde_json::json!({"schema_version":1,"adapter":adapter,"targets":entries}),
        )? + "\n"),
    )
}

fn ensure_no_drift(adapter: &str, targets: &[PathBuf]) -> anyhow::Result<()> {
    let state_path = state_root().join(format!("{adapter}.json"));
    if !state_path.is_file() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(state_path)?)?;
    let Some(entries) = value.get("targets").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    for entry in entries {
        let Some(path) = entry.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(expected) = entry.get("hash").and_then(|v| v.as_str()) else {
            continue;
        };
        let path = PathBuf::from(path);
        let actual = std::fs::read(&path).ok().map(|bytes| hash_bytes(&bytes));
        if actual.as_deref() != Some(expected) {
            anyhow::bail!(
                "drift: {adapter} target {} changed since Kaku applied it",
                path.display()
            );
        }
    }
    let _ = targets;
    Ok(())
}

impl AdapterContext {
    fn allows_takeover(&self, id: &str) -> bool {
        self.takeover.iter().any(|tool| tool == id)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterExecution {
    pub adapter_id: &'static str,
    pub result: &'static str,
    pub targets: Vec<PathBuf>,
}

pub(crate) trait ThemeAdapter: Send + Sync {
    fn descriptor(&self) -> AdapterDescriptor;
    fn detect(&self, context: &AdapterContext) -> AdapterState;

    #[allow(dead_code)]
    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        if self.descriptor().mutation == MutationSupport::ReadOnlyInheritance {
            return AdapterPlan {
                adapter_id: self.descriptor().id,
                operation,
                targets: Vec::new(),
                result: "informational_only",
            };
        }
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: Vec::new(),
            result: "informational_only",
        }
    }

    fn execute(
        &self,
        _operation: AdapterOperation,
        _context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        let descriptor = self.descriptor();
        if descriptor.mutation == MutationSupport::ReadOnlyInheritance {
            return Ok(AdapterExecution {
                adapter_id: descriptor.id,
                result: "informational_only",
                targets: Vec::new(),
            });
        }
        if descriptor.mutation == MutationSupport::BuiltInIntegration {
            return Ok(AdapterExecution {
                adapter_id: descriptor.id,
                result: "built_in_integration",
                targets: Vec::new(),
            });
        }
        anyhow::bail!("adapter `{}` is not implemented", self.descriptor().id)
    }
}

pub(crate) struct ThemeAdapterRegistry {
    adapters: Vec<Box<dyn ThemeAdapter>>,
}

impl ThemeAdapterRegistry {
    pub(crate) fn first_release() -> Self {
        Self {
            adapters: vec![
                Box::new(ClaudeCodeAdapter),
                Box::new(OpenCodeAdapter),
                Box::new(YaziBuiltInAdapter),
                Box::new(CodexAnsiAdapter),
                Box::new(AtuinAdapter),
                Box::new(FishAdapter),
                Box::new(FzfAdapter),
                Box::new(StarshipAdapter),
                Box::new(BtopAdapter),
            ],
        }
    }

    pub(crate) fn detect(&self, context: &AdapterContext) -> Vec<AdapterState> {
        self.adapters
            .iter()
            .map(|adapter| adapter.detect(context))
            .collect()
    }

    pub(crate) fn plans(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
        selected: Option<&[String]>,
    ) -> Vec<AdapterPlan> {
        self.adapters
            .iter()
            .filter(|adapter| {
                selected.is_none_or(|ids| ids.iter().any(|id| id == adapter.descriptor().id))
            })
            .map(|adapter| adapter.plan(operation, context))
            .collect()
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.adapters
            .iter()
            .any(|adapter| adapter.descriptor().id == id)
    }

    pub(crate) fn execute(
        &self,
        id: &str,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.descriptor().id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown theme tool `{id}`"))?;
        adapter.execute(operation, context)
    }
}

#[cfg(test)]
mod tests {
    use super::{AdapterContext, AdapterKind, ThemeAdapterRegistry};

    #[test]
    fn first_release_registry_has_one_entry_per_coordination_mode() {
        let states = ThemeAdapterRegistry::first_release().detect(&AdapterContext {
            effective_is_light: false,
            takeover: Vec::new(),
        });
        assert_eq!(states.len(), 9);
        assert_eq!(states[0].descriptor.id, "claude");
        assert_eq!(states[1].descriptor.id, "opencode");
        assert_eq!(states[2].descriptor.kind, AdapterKind::BuiltIn);
        assert_eq!(states[3].descriptor.kind, AdapterKind::AnsiInheritance);
        assert_eq!(states[4].descriptor.id, "atuin");
        assert_eq!(states[8].descriptor.id, "btop");
    }
}

struct ClaudeCodeAdapter;
impl ThemeAdapter for ClaudeCodeAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "claude",
            kind: AdapterKind::Native,
            display_name: "Claude Code",
            mutation: MutationSupport::NativeFiles,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        let (settings, light, dark) = claude_paths();
        let status = if light.is_file() && dark.is_file() {
            "theme_detected"
        } else {
            "available"
        };
        AdapterState {
            descriptor: self.descriptor(),
            status,
            activation: "next_launch",
            note: format!("Native Claude themes target {}", settings.display()),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        let (settings, light, dark) = claude_paths();
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: vec![settings, light, dark],
            result: "ready",
        }
    }

    fn execute(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        if matches!(operation, AdapterOperation::Remove) {
            let (settings_path, light_path, dark_path) = claude_paths();
            ensure_no_drift(
                self.descriptor().id,
                &[settings_path.clone(), light_path.clone(), dark_path.clone()],
            )?;
            if settings_path.is_file() {
                let original = std::fs::read_to_string(&settings_path)?;
                let mut settings: serde_json::Value = serde_json::from_str(&original)?;
                if settings
                    .get("theme")
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| v.starts_with("custom:kaku-"))
                {
                    settings
                        .as_object_mut()
                        .map(|object| object.remove("theme"));
                    atomic_write(
                        &settings_path,
                        &(serde_json::to_string_pretty(&settings)? + "\n"),
                    )?;
                }
            }
            for path in [&light_path, &dark_path] {
                if path.is_file()
                    && std::fs::read_to_string(path)?.contains("Kaku-managed theme adapter")
                {
                    std::fs::remove_file(path)?;
                }
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: "removed",
                targets: vec![settings_path, light_path, dark_path],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("Claude Code currently supports apply and remove only")
        }
        let (settings_path, light_path, dark_path) = claude_paths();
        let original = settings_path
            .is_file()
            .then(|| std::fs::read_to_string(&settings_path))
            .transpose()?
            .unwrap_or_else(|| "{}".into());
        let mut settings: serde_json::Value = serde_json::from_str(&original)?;
        let existing = settings.get("theme").and_then(serde_json::Value::as_str);
        if existing.is_some_and(|value| !value.starts_with("custom:kaku-"))
            && !context.allows_takeover("claude")
        {
            anyhow::bail!(
                "consent_required: existing Claude Code theme requires --take-over claude"
            )
        }
        let palette = if context.effective_is_light {
            "light"
        } else {
            "dark"
        };
        let slug = format!("custom:kaku-{palette}");
        for target in [&light_path, &dark_path] {
            if target.is_file()
                && !std::fs::read_to_string(target)?.contains("Kaku-managed theme adapter")
                && !context.allows_takeover("claude")
            {
                anyhow::bail!(
                    "consent_required: existing Claude theme file requires --take-over claude"
                )
            }
        }
        settings["theme"] = serde_json::Value::String(slug);
        if let Some(parent) = light_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(
            &light_path,
            &(serde_json::to_string_pretty(&claude_theme_json(true))? + "\n"),
        )?;
        atomic_write(
            &dark_path,
            &(serde_json::to_string_pretty(&claude_theme_json(false))? + "\n"),
        )?;
        atomic_write(
            &settings_path,
            &(serde_json::to_string_pretty(&settings)? + "\n"),
        )?;
        record_state(
            self.descriptor().id,
            &[settings_path.clone(), light_path.clone(), dark_path.clone()],
        )?;
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![settings_path, light_path, dark_path],
        })
    }
}

fn claude_paths() -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::HOME_DIR.join(".claude"));
    (
        root.join("settings.json"),
        root.join("themes/kaku-light.json"),
        root.join("themes/kaku-dark.json"),
    )
}

fn claude_theme_json(is_light: bool) -> serde_json::Value {
    let (primary, text, inactive, suggestion, error, background) = if is_light {
        (
            "#5E3DB3", "#403E3C", "#7A7872", "#E9E3D5", "#AF3029", "#FFFCF0",
        )
    } else {
        (
            "#8E6AD9", "#D5D4D6", "#6D6D6D", "#2A2735", "#D85D5D", "#15141B",
        )
    };
    serde_json::json!({
        "name": "Kaku-managed theme adapter",
        "base": if is_light { "light" } else { "dark" },
        "overrides": {
            "claude": primary, "text": text, "inactive": inactive,
            "suggestion": suggestion, "permission": primary, "error": error,
            "diffAdded": "#24837B", "diffRemoved": error,
            "background": background
        }
    })
}

struct YaziBuiltInAdapter;
impl ThemeAdapter for YaziBuiltInAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "yazi",
            kind: AdapterKind::BuiltIn,
            display_name: "Yazi",
            mutation: MutationSupport::BuiltInIntegration,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        AdapterState {
            descriptor: self.descriptor(),
            status: "coordinated",
            activation: "shell_integration",
            note: "Existing Kaku-managed flavor integration".into(),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: Vec::new(),
            result: "built_in_integration",
        }
    }
}

struct OpenCodeAdapter;
impl ThemeAdapter for OpenCodeAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "opencode",
            kind: AdapterKind::Native,
            display_name: "OpenCode",
            mutation: MutationSupport::NativeFiles,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        let (config, theme) = opencode_paths();
        let status = if theme.is_file() {
            "theme_detected"
        } else {
            "available"
        };
        AdapterState {
            descriptor: self.descriptor(),
            status,
            activation: "next_launch",
            note: format!("OpenCode TUI theme target {}", config.display()),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        let (config, theme) = opencode_paths();
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: vec![config, theme],
            result: "ready",
        }
    }

    fn execute(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        if matches!(operation, AdapterOperation::Remove) {
            let (config_path, theme_path) = opencode_paths();
            ensure_no_drift(
                self.descriptor().id,
                &[config_path.clone(), theme_path.clone()],
            )?;
            if config_path.is_file() {
                let original = std::fs::read_to_string(&config_path)?;
                let mut config: serde_json::Value = serde_json::from_str(&original)?;
                if config.get("theme").and_then(|v| v.as_str()) == Some("kaku") {
                    config.as_object_mut().map(|object| object.remove("theme"));
                    atomic_write(
                        &config_path,
                        &(serde_json::to_string_pretty(&config)? + "\n"),
                    )?;
                }
            }
            if theme_path.is_file()
                && std::fs::read_to_string(&theme_path)?.contains("\"name\": \"Kaku\"")
            {
                std::fs::remove_file(&theme_path)?;
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: "removed",
                targets: vec![config_path, theme_path],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("OpenCode currently supports apply and remove only")
        }
        let (config_path, theme_path) = opencode_paths();
        let original = config_path
            .is_file()
            .then(|| std::fs::read_to_string(&config_path))
            .transpose()?
            .unwrap_or_else(|| "{}".into());
        let mut config: serde_json::Value = serde_json::from_str(&original)
            .map_err(|error| anyhow::anyhow!("OpenCode TUI config is not valid JSON: {error}"))?;
        let existing = config.get("theme").and_then(serde_json::Value::as_str);
        if existing.is_some_and(|value| value != "kaku") && !context.allows_takeover("opencode") {
            anyhow::bail!("consent_required: existing OpenCode theme requires --take-over opencode")
        }
        if theme_path.is_file()
            && !std::fs::read_to_string(&theme_path)?.contains("Kaku-managed theme adapter")
            && !context.allows_takeover("opencode")
        {
            anyhow::bail!(
                "consent_required: existing OpenCode theme file requires --take-over opencode"
            )
        }
        config["theme"] = serde_json::Value::String("kaku".into());
        if let Some(parent) = theme_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(
            &theme_path,
            &(serde_json::to_string_pretty(&opencode_theme())? + "\n"),
        )?;
        atomic_write(
            &config_path,
            &(serde_json::to_string_pretty(&config)? + "\n"),
        )?;
        record_state(
            self.descriptor().id,
            &[config_path.clone(), theme_path.clone()],
        )?;
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![config_path, theme_path],
        })
    }
}

fn opencode_paths() -> (PathBuf, PathBuf) {
    let config_path = std::env::var_os("OPENCODE_TUI_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| config::HOME_DIR.join(".config"));
            root.join("opencode/tui.json")
        });
    let theme_root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::HOME_DIR.join(".config"))
        .join("opencode/themes");
    (config_path, theme_root.join("kaku.json"))
}

fn opencode_theme() -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://opencode.ai/theme.json",
        "name": "Kaku",
        "defs": {
            "dark_bg": "#15141B", "dark_panel": "#201E29", "dark_fg": "#D5D4D6",
            "dark_muted": "#A9A6B0", "dark_primary": "#8E6AD9", "dark_red": "#D85D5D",
            "dark_green": "#58D8AD", "dark_yellow": "#DAAE76", "dark_blue": "#7EA6F8",
            "light_bg": "#FFFCF0", "light_panel": "#F5F1E6", "light_fg": "#403E3C",
            "light_muted": "#68645E", "light_primary": "#5E3DB3", "light_red": "#AF3029",
            "light_green": "#24837B", "light_yellow": "#9A7400", "light_blue": "#275FE4"
        },
        "theme": {
            "primary": {"dark": "dark_primary", "light": "light_primary"},
            "secondary": {"dark": "dark_green", "light": "light_green"},
            "accent": {"dark": "dark_yellow", "light": "light_yellow"},
            "error": {"dark": "dark_red", "light": "light_red"},
            "warning": {"dark": "dark_yellow", "light": "light_yellow"},
            "success": {"dark": "dark_green", "light": "light_green"},
            "info": {"dark": "dark_blue", "light": "light_blue"},
            "text": {"dark": "dark_fg", "light": "light_fg"},
            "textMuted": {"dark": "dark_muted", "light": "light_muted"},
            "background": {"dark": "dark_bg", "light": "light_bg"},
            "backgroundPanel": {"dark": "dark_panel", "light": "light_panel"},
            "backgroundElement": {"dark": "dark_panel", "light": "light_panel"},
            "border": {"dark": "#45414F", "light": "#C9C1B3"},
            "borderActive": {"dark": "dark_primary", "light": "light_primary"},
            "borderSubtle": {"dark": "#45414F", "light": "#C9C1B3"},
            "diffAdded": {"dark": "dark_green", "light": "light_green"},
            "diffRemoved": {"dark": "dark_red", "light": "light_red"},
            "diffAddedBg": {"dark": "#24352F", "light": "#DDEDE7"},
            "diffRemovedBg": {"dark": "#3A282B", "light": "#F4DFDC"},
            "markdownText": {"dark": "dark_fg", "light": "light_fg"},
            "markdownHeading": {"dark": "dark_primary", "light": "light_primary"},
            "markdownLink": {"dark": "dark_blue", "light": "light_blue"},
            "markdownCode": {"dark": "dark_green", "light": "light_green"},
            "syntaxComment": {"dark": "dark_muted", "light": "light_muted"},
            "syntaxKeyword": {"dark": "dark_primary", "light": "light_primary"},
            "syntaxFunction": {"dark": "dark_blue", "light": "light_blue"},
            "syntaxString": {"dark": "dark_green", "light": "light_green"},
            "syntaxNumber": {"dark": "dark_yellow", "light": "light_yellow"}
        }
    })
}

struct CodexAnsiAdapter;
impl ThemeAdapter for CodexAnsiAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "codex",
            kind: AdapterKind::AnsiInheritance,
            display_name: "Codex",
            mutation: MutationSupport::ReadOnlyInheritance,
        }
    }

    fn detect(&self, context: &AdapterContext) -> AdapterState {
        let mode = if context.effective_is_light {
            "light"
        } else {
            "dark"
        };
        AdapterState {
            descriptor: self.descriptor(),
            status: "inherited",
            activation: "terminal_palette",
            note: format!("Follows Kaku {mode} ANSI colors; Codex config untouched"),
        }
    }
}

struct AtuinAdapter;
impl ThemeAdapter for AtuinAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "atuin",
            kind: AdapterKind::AnsiInheritance,
            display_name: "Atuin",
            mutation: MutationSupport::ReadOnlyInheritance,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        AdapterState {
            descriptor: self.descriptor(),
            status: "informational",
            activation: "terminal_palette",
            note: "Atuin is registered for status only; Kaku does not write Atuin configuration"
                .into(),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: Vec::new(),
            result: "informational_only",
        }
    }

    fn execute(
        &self,
        _operation: AdapterOperation,
        _context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "informational_only",
            targets: Vec::new(),
        })
    }
}

/* Legacy Atuin implementation intentionally retained in history, not compiled.
        if matches!(operation, AdapterOperation::Remove) {
            let (config, theme) = atuin_paths();
            if config.is_file() {
                let original = std::fs::read_to_string(&config)?;
                let mut doc = original.parse::<toml_edit::DocumentMut>()?;
                if doc
                    .get("theme")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    == Some("kaku")
                {
                    if let Some(theme) = doc.get_mut("theme").and_then(|v| v.as_table_mut()) {
                        theme.remove("name");
                    }
                    atomic_write(&config, &doc.to_string())?;
                }
            }
            if theme.is_file()
                && std::fs::read_to_string(&theme)?.contains("Kaku-managed theme adapter: atuin")
            {
                std::fs::remove_file(&theme)?;
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: "removed",
                targets: vec![config, theme],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("Atuin currently supports apply and remove only")
        }
        let (config_path, theme_path) = atuin_paths();
        if theme_path.is_file() {
            let existing_theme = std::fs::read_to_string(&theme_path)?;
            if !existing_theme.contains("Kaku-managed theme adapter: atuin")
                && !context.allows_takeover("atuin")
            {
                anyhow::bail!("consent_required: existing Atuin theme requires --take-over atuin");
            }
        }
        let original = if config_path.exists() {
            std::fs::read_to_string(&config_path)?
        } else {
            String::new()
        };
        let mut document = if original.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            original.parse::<toml_edit::DocumentMut>()?
        };
        let existing_selector = document
            .get("theme")
            .and_then(|theme| theme.get("name"))
            .and_then(|name| name.as_str())
            .map(str::to_owned);
        if existing_selector
            .as_deref()
            .is_some_and(|name| name != "kaku")
            && !context.allows_takeover("atuin")
        {
            anyhow::bail!(
                "consent_required: existing Atuin theme selector requires --take-over atuin"
            )
        }

        let base = if context.effective_is_light {
            "#343331"
        } else {
            "#D5D4D6"
        };
        let theme_content = format!(
            "# Kaku-managed theme adapter: atuin\n[theme]\nname = \"kaku\"\n\n[colors]\nBase = \"{base}\"\n"
        );
        let config_content = {
            document["theme"]["name"] = toml_edit::value("kaku");
            document.to_string()
        };

        // Prepare both files before committing either one. If the second rename
        // fails, restore the first file from its in-memory baseline.
        let old_theme = theme_path
            .is_file()
            .then(|| std::fs::read(&theme_path))
            .transpose()?;
        let old_config = config_path
            .is_file()
            .then(|| std::fs::read(&config_path))
            .transpose()?;
        if let Some(parent) = theme_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&theme_path, &theme_content)?;
        if let Err(error) = atomic_write(&config_path, &config_content) {
            if let Some(previous) = old_theme {
                let _ = std::fs::write(&theme_path, previous);
            } else {
                let _ = std::fs::remove_file(&theme_path);
            }
            return Err(error.context("rolling back Atuin theme after config write failure"));
        }

        // Keep these bindings explicit: they document that a future remove
        // operation must only restore files that this transaction owned.
        let _ = (old_config, existing_selector);
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![config_path, theme_path],
        })
*/

fn atomic_write(path: &PathBuf, content: &str) -> anyhow::Result<()> {
    let tmp = path.with_extension("kaku-tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

struct FishAdapter;
impl ThemeAdapter for FishAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "fish",
            kind: AdapterKind::Native,
            display_name: "Fish",
            mutation: MutationSupport::NativeFiles,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        let (theme, snippet) = fish_paths();
        let status = if theme.is_file() && snippet.is_file() {
            "theme_detected"
        } else {
            "available"
        };
        AdapterState {
            descriptor: self.descriptor(),
            status,
            activation: "shell_startup_and_appearance",
            note: format!("Fish dual theme target {}", theme.display()),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        let (theme, snippet) = fish_paths();
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: vec![theme, snippet],
            result: "ready",
        }
    }

    fn execute(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        let (theme_path, snippet_path) = fish_paths();
        if matches!(operation, AdapterOperation::Remove) {
            let mut removed = false;
            ensure_no_drift(
                self.descriptor().id,
                &[theme_path.clone(), snippet_path.clone()],
            )?;
            if snippet_path.is_file()
                && std::fs::read_to_string(&snippet_path)?.contains("Kaku-managed theme adapter")
            {
                std::fs::remove_file(&snippet_path)?;
                removed = true;
            }
            if theme_path.is_file()
                && std::fs::read_to_string(&theme_path)?.contains("Kaku-managed theme adapter")
            {
                std::fs::remove_file(&theme_path)?;
                removed = true;
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: if removed { "removed" } else { "not_installed" },
                targets: vec![theme_path, snippet_path],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("Fish currently supports apply and remove only")
        }
        for path in [&theme_path, &snippet_path] {
            if path.is_file()
                && !std::fs::read_to_string(path)?.contains("Kaku-managed theme adapter")
                && !context.allows_takeover("fish")
            {
                anyhow::bail!(
                    "consent_required: existing Fish theme asset requires --take-over fish"
                )
            }
        }
        if let Some(parent) = theme_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = snippet_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&theme_path, &fish_theme())?;
        atomic_write(&snippet_path, &fish_snippet())?;
        record_state(
            self.descriptor().id,
            &[theme_path.clone(), snippet_path.clone()],
        )?;
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![theme_path, snippet_path],
        })
    }
}

fn fish_paths() -> (PathBuf, PathBuf) {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::HOME_DIR.join(".config"))
        .join("fish");
    (
        root.join("themes/Kaku.theme"),
        root.join("conf.d/kaku-theme.fish"),
    )
}

fn fish_theme() -> String {
    "# Kaku-managed theme adapter: fish\n[dark]\nset fish_color_command #58D8AD\nset fish_color_param #D5D4D6\nset fish_color_quote #65D1E5\nset fish_color_redirection #7EA6F8\nset fish_color_end #DAAE76\nset fish_color_error #D85D5D\nset fish_color_comment #6D6D6D\nset fish_color_operator #8E6AD9\nset fish_color_escape #C792EA\nset fish_color_autosuggestion #A9A6B0\nset fish_pager_color_prefix #8E6AD9\nset fish_pager_color_completion #D5D4D6\nset fish_pager_color_description #A9A6B0\nset fish_pager_color_selected_background --background=#2A2735\n[light]\nset fish_color_command #24837B\nset fish_color_param #403E3C\nset fish_color_quote #0E6A83\nset fish_color_redirection #275FE4\nset fish_color_end #9A7400\nset fish_color_error #AF3029\nset fish_color_comment #68645E\nset fish_color_operator #5E3DB3\nset fish_color_escape #7A3E9D\nset fish_color_autosuggestion #68645E\nset fish_pager_color_prefix #5E3DB3\nset fish_pager_color_completion #403E3C\nset fish_pager_color_description #68645E\nset fish_pager_color_selected_background --background=#E9E3D5\n[unknown]\nset fish_color_command #8E6AD9\nset fish_color_error #D85D5D\n".into()
}

fn fish_snippet() -> String {
    "# Kaku-managed theme adapter: fish\nif status is-interactive\n    fish_config theme choose Kaku >/dev/null 2>/dev/null\nend\n".into()
}

struct FzfAdapter;
impl ThemeAdapter for FzfAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "fzf",
            kind: AdapterKind::Native,
            display_name: "fzf",
            mutation: MutationSupport::NativeFiles,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        let (light, dark, helper) = fzf_paths();
        AdapterState {
            descriptor: self.descriptor(),
            status: if light.is_file() && dark.is_file() && helper.is_file() {
                "theme_detected"
            } else {
                "available"
            },
            activation: "next_invocation",
            note: format!("fzf option files target {}", light.display()),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        let (light, dark, helper) = fzf_paths();
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: vec![light, dark, helper],
            result: "ready",
        }
    }

    fn execute(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        let (light, dark, helper) = fzf_paths();
        if matches!(operation, AdapterOperation::Remove) {
            ensure_no_drift(
                self.descriptor().id,
                &[light.clone(), dark.clone(), helper.clone()],
            )?;
            for path in [&light, &dark, &helper] {
                if path.is_file()
                    && std::fs::read_to_string(path)?.contains("Kaku-managed theme adapter")
                {
                    std::fs::remove_file(path)?;
                }
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: "removed",
                targets: vec![light, dark, helper],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("fzf currently supports apply and remove only")
        }
        for path in [&light, &dark, &helper] {
            if path.is_file()
                && !std::fs::read_to_string(path)?.contains("Kaku-managed theme adapter")
                && !context.allows_takeover("fzf")
            {
                anyhow::bail!("consent_required: existing fzf theme asset requires --take-over fzf")
            }
        }
        if let Some(parent) = light.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&light, &fzf_options(true))?;
        atomic_write(&dark, &fzf_options(false))?;
        if let Some(parent) = helper.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&helper, &fzf_env())?;
        record_state(
            self.descriptor().id,
            &[light.clone(), dark.clone(), helper.clone()],
        )?;
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![light, dark, helper],
        })
    }
}

fn fzf_paths() -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::HOME_DIR.join(".config"))
        .join("fzf");
    (
        root.join("kaku-light.opts"),
        root.join("kaku-dark.opts"),
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| config::HOME_DIR.join(".config"))
            .join("kaku/theme/fzf.sh"),
    )
}
fn fzf_options(light: bool) -> String {
    let (bg, fg, prompt, pointer, marker, hl, selected) = if light {
        (
            "#FFFCF0", "#403E3C", "#5E3DB3", "#5E3DB3", "#24837B", "#275FE4", "#E9E3D5",
        )
    } else {
        (
            "#15141B", "#D5D4D6", "#8E6AD9", "#8E6AD9", "#58D8AD", "#7EA6F8", "#2A2735",
        )
    };
    format!("# Kaku-managed theme adapter: fzf\n--color=fg:{fg},bg:{bg},prompt:{prompt},pointer:{pointer},marker:{marker},hl:{hl},fg+:{fg},bg+:{selected},border:{prompt},header:{hl}\n")
}
fn fzf_env() -> String {
    "# Kaku-managed theme adapter: fzf\n_fzf_kaku_config=${XDG_CONFIG_HOME:-$HOME/.config}/fzf\nif [ \"${COLORFGBG##*;}\" = \"15\" ]; then\n  export FZF_DEFAULT_OPTS_FILE=\"$_fzf_kaku_config/kaku-light.opts\"\nelse\n  export FZF_DEFAULT_OPTS_FILE=\"$_fzf_kaku_config/kaku-dark.opts\"\nfi\nunset _fzf_kaku_config\n".into()
}

const FZF_ZSH_HOOK: &str = "# Kaku fzf theme adapter\n[[ -f \"${XDG_CONFIG_HOME:-$HOME/.config}/kaku/theme/fzf.sh\" ]] && source \"${XDG_CONFIG_HOME:-$HOME/.config}/kaku/theme/fzf.sh\"\n";
const FZF_FISH_HOOK: &str = "# Kaku fzf theme adapter\nset -l _kaku_fzf_config $HOME/.config\nif set -q XDG_CONFIG_HOME\n    set _kaku_fzf_config $XDG_CONFIG_HOME\nend\nset -l _kaku_fzf_theme $_kaku_fzf_config/kaku/theme/fzf.sh\nif test -f $_kaku_fzf_theme\n    source $_kaku_fzf_theme\nend\nunset _kaku_fzf_theme _kaku_fzf_config\n";

#[allow(dead_code)]
fn install_fzf_shell_hooks() -> anyhow::Result<()> {
    let homes = [
        (
            config::HOME_DIR.join(".config/kaku/zsh/kaku.zsh"),
            FZF_ZSH_HOOK,
        ),
        (
            config::HOME_DIR.join(".config/kaku/fish/kaku.fish"),
            FZF_FISH_HOOK,
        ),
    ];
    for (path, hook) in homes {
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        if !content.contains("# Kaku fzf theme adapter") {
            atomic_write(&path, &(content + "\n" + hook))?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn remove_fzf_shell_hooks() -> anyhow::Result<()> {
    for (path, hook) in [
        (
            config::HOME_DIR.join(".config/kaku/zsh/kaku.zsh"),
            FZF_ZSH_HOOK,
        ),
        (
            config::HOME_DIR.join(".config/kaku/fish/kaku.fish"),
            FZF_FISH_HOOK,
        ),
    ] {
        if path.is_file() {
            let content = std::fs::read_to_string(&path)?;
            if content.contains(hook) {
                atomic_write(&path, &content.replace(hook, ""))?;
            }
        }
    }
    Ok(())
}

struct StarshipAdapter;
impl ThemeAdapter for StarshipAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "starship",
            kind: AdapterKind::Native,
            display_name: "Starship",
            mutation: MutationSupport::NativeFiles,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        let path = starship_path();
        AdapterState {
            descriptor: self.descriptor(),
            status: "available",
            activation: "next_prompt",
            note: format!("Starship config target {}", path.display()),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: vec![starship_path()],
            result: "ready",
        }
    }

    fn execute(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        let path = starship_path();
        if matches!(operation, AdapterOperation::Remove) {
            ensure_no_drift(self.descriptor().id, std::slice::from_ref(&path))?;
            if path.is_file() {
                let original = std::fs::read_to_string(&path)?;
                let mut doc = original.parse::<toml_edit::DocumentMut>()?;
                let is_kaku = doc
                    .get("palette")
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| v.starts_with("kaku_"));
                if is_kaku {
                    doc.remove("palette");
                }
                if let Some(palettes) = doc.get_mut("palettes").and_then(|v| v.as_table_mut()) {
                    palettes.remove("kaku_dark");
                    palettes.remove("kaku_light");
                }
                atomic_write(&path, &doc.to_string())?;
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: "removed",
                targets: vec![path],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("Starship currently supports apply only")
        }
        let original = path
            .is_file()
            .then(|| std::fs::read_to_string(&path))
            .transpose()?
            .unwrap_or_default();
        let mut doc = if original.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            original.parse::<toml_edit::DocumentMut>()?
        };
        if doc.get("palette").is_some() && !context.allows_takeover("starship") {
            anyhow::bail!(
                "consent_required: existing Starship palette requires --take-over starship"
            )
        }
        for (name, values) in [
            (
                "kaku_dark",
                [
                    ("blue", "#7EA6F8"),
                    ("green", "#58D8AD"),
                    ("red", "#D85D5D"),
                    ("purple", "#8E6AD9"),
                ],
            ),
            (
                "kaku_light",
                [
                    ("blue", "#275FE4"),
                    ("green", "#24837B"),
                    ("red", "#AF3029"),
                    ("purple", "#5E3DB3"),
                ],
            ),
        ] {
            for (key, value) in values {
                doc["palettes"][name][key] = toml_edit::value(value);
            }
        }
        doc["palette"] = toml_edit::value(if context.effective_is_light {
            "kaku_light"
        } else {
            "kaku_dark"
        });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&path, &doc.to_string())?;
        record_state(self.descriptor().id, std::slice::from_ref(&path))?;
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![path],
        })
    }
}

fn starship_path() -> PathBuf {
    std::env::var_os("STARSHIP_CONFIG")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(|root| PathBuf::from(root).join("starship.toml"))
                .map(Into::into)
        })
        .unwrap_or_else(|| config::HOME_DIR.join(".config/starship.toml"))
}

struct BtopAdapter;
impl ThemeAdapter for BtopAdapter {
    fn descriptor(&self) -> AdapterDescriptor {
        AdapterDescriptor {
            id: "btop",
            kind: AdapterKind::Native,
            display_name: "btop",
            mutation: MutationSupport::NativeFiles,
        }
    }

    fn detect(&self, _context: &AdapterContext) -> AdapterState {
        let (_config, theme) = btop_paths();
        AdapterState {
            descriptor: self.descriptor(),
            status: if theme.is_file() {
                "theme_detected"
            } else {
                "available"
            },
            activation: "next_launch",
            note: format!("btop theme target {}", theme.display()),
        }
    }

    fn plan(&self, operation: AdapterOperation, _context: &AdapterContext) -> AdapterPlan {
        let (config, theme) = btop_paths();
        AdapterPlan {
            adapter_id: self.descriptor().id,
            operation,
            targets: vec![config, theme],
            result: "ready",
        }
    }

    fn execute(
        &self,
        operation: AdapterOperation,
        context: &AdapterContext,
    ) -> anyhow::Result<AdapterExecution> {
        let (config_path, theme_path) = btop_paths();
        if matches!(operation, AdapterOperation::Remove) {
            ensure_no_drift(
                self.descriptor().id,
                &[config_path.clone(), theme_path.clone()],
            )?;
            if config_path.is_file() {
                let original = std::fs::read_to_string(&config_path)?;
                let filtered = original
                    .lines()
                    .filter(|line| {
                        !line.trim_start().starts_with("color_theme") || !line.contains("Kaku")
                    })
                    .map(|line| format!("{line}\n"))
                    .collect::<String>();
                atomic_write(&config_path, &filtered)?;
            }
            if theme_path.is_file()
                && std::fs::read_to_string(&theme_path)?
                    .contains("Kaku-managed theme adapter: btop")
            {
                std::fs::remove_file(&theme_path)?;
            }
            return Ok(AdapterExecution {
                adapter_id: self.descriptor().id,
                result: "removed",
                targets: vec![config_path, theme_path],
            });
        }
        if !matches!(operation, AdapterOperation::Apply) {
            anyhow::bail!("btop currently supports apply only")
        }
        let original = config_path
            .is_file()
            .then(|| std::fs::read_to_string(&config_path))
            .transpose()?
            .unwrap_or_default();
        if original
            .lines()
            .any(|line| line.trim_start().starts_with("color_theme") && !line.contains("Kaku"))
            && !context.allows_takeover("btop")
        {
            anyhow::bail!("consent_required: existing btop color_theme requires --take-over btop")
        }
        let mut config = String::new();
        let mut found = false;
        for line in original.lines() {
            if line.trim_start().starts_with("color_theme") {
                config.push_str("color_theme = \"Kaku\"\n");
                found = true;
            } else {
                config.push_str(line);
                config.push('\n');
            }
        }
        if !found {
            config.push_str("color_theme = \"Kaku\"\n");
        }
        if theme_path.is_file()
            && !std::fs::read_to_string(&theme_path)?.contains("Kaku-managed theme adapter")
            && !context.allows_takeover("btop")
        {
            anyhow::bail!("consent_required: existing btop theme requires --take-over btop")
        }
        if let Some(parent) = theme_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&theme_path, &btop_theme(context.effective_is_light))?;
        atomic_write(&config_path, &config)?;
        record_state(
            self.descriptor().id,
            &[config_path.clone(), theme_path.clone()],
        )?;
        Ok(AdapterExecution {
            adapter_id: self.descriptor().id,
            result: "applied",
            targets: vec![config_path, theme_path],
        })
    }
}

fn btop_paths() -> (PathBuf, PathBuf) {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| config::HOME_DIR.join(".config"))
        .join("btop");
    (root.join("btop.conf"), root.join("themes/Kaku.theme"))
}
fn btop_theme(light: bool) -> String {
    let (bg, fg, hi, selected, muted, green, border, yellow, red, blue, panel) = if light {
        (
            "#FFFCF0", "#403E3C", "#5E3DB3", "#E9E3D5", "#68645E", "#24837B", "#C9C1B3", "#9A7400",
            "#AF3029", "#275FE4", "#F5F1E6",
        )
    } else {
        (
            "#15141B", "#D5D4D6", "#8E6AD9", "#2A2735", "#A9A6B0", "#58D8AD", "#45414F", "#DAAE76",
            "#D85D5D", "#7EA6F8", "#201E29",
        )
    };
    format!("# Kaku-managed theme adapter: btop\ntheme[main_bg] = \"{bg}\"\ntheme[main_fg] = \"{fg}\"\ntheme[hi_fg] = \"{hi}\"\ntheme[selected_bg] = \"{selected}\"\ntheme[selected_fg] = \"{fg}\"\ntheme[inactive_fg] = \"{muted}\"\ntheme[proc_misc] = \"{green}\"\ntheme[proc_box] = \"{border}\"\ntheme[div_line] = \"{border}\"\ntheme[temp_start] = \"{green}\"\ntheme[temp_mid] = \"{yellow}\"\ntheme[temp_end] = \"{red}\"\ntheme[cpu_start] = \"{green}\"\ntheme[cpu_mid] = \"{yellow}\"\ntheme[cpu_end] = \"{red}\"\ntheme[free_start] = \"{green}\"\ntheme[free_mid] = \"{yellow}\"\ntheme[free_end] = \"{red}\"\ntheme[cached_start] = \"{blue}\"\ntheme[cached_mid] = \"{hi}\"\ntheme[cached_end] = \"{red}\"\ntheme[download_start] = \"{green}\"\ntheme[download_mid] = \"{blue}\"\ntheme[download_end] = \"{hi}\"\ntheme[upload_start] = \"{yellow}\"\ntheme[upload_mid] = \"{red}\"\ntheme[upload_end] = \"{hi}\"\ntheme[graph_text] = \"{fg}\"\ntheme[meter_bg] = \"{panel}\"\ntheme[joy] = \"{green}\"\n")
}
