//! Tool registry: ToolDef, all_tools, to_api_schema, and output budgets.

use crate::ai_client::AssistantConfig;
use std::borrow::Cow;

/// A single callable tool exposed to the AI model.
pub struct ToolDef {
    pub name: &'static str,
    pub description: Cow<'static, str>,
    /// JSON Schema for the function's parameters.
    pub parameters: serde_json::Value,
}

/// All tools exposed to the model, filtered by the active configuration.
pub fn all_tools(config: &AssistantConfig) -> Vec<ToolDef> {
    let mut tools = vec![
        ToolDef {
            name: "fs_read",
            description: Cow::Borrowed(
                "Read a file and return its content. By default returns the whole file up to the \
                 output cap. Use start_line / end_line to read a specific range (1-indexed, \
                 inclusive). Efficient for large files when you only need a section.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or ~/relative path" },
                    "start_line": {
                        "type": "integer",
                        "description": "First line to return (1 = first line of file). Optional."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Last line to return (inclusive). Optional."
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "fs_list",
            description: Cow::Borrowed("List files and sub-directories inside a directory. \
                          Directories are shown with a trailing /."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "fs_write",
            description: Cow::Borrowed("Write (create or overwrite) a file with the given content. \
                          Parent directories are created automatically."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path":    { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "fs_patch",
            description: Cow::Borrowed("Replace the first occurrence of `old_text` with `new_text` in a file. \
                          Fails if old_text is not found."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path":     { "type": "string" },
                    "old_text": { "type": "string", "description": "Exact text to find" },
                    "new_text": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        },
        ToolDef {
            name: "fs_mkdir",
            description: Cow::Borrowed("Create a directory and all missing parent directories."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "fs_delete",
            description: Cow::Borrowed("Delete a file or directory (recursive for directories)."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "shell_exec",
            description: Cow::Borrowed(
                "Run an arbitrary shell command via bash and return stdout + stderr. \
                 Use for building, testing, grepping, git, npm, cargo, etc. \
                 Output is capped; for commands that produce large output or run \
                 indefinitely, use shell_bg + shell_poll instead.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute (passed to bash -c)"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory override (optional, defaults to pane cwd)"
                    },
                    "detail": {
                        "type": "string",
                        "enum": ["brief", "default", "full"],
                        "description": "Output size: 'brief' for summaries, 'default' (standard cap), \
                                        'full' for deep inspection. Default: 'default'."
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "shell_bg",
            description: Cow::Borrowed("Start a long-running shell command in the background and return its process id immediately. \
                          Use for commands that take minutes (builds, dev servers, watchers). \
                          Call shell_poll to check status and collect output."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to run in background"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory (optional)"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "shell_poll",
            description: Cow::Borrowed("Check the status of a background process started with shell_bg. \
                          Returns accumulated stdout/stderr and whether the process has exited. \
                          Pass timeout_secs > 0 to wait up to that many seconds for it to finish."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pid": {
                        "type": "integer",
                        "description": "Process id returned by shell_bg"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Seconds to wait for process exit (0 = non-blocking check)"
                    }
                },
                "required": ["pid"]
            }),
        },
        ToolDef {
            name: "pwd",
            description: Cow::Borrowed("Return the current working directory of the terminal pane."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ];

    tools.push(ToolDef {
        name: "web_fetch",
        description: Cow::Borrowed(
            "Fetch a URL and return its content as Markdown. \
             Uses defuddle.md then r.jina.ai as free anonymous backends. \
             Use for reading documentation, articles, or any public web page.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Full URL to fetch (must start with http:// or https://)"
                },
                "detail": {
                    "type": "string",
                    "enum": ["brief", "default", "full"],
                    "description": "Output size. Default: 'default'. Use 'full' when exact source text is needed; it disables automatic summarization."
                },
                "raw": {
                    "type": "boolean",
                    "description": "Return fetched content verbatim and skip automatic summarization. Use for exact quotes, debugging, or source inspection."
                }
            },
            "required": ["url"]
        }),
    });

    if config.web_search_ready() {
        let provider = config.web_search_provider.as_deref().unwrap_or("search");
        tools.push(ToolDef {
            name: "web_search",
            description: Cow::Owned(format!(
                "Search the web using {} and return results with title, URL, snippet, and (where supported) \
                 a direct AI answer. Use for finding current information, documentation, or answering questions. \
                 Use kind='news' for recent events; kind='deep' (pipellm) for richer RAG results; \
                 freshness to limit by recency.",
                provider
            )),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "kind": {
                        "type": "string",
                        "enum": ["web", "news", "deep"],
                        "description": "'web' (default), 'news' (recent news), or 'deep' (pipellm: full RAG pipeline)"
                    },
                    "freshness": {
                        "type": "string",
                        "description": "Recency filter: 'pd' (24h), 'pw' (7d), 'pm' (31d), 'py' (1y). \
                                        Brave also accepts custom ranges like '2024-01-01to2024-06-30'."
                    },
                    "search_depth": {
                        "type": "string",
                        "enum": ["basic", "advanced"],
                        "description": "Tavily only. 'advanced' performs deeper crawling for richer results."
                    }
                },
                "required": ["query"]
            }),
        });

        tools.push(ToolDef {
            name: "read_url",
            description: Cow::Borrowed(
                "Fetch a web page and return its clean text content, optimized for AI reading. \
                 Use after web_search to read the full content of a promising result. \
                 Handles JS-heavy pages better than web_fetch for supported providers.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Full URL to read (must start with http:// or https://)"
                    },
                    "detail": {
                        "type": "string",
                        "enum": ["brief", "default", "full"],
                        "description": "Output size. Default: 'default'. Use 'full' when exact source text is needed; it disables automatic summarization."
                    },
                    "raw": {
                        "type": "boolean",
                        "description": "Return fetched content verbatim and skip automatic summarization. Use for exact quotes, debugging, or source inspection."
                    }
                },
                "required": ["url"]
            }),
        });
    }

    tools.push(ToolDef {
        name: "project_summary",
        description: Cow::Borrowed(
            "Scan a directory for project markers (Cargo.toml, package.json, go.mod, \
             Makefile, .git, etc.) and return a brief summary: language, build system, \
             key directories, entry points. Call this first on unfamiliar codebases.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory to scan (defaults to cwd)" }
            },
            "required": []
        }),
    });

    tools.push(ToolDef {
        name: "file_tree",
        description: Cow::Borrowed(
            "List the directory tree up to a given depth, skipping .git, node_modules, \
             target, and other common noise directories. Useful for understanding project \
             structure before searching.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Root directory (defaults to cwd)" },
                "depth": {
                    "type": "integer",
                    "description": "Maximum depth to recurse (default 3, max 6)"
                }
            },
            "required": []
        }),
    });

    tools.push(ToolDef {
        name: "symbol_search",
        description: Cow::Borrowed(
            "Find symbol definitions (functions, types, traits, classes, methods) by name. \
             More precise than grep_search for code navigation because it uses language-aware \
             patterns to match definitions, not arbitrary occurrences.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Symbol name or partial name to search for" },
                "kind": {
                    "type": "string",
                    "enum": ["function", "type", "class", "method", "all"],
                    "description": "Kind of symbol to find (default: all)"
                },
                "path": { "type": "string", "description": "Directory to search in (defaults to cwd)" },
                "glob": { "type": "string", "description": "File glob filter, e.g. '*.rs' or '*.{ts,tsx}'" }
            },
            "required": ["query"]
        }),
    });

    tools.push(ToolDef {
        name: "grep_search",
        description: Cow::Borrowed(
            "Recursively search for a regex pattern in files and return matching lines with context. \
             Use for finding symbol definitions, usages, TODO comments, or any text pattern across \
             the codebase. Faster and more precise than reading individual files.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regular expression pattern to search for" },
                "path": { "type": "string", "description": "Directory or file to search in (defaults to cwd)" },
                "glob": { "type": "string", "description": "File glob filter, e.g. '*.rs' or '*.{ts,tsx}' (optional)" },
                "context_lines": { "type": "integer", "description": "Lines of context before and after each match (default 2)" },
                "case_insensitive": { "type": "boolean", "description": "Case-insensitive matching (default false)" },
                "max_results": { "type": "integer", "description": "Maximum number of matching lines to return (default 100)" },
                "detail": {
                    "type": "string",
                    "enum": ["brief", "default", "full"],
                    "description": "Output size. Default: 'default'."
                }
            },
            "required": ["pattern"]
        }),
    });

    tools.push(ToolDef {
        name: "memory_read",
        description: Cow::Borrowed(
            "Read the rolling memory file that stores persistent facts, \
             preferences, and project context across AI chat sessions. \
             Kaku updates this file automatically after each conversation; \
             you do not need to write to it yourself.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    });

    tools.push(ToolDef {
        name: "soul_read",
        description: Cow::Borrowed(
            "Read one of the user's soul identity files. These are stable, \
             user-authored documents that describe who the user is (soul), \
             their preferred style (style), and how they work (skill). \
             Call this when you need to recall a specific identity detail \
             not already present in the system prompt.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "enum": ["soul", "style", "skill", "memory"],
                    "description": "Which soul file to read. Omit to read all four."
                }
            },
            "required": []
        }),
    });

    tools.push(ToolDef {
        name: "http_request",
        description: Cow::Borrowed(
            "Make an HTTP request (GET, POST, PUT, PATCH, DELETE) and return the response status, \
             headers, and body. Use for testing APIs, fetching JSON endpoints, or any HTTP call \
             that requires a specific method or request body. For web pages, prefer web_fetch instead.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"],
                    "description": "HTTP method"
                },
                "url": { "type": "string", "description": "Full URL (must start with http:// or https://)" },
                "headers": {
                    "type": "object",
                    "description": "Optional extra request headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Request body (for POST/PUT/PATCH). If it is valid JSON, \
                                   Content-Type is set to application/json automatically."
                },
                "query": {
                    "type": "object",
                    "description": "Optional URL query parameters as key-value pairs",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["method", "url"]
        }),
    });

    tools
}

/// Serialize a ToolDef into the JSON object expected by the OpenAI API.
pub fn to_api_schema(tool: &ToolDef) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}

/// Fallback output cap for tools not matched in `budget_for`.
const DEFAULT_RESULT_BYTES: usize = 8_000;

/// Per-tool byte budgets for tool-call results.
/// `detail` maps to the budget tier: "brief" / "default" / "full".
pub(super) fn budget_for(tool: &str, detail: &str) -> usize {
    let (default_bytes, max_bytes): (usize, usize) = match tool {
        "fs_list" | "pwd" | "memory_read" | "soul_read" | "project_summary" => (2_000, 4_000),
        "fs_read" | "grep_search" | "symbol_search" => (8_000, 16_000),
        "file_tree" => (4_000, 8_000),
        "shell_exec" | "shell_poll" => (12_000, 24_000),
        "web_fetch" | "read_url" => (10_000, 20_000),
        "shell_bg" => (8_000, 8_000),
        _ => (DEFAULT_RESULT_BYTES, DEFAULT_RESULT_BYTES),
    };
    match detail {
        "brief" => default_bytes / 2,
        "full" => max_bytes,
        _ => default_bytes,
    }
}
