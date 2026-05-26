//! Built-in tools for the Kaku AI chat overlay.
//!
//! `ai_tools/` is split into focused submodules so each concern can be read
//! and audited independently. The public surface is unchanged: callers use
//! `execute`, `all_tools`, `to_api_schema`, `cleanup_spill_files`, and the
//! two path helpers.
//!
//! Submodule layout (matches the long-term plan in `kaku-gui/AGENTS.md`):
//!
//! | module        | responsibility                                      |
//! |---------------|-----------------------------------------------------|
//! | `paths`       | path resolution and sensitive-path guards           |
//! | `fs`          | fs_read / fs_list / fs_write / fs_patch / mkdir / delete |
//! | `shell`       | shell_exec / shell_bg / shell_poll + BgProcess registry |
//! | `web`         | web_fetch / http_request / read_url + HTTP helpers  |
//! | `search`      | web_search / symbol_search / grep_search            |
//! | `project`     | project_summary / file_tree                         |
//! | `soul`        | memory_read / soul_read + spill-file cleanup        |
//! | `registry`    | ToolDef, all_tools, to_api_schema, budget_for       |

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

mod fs;
pub(crate) mod paths;
mod project;
mod registry;
mod search;
mod shell;
mod soul;
mod web;

// ─── Public re-exports ────────────────────────────────────────────────────────

pub use registry::{all_tools, to_api_schema};
pub use soul::cleanup_spill_files;
// `pub(crate) use` of these helpers is the canonical access path for callers
// outside this module (`overlay/ai_chat/state.rs` reaches them via
// `crate::ai_tools::memory_file_path()`). rustc still flags the binding as
// unused because nothing inside *this* file consults it, hence the explicit
// allow — removing it would force consumers to learn the internal `soul`
// submodule name, which leaks the planned-internal split.
#[allow(unused_imports)]
pub(crate) use soul::{memory_file_path, onboarding_flag_path};

// ─── Dispatcher ───────────────────────────────────────────────────────────────

