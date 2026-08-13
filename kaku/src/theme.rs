//! Theme coordination commands and adapter dispatch.

use crate::theme_adapters::{AdapterContext, AdapterOperation, ThemeAdapterRegistry};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Parser, Clone)]
pub struct ThemeCommand {
    #[command(subcommand)]
    pub command: ThemeSubcommand,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, global = true)]
    pub format: OutputFormat,
}

#[derive(Debug, Subcommand, Clone)]
pub enum ThemeSubcommand {
    Current,
    Palette,
    Status,
    Preview {
        #[arg(long, value_delimiter = ',', required = true)]
        tools: Vec<String>,
    },
    Apply {
        #[arg(long, value_delimiter = ',', required = true)]
        tools: Vec<String>,
        #[arg(long = "take-over", value_delimiter = ',')]
        take_over: Vec<String>,
    },
    Remove {
        #[arg(long, value_delimiter = ',', required = true)]
        tools: Vec<String>,
    },
    Setup,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug)]
struct ThemeRunError {
    code: i32,
    message: String,
}

impl fmt::Display for ThemeRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ThemeRunError {}

pub(crate) fn exit_code(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<ThemeRunError>()
        .map(|error| error.code)
}

#[derive(Debug, Serialize)]
struct ThemeState {
    selection: String,
    effective_mode: String,
    scheme: String,
    appearance_source: String,
}

#[derive(Debug, Serialize)]
struct ToolStatus {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    mutation: &'static str,
    status: &'static str,
    activation: &'static str,
    note: String,
}