/// Execute a tool by name. `args` is the parsed JSON from the model.
/// `cwd` is the agent's current working directory; shell_exec updates it
/// in-place when the command changes directory via `cd`.
/// `cancel` is polled by long-running tools so Esc / session shutdown can
/// interrupt a hung child process.
pub fn execute(
    name: &str,
    args: &serde_json::Value,
    cwd: &mut String,
    config: &crate::ai_client::AssistantConfig,
    cancel: &Arc<AtomicBool>,
) -> Result<String> {
    let detail = args["detail"].as_str().unwrap_or("default");
    let cap = registry::budget_for(name, detail);

    let result = match name {
        "fs_read" => fs::exec_fs_read(args, cwd, cap)?,
        "fs_list" => fs::exec_fs_list(args, cwd)?,
        "fs_write" => fs::exec_fs_write(args, cwd)?,
        "fs_patch" => fs::exec_fs_patch(args, cwd)?,
        "fs_mkdir" => fs::exec_fs_mkdir(args, cwd)?,
        "fs_delete" => fs::exec_fs_delete(args, cwd)?,
        "shell_exec" => shell::exec_shell_exec(args, cwd, cap, cancel)?,
        "shell_bg" => shell::exec_shell_bg(args, cwd)?,
        "shell_poll" => shell::exec_shell_poll(args, cancel)?,
        "pwd" => cwd.clone(),
        "web_fetch" => {
            let url = args["url"].as_str().context("missing url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("url must start with http:// or https://");
            }
            let raw = if let Some(script) = &config.web_fetch_script {
                let output = std::process::Command::new("bash")
                    .arg(script)
                    .arg("--")
                    .arg(url)
                    .output()
                    .context("web_fetch_script exec failed")?;
                if !output.status.success() {
                    anyhow::bail!(
                        "fetch script failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                String::from_utf8_lossy(&output.stdout).into_owned()
            } else {
                web::fetch_markdown_default(url)?
            };
            let raw_passthrough =
                web::should_return_raw_fetch(detail, args["raw"].as_bool().unwrap_or(false));
            web::maybe_summarize_fetched(url, raw, config, raw_passthrough)
        }
        "web_search" => search::exec_web_search(args, config)?,
        "read_url" => {
            let url = args["url"].as_str().context("missing url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("url must start with http:// or https://");
            }
            let provider = config.web_search_provider.as_deref().unwrap_or("");
            let api_key = config.web_search_api_key.as_deref().unwrap_or("");
            let raw = web::exec_read_url(url, provider, api_key)?;
            let raw_passthrough =
                web::should_return_raw_fetch(detail, args["raw"].as_bool().unwrap_or(false));
            web::maybe_summarize_fetched(url, raw, config, raw_passthrough)
        }
        "project_summary" => {
            let raw_path = args["path"].as_str();
            let scan_path = raw_path
                .map(|p| paths::resolve(p, cwd))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from(cwd.as_str()));
            if let Some(raw_path) = raw_path {
                paths::reject_relative_cwd_escape(raw_path, &scan_path, cwd)?;
            }
            paths::reject_if_sensitive(&scan_path)?;
            project::exec_project_summary(&scan_path)?
        }
        "file_tree" => {
            let raw_path = args["path"].as_str();
            let tree_path = raw_path
                .map(|p| paths::resolve(p, cwd))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from(cwd.as_str()));
            if let Some(raw_path) = raw_path {
                paths::reject_relative_cwd_escape(raw_path, &tree_path, cwd)?;
            }
            paths::reject_if_sensitive(&tree_path)?;
            let depth = args["depth"].as_u64().unwrap_or(3).min(6) as usize;
            project::exec_file_tree(&tree_path, depth)?
        }
        "symbol_search" => {
            let query = args["query"].as_str().context("missing query")?;
            let search_path = args["path"].as_str().unwrap_or(cwd);
            let resolved = paths::resolve(search_path, cwd)?;
            paths::reject_relative_cwd_escape(search_path, &resolved, cwd)?;
            paths::reject_if_sensitive(&resolved)?;
            let kind = args["kind"].as_str().unwrap_or("all");
            let glob_filter = args["glob"].as_str();
            search::exec_symbol_search(query, kind, search_path, glob_filter, cwd, cancel)?
        }
        "grep_search" => {
            let pattern = args["pattern"].as_str().context("missing pattern")?;
            let search_path = args["path"].as_str().unwrap_or(cwd);
            let resolved = paths::resolve(search_path, cwd)?;
            paths::reject_relative_cwd_escape(search_path, &resolved, cwd)?;
            paths::reject_if_sensitive(&resolved)?;
            let context_lines = args["context_lines"].as_u64().unwrap_or(2) as usize;
            let case_insensitive = args["case_insensitive"].as_bool().unwrap_or(false);
            let max_results = args["max_results"].as_u64().unwrap_or(100) as usize;
            let glob_filter = args["glob"].as_str();
            search::exec_grep_search(
                pattern,
                search_path,
                glob_filter,
                context_lines,
                case_insensitive,
                max_results,
                cwd,
                cancel,
            )?
        }
        "memory_read" => {
            let path = soul::memory_file_path();
            match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(_) => "(no memories yet)".into(),
            }
        }
        "soul_read" => {
            let file = args["file"].as_str().unwrap_or("all");
            soul::exec_soul_read(file)?
        }
        "http_request" => {
            let method = args["method"].as_str().context("missing method")?;
            let url = args["url"].as_str().context("missing url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("url must start with http:// or https://");
            }
            let body = args["body"].as_str();
            let headers = args["headers"].as_object();
            let query_params = args["query"].as_object();
            web::exec_http_request(method, url, headers, body, query_params)?
        }
        _ => anyhow::bail!("unknown tool: {}", name),
    };

    soul::truncate_and_spill(result, cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_client::AssistantConfig;

    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn dummy_config() -> AssistantConfig {
        AssistantConfig {
            api_key: "test".to_string(),
            chat_model: "test".to_string(),
            chat_model_choices: vec![],
            base_url: "https://example.com".to_string(),
            custom_headers: vec![],
            provider: "Custom".to_string(),
            auth_type: "api_key".to_string(),
            chat_tools_enabled: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            fast_model: None,
            memory_curator_model: None,
        }
    }

    // `resolve_*` and `reject_if_sensitive` tests live in `paths.rs`.

    #[test]
    fn fs_read_refuses_ssh_directory() {
        let home = std::env::var("HOME").expect("HOME not set");
        let ssh = format!("{}/.ssh", home);
        if !std::path::Path::new(&ssh).exists() {
            return;
        }
        let mut cwd = home.clone();
        let cfg = dummy_config();
        let result = execute(
            "fs_read",
            &serde_json::json!({"path": ssh + "/id_rsa"}),
            &mut cwd,
            &cfg,
            &no_cancel(),
        );
        assert!(result.is_err(), "fs_read should refuse ~/.ssh/id_rsa");
    }

    #[test]
    fn search_tools_refuse_ssh_directory() {
        let home = std::env::var("HOME").expect("HOME not set");
        let ssh = format!("{}/.ssh", home);
        if !std::path::Path::new(&ssh).exists() {
            return;
        }
        let mut cwd = home.clone();
        let cfg = dummy_config();
        let cancel = no_cancel();
        let cases = [
            ("project_summary", serde_json::json!({"path": &ssh})),
            ("file_tree", serde_json::json!({"path": &ssh})),
            (
                "symbol_search",
                serde_json::json!({"query": "BEGIN", "path": &ssh}),
            ),
            (
                "grep_search",
                serde_json::json!({"pattern": "BEGIN", "path": &ssh}),
            ),
        ];

        for (name, args) in cases {
            let err = execute(name, &args, &mut cwd, &cfg, &cancel)
                .err()
                .unwrap_or_else(|| panic!("{} should refuse ~/.ssh", name));
            assert!(
                err.to_string().contains("protected secret location"),
                "{} returned wrong error: {}",
                name,
                err
            );
        }
    }

    #[test]
    fn search_tools_reject_relative_cwd_escape() {
        let sandbox = tempfile::tempdir().expect("tempdir");
        let project = sandbox.path().join("project");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("lib.rs"), "fn outside_fn() {}\n").unwrap();

        let mut cwd = project.to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();
        let cases = [
            ("project_summary", serde_json::json!({"path": "../outside"})),
            ("file_tree", serde_json::json!({"path": "../outside"})),
            (
                "symbol_search",
                serde_json::json!({"query": "outside_fn", "path": "../outside"}),
            ),
            (
                "grep_search",
                serde_json::json!({"pattern": "outside_fn", "path": "../outside"}),
            ),
        ];

        for (name, args) in cases {
            let err = execute(name, &args, &mut cwd, &cfg, &cancel)
                .err()
                .unwrap_or_else(|| panic!("{} should reject relative cwd escape", name));
            assert!(
                err.to_string().contains("outside the working directory"),
                "{} returned wrong error: {}",
                name,
                err
            );
        }
    }

    #[test]
    fn search_tools_allow_absolute_non_sensitive_paths() {
        let sandbox = tempfile::tempdir().expect("tempdir");
        let project = sandbox.path().join("project");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("Cargo.toml"),
            "[package]\nname = \"outside\"\n",
        )
        .unwrap();
        std::fs::write(outside.join("lib.rs"), "fn outside_fn() {}\n").unwrap();

        let outside_path = outside.to_string_lossy().into_owned();
        let mut cwd = project.to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let summary = execute(
            "project_summary",
            &serde_json::json!({"path": &outside_path}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(summary.contains("outside"), "summary: {}", summary);

        let tree = execute(
            "file_tree",
            &serde_json::json!({"path": &outside_path}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(tree.contains("lib.rs"), "tree: {}", tree);

        let symbols = execute(
            "symbol_search",
            &serde_json::json!({"query": "outside_fn", "kind": "function", "path": &outside_path}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(symbols.contains("outside_fn"), "symbols: {}", symbols);

        let grep = execute(
            "grep_search",
            &serde_json::json!({"pattern": "outside_fn", "path": &outside_path}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(grep.contains("outside_fn"), "grep: {}", grep);
    }

    fn create_project_fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"demo_app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("src/main.rs"),
            "fn main() { println!(\"hello\"); }\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("src/lib.rs"),
            "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("src/utils.rs"),
            "// TODO: add input validation\npub fn sanitize_input(input: &str) -> String {\n    input.trim().to_string()\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("src/config.rs"),
            "pub struct AppConfig {\n    pub verbose: bool,\n    pub name: String,\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.path().join("tests")).unwrap();
        std::fs::write(
            root.path().join("tests/integration.rs"),
            "#[test]\nfn test_greet() {\n    assert_eq!(demo_app::greet(\"Rust\"), \"Hello, Rust!\");\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("docs/API.md"),
            "# API\n\n## greet(name)\nReturns a greeting string.\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn eval_01_project_and_tree() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let summary = execute(
            "project_summary",
            &serde_json::json!({}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            summary.contains("Rust"),
            "project_summary should detect Rust"
        );
        assert!(
            summary.contains("cargo"),
            "project_summary should detect cargo"
        );

        let tree = execute(
            "file_tree",
            &serde_json::json!({"depth": 2}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(tree.contains("src/"), "file_tree should list src/");
    }

    #[test]
    fn eval_02_grep_and_symbol() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let grep = execute(
            "grep_search",
            &serde_json::json!({"pattern": "fn greet", "glob": "*.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            grep.contains("greet"),
            "grep_search should find fn greet: {}",
            grep
        );

        let sym = execute(
            "symbol_search",
            &serde_json::json!({"query": "greet", "kind": "function"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            sym.contains("greet"),
            "symbol_search should find greet: {}",
            sym
        );
    }

    #[test]
    fn eval_03_fs_write_read_patch_list() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        execute(
            "fs_write",
            &serde_json::json!({"path": "src/new_module.rs", "content": "pub fn new_fn() {}\n"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();

        let read = execute(
            "fs_read",
            &serde_json::json!({"path": "src/new_module.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            read.contains("new_fn"),
            "fs_read should return written content: {}",
            read
        );

        execute(
            "fs_patch",
            &serde_json::json!({
                "path": "src/new_module.rs",
                "old_text": "pub fn new_fn() {}",
                "new_text": "pub fn new_fn() { println!(\"patched\"); }"
            }),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();

        let patched = execute(
            "fs_read",
            &serde_json::json!({"path": "src/new_module.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            patched.contains("patched"),
            "fs_read after patch should contain new text: {}",
            patched
        );

        let list = execute(
            "fs_list",
            &serde_json::json!({"path": "src"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            list.contains("new_module.rs"),
            "fs_list should include new file: {}",
            list
        );
    }

    #[test]
    fn eval_04_mkdir_and_nested_write() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        execute(
            "fs_mkdir",
            &serde_json::json!({"path": "src/subdir/nested"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();

        execute(
            "fs_write",
            &serde_json::json!({
                "path": "src/subdir/nested/file.rs",
                "content": "// nested\n"
            }),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();

        let list = execute(
            "fs_list",
            &serde_json::json!({"path": "src/subdir/nested"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            list.contains("file.rs"),
            "nested write should be listable: {}",
            list
        );
    }

    #[test]
    fn eval_05_shell_exec_basic() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let out = execute(
            "shell_exec",
            &serde_json::json!({"command": "echo hello_kaku"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            out.contains("hello_kaku"),
            "shell_exec should return echo output: {}",
            out
        );
    }

    #[test]
    fn eval_06_shell_exec_cancel() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            cancel_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let out = execute(
            "shell_exec",
            &serde_json::json!({"command": "sleep 60"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            out.contains("canceled"),
            "shell_exec should report cancel: {}",
            out
        );
    }

    #[test]
    fn shell_exec_respects_cancel_flag() {
        let mut cwd = std::env::temp_dir().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let out = execute(
            "shell_exec",
            &serde_json::json!({"command": "sleep 60"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            out.contains("canceled"),
            "should be canceled immediately: {}",
            out
        );
    }

    #[test]
    fn shell_exec_overflow_still_honors_cancel() {
        let mut cwd = std::env::temp_dir().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            cancel_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let out = execute(
            "shell_exec",
            &serde_json::json!({"command": "yes | head -c 1000000"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            out.contains("canceled") || out.contains("truncated"),
            "overflow+cancel: {}",
            out
        );
    }

    #[test]
    fn eval_09_delete_file() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let del = execute(
            "fs_delete",
            &serde_json::json!({"path": "docs/API.md"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(del.contains("Deleted"), "should confirm deletion: {}", del);

        let after = execute(
            "fs_list",
            &serde_json::json!({"path": "docs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            !after.contains("API.md"),
            "API.md should be gone after delete: {}",
            after
        );
    }

    #[test]
    fn eval_10_error_paths() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let read_err = execute(
            "fs_read",
            &serde_json::json!({"path": "src/nonexistent.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        );
        assert!(
            read_err.is_err(),
            "fs_read of nonexistent file should return Err"
        );

        let grep_result = execute(
            "grep_search",
            &serde_json::json!({"pattern": "ZZZZNOTEXIST"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            grep_result.contains("No matches"),
            "grep for nonexistent pattern should report no matches: {}",
            grep_result
        );
    }
}