pub fn run(command: &ThemeCommand) -> anyhow::Result<()> {
    let config = crate::kaku_theme::config_for_theme_commands();
    let palette = crate::kaku_theme::current_theme_palette();
    let state = state_from_config(&config, palette.is_light);
    match &command.command {
        ThemeSubcommand::Current => output(
            &state,
            command.format,
            format!(
                "selection={} effective_mode={} scheme={} source={}",
                state.selection, state.effective_mode, state.scheme, state.appearance_source
            ),
        ),
        ThemeSubcommand::Palette => {
            let semantic = serde_json::json!({
                "background": hex(palette.bg),
                "surface": if palette.is_light { "#F5F1E6" } else { "#201E29" },
                "surface_active": if palette.is_light { "#E9E3D5" } else { "#2A2735" },
                "border": if palette.is_light { "#C9C1B3" } else { "#45414F" },
                "foreground": hex(palette.text),
                "foreground_soft": if palette.is_light { "#5D5954" } else { "#B5B2BA" },
                "muted": hex(palette.muted),
                "syntax_muted": if palette.is_light { "#68645E" } else { "#A9A6B0" },
                "primary": hex(palette.primary),
                "red": hex(palette.error),
                "green": hex(palette.secondary),
                "yellow": hex(palette.accent),
                "blue": if palette.is_light { "#275FE4" } else { "#7EA6F8" },
                "bright_blue": if palette.is_light { "#305FCE" } else { "#9DBBFF" },
                "magenta": if palette.is_light { "#7A3E9D" } else { "#C792EA" },
                "cyan": if palette.is_light { "#0E6A83" } else { "#65D1E5" },
            });
            let value = serde_json::json!({
                "schema_version": 1,
                "palette_revision": 1,
                "theme": state,
                "semantic": semantic,
                "terminal": {
                    "foreground": hex(palette.text), "background": hex(palette.bg),
                    "cursor": hex(palette.primary), "selection": hex(palette.primary),
                    "selection_bg_alpha": if palette.is_light { 1.0 } else { 0.55 },
                    "ansi": {
                        "black": "#000000", "red": hex(palette.error), "green": hex(palette.secondary), "yellow": hex(palette.accent),
                        "blue": if palette.is_light { "#275FE4" } else { "#7EA6F8" }, "magenta": if palette.is_light { "#7A3E9D" } else { "#C792EA" }, "cyan": if palette.is_light { "#0E6A83" } else { "#65D1E5" }, "white": hex(palette.text),
                        "bright_black": hex(palette.muted), "bright_red": hex(palette.error), "bright_green": hex(palette.secondary), "bright_yellow": hex(palette.accent), "bright_blue": if palette.is_light { "#305FCE" } else { "#9DBBFF" }, "bright_magenta": if palette.is_light { "#A02F6F" } else { "#D383DA" }, "bright_cyan": if palette.is_light { "#1C6C66" } else { "#58D8AD" }, "bright_white": "#FFFFFF"
                    }
                }
            });
            output_json_or_text(
                value,
                command.format,
                format!(
                    "{} {} primary={} background={}",
                    state.scheme,
                    state.effective_mode,
                    hex(palette.primary),
                    hex(palette.bg)
                ),
            )
        }
        ThemeSubcommand::Status => {
            let registry = ThemeAdapterRegistry::first_release();
            let context = AdapterContext {
                effective_is_light: palette.is_light,
                takeover: Vec::new(),
            };
            let tools = registry
                .detect(&context)
                .into_iter()
                .map(|tool| ToolStatus {
                    id: tool.descriptor.id,
                    name: tool.descriptor.display_name,
                    kind: tool.descriptor.kind.as_str(),
                    mutation: match tool.descriptor.mutation {
                        crate::theme_adapters::MutationSupport::NativeFiles => "native_files",
                        crate::theme_adapters::MutationSupport::BuiltInIntegration => {
                            "built_in_integration"
                        }
                        crate::theme_adapters::MutationSupport::ReadOnlyInheritance => "read_only",
                    },
                    status: tool.status,
                    activation: tool.activation,
                    note: tool.note,
                })
                .collect::<Vec<_>>();
            let value = serde_json::json!({ "schema_version": 1, "command": "status", "theme": state, "tools": tools });
            let text = value["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|tool| {
                    format!(
                        "{}: {} ({})",
                        tool["id"].as_str().unwrap_or("unknown"),
                        tool["status"].as_str().unwrap_or("unknown"),
                        tool["kind"].as_str().unwrap_or("adapter")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            output_json_or_text(value, command.format, text)
        }
        ThemeSubcommand::Preview { tools } => {
            let registry = ThemeAdapterRegistry::first_release();
            for id in tools {
                if !registry.contains(id) {
                    anyhow::bail!("unknown theme tool `{id}`; run `kaku theme status` to list registered tools");
                }
            }
            let context = AdapterContext {
                effective_is_light: palette.is_light,
                takeover: Vec::new(),
            };
            let plans = registry.plans(AdapterOperation::Preview, &context, Some(tools));
            let value = serde_json::json!({
                "schema_version": 1,
                "command": "preview",
                "theme": state,
                "changed": false,
                "tools": plans.iter().map(|plan| serde_json::json!({
                    "id": plan.adapter_id,
                    "operation": "preview",
                    "result": plan.result,
                    "targets": plan.targets.iter().map(|path| display_path(path)).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            });
            output_json_or_text(
                value,
                command.format,
                plans
                    .iter()
                    .map(|plan| format!("{}: {}", plan.adapter_id, plan.result))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        ThemeSubcommand::Apply { tools, take_over } => {
            let registry = ThemeAdapterRegistry::first_release();
            for id in tools {
                if !registry.contains(id) {
                    anyhow::bail!("unknown theme tool `{id}`; run `kaku theme status` to list registered tools");
                }
            }
            for id in take_over {
                if !registry.contains(id) {
                    anyhow::bail!("unknown takeover target `{id}`; run `kaku theme status` to list registered tools");
                }
                if !tools.iter().any(|tool| tool == id) {
                    anyhow::bail!("takeover target `{id}` must also be listed in --tools");
                }
            }
            let context = AdapterContext {
                effective_is_light: palette.is_light,
                takeover: take_over.clone(),
            };
            let mut results = Vec::new();
            let mut failed = false;
            for id in tools {
                let result = registry.execute(id, AdapterOperation::Apply, &context);
                match result {
                    Ok(execution) => results.push(serde_json::json!({
                        "id": execution.adapter_id,
                        "result": execution.result,
                        "targets": execution.targets.iter().map(|path| display_path(path)).collect::<Vec<_>>(),
                    })),
                    Err(error) => {
                        failed = true;
                        results.push(serde_json::json!({
                            "id": id,
                            "result": "failed",
                            "error": error.to_string(),
                        }));
                    }
                }
            }
            let value = serde_json::json!({ "schema_version": 1, "command": "apply", "theme": state, "results": results });
            output_json_or_text(value, command.format, "theme apply completed".into())?;
            if failed {
                return Err(anyhow::Error::new(ThemeRunError {
                    code: 3,
                    message: "theme apply failed; inspect the per-tool results".into(),
                }));
            }
            Ok(())
        }
        ThemeSubcommand::Remove { tools } => {
            let registry = ThemeAdapterRegistry::first_release();
            for id in tools {
                if !registry.contains(id) {
                    anyhow::bail!("unknown theme tool `{id}`; run `kaku theme status` to list registered tools");
                }
            }
            let context = AdapterContext {
                effective_is_light: palette.is_light,
                takeover: Vec::new(),
            };
            let mut results = Vec::new();
            let mut failed = false;
            for id in tools {
                match registry.execute(id, AdapterOperation::Remove, &context) {
                    Ok(execution) => results.push(serde_json::json!({
                        "id": execution.adapter_id,
                        "result": execution.result,
                            "targets": execution.targets.iter().map(|path| display_path(path)).collect::<Vec<_>>(),
                    })),
                    Err(error) => {
                        failed = true;
                        results.push(serde_json::json!({ "id": id, "result": "failed", "error": error.to_string() }));
                    }
                }
            }
            let value = serde_json::json!({ "schema_version": 1, "command": "remove", "theme": state, "results": results });
            output_json_or_text(value, command.format, "theme remove completed".into())?;
            if failed {
                return Err(anyhow::Error::new(ThemeRunError {
                    code: 3,
                    message: "theme remove failed; inspect the per-tool results".into(),
                }));
            }
            Ok(())
        }
        ThemeSubcommand::Setup => {
            println!("Kaku theme setup is available through explicit adapter commands.");
            println!("Run `kaku theme status`, then `kaku theme preview --tools <list>` and `kaku theme apply --tools <list>`.");
            println!("Use `--take-over <adapter>` only when you want Kaku to replace an existing selector or theme asset.");
            Ok(())
        }
    }
}

fn output<T: Serialize>(value: &T, format: OutputFormat, text: String) -> anyhow::Result<()> {
    match format {
        OutputFormat::Text => println!("{text}"),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}

fn output_json_or_text(
    value: serde_json::Value,
    format: OutputFormat,
    text: String,
) -> anyhow::Result<()> {
    match format {
        OutputFormat::Text => println!("{text}"),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
    }
    Ok(())
}

fn state_from_config(config: &config::ConfigHandle, is_light: bool) -> ThemeState {
    let (selection, scheme, source) = match config.color_scheme.as_deref() {
        Some("Kaku Light") => ("light", "Kaku Light", "explicit"),
        Some("Kaku Dark") | Some("Kaku Theme") => ("dark", "Kaku Dark", "explicit"),
        Some(other) => ("custom", other, "custom"),
        None => (
            "auto",
            if is_light { "Kaku Light" } else { "Kaku Dark" },
            "macos",
        ),
    };
    ThemeState {
        selection: selection.into(),
        effective_mode: if is_light { "light" } else { "dark" }.into(),
        scheme: scheme.into(),
        appearance_source: source.into(),
    }
}

fn hex(color: wezterm_term::color::SrgbaTuple) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (color.0 * 255.0).round() as u8,
        (color.1 * 255.0).round() as u8,
        (color.2 * 255.0).round() as u8
    )
}

fn display_path(path: &std::path::Path) -> String {
    let home = config::HOME_DIR.as_path();
    path.strip_prefix(home)
        .map(|relative| format!("~/{relative}", relative = relative.display()))
        .unwrap_or_else(|_| path.display().to_string())
}
