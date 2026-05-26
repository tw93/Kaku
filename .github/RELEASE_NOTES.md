# V0.12.0 Sharper ✂️

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.12.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog

1. **AI Prompt Rebuild**: The chat system prompt is split into six topical fragments with versioned metadata headers and a CI gate, so the prompt chain stays auditable and stable for cache hits.
2. **Smarter Chat Loop**: `/suggest`, older-history fold-into-summary, JSON-output session titles, and a webfetch summarizer all route to the configured fast model and keep the context window tight.
3. **AI Shell Hardening**: The `#` quick-fix flow gains command-injection detection on known-risky patterns, externalized prompts, and intent-based dispatch between command synthesis, explain, and lookup paths.
4. **Tool Sandbox Audit**: File-access policy ships with thorough symlink-escape and credential-path tests; `.env` exposure is pinned as a known gap to address next.
5. **macOS Appearance**: Light/Dark flips refresh all windows in a single pass, the red-dot button cleanly exits Space fullscreen, and title-bar dragging is steadier on macOS 26.
6. **Document Open**: PDFs, images, audio, video, archives, and Office documents launch in their default app instead of being grabbed by VS Code.
7. **Window Polish**: Hollow cursor on the unfocused active pane, non-fancy tab bar top inset and cell height, and a stale "Restart to Update" menu item are all cleaned up.
8. **Font Scaling**: Prompt redraws are skipped while the font scale is settling, and PTY resizes flush only after the cell dimensions stabilize.
9. **`kaku chat` Overlay**: Every invocation reliably retriggers the AI chat overlay, even when the user-var value would otherwise be deduped.
10. **Tidy**: Simplified Chinese localization is removed (the `language` option is still accepted as a deprecated field), `smart_tab_mode` and an opt-in `SmartPrompt` close-confirmation mode are added, dependencies are audit-clean, and new CI gates cover logs, clippy, and prompt metadata.

### 更新日志

1. **AI 提示重构**：聊天系统提示拆成六个主题片段，每段带版本化的 metadata 头，并加上 CI 校验，提示链可审计也对缓存友好。
2. **对话回路更利**：`/suggest`、旧历史 fold 成 summary、JSON 输出的会话标题、webfetch 摘要全部走配置中的 fast model，让上下文更紧。
3. **AI Shell 加固**：`#` 快速修复流加入命令注入检测、提示外置，并按意图在命令合成、解释、查询之间分发。
4. **工具沙箱审计**：文件访问策略补充了软链逃逸与凭证路径的完整测试；`.env` 文件暴露作为已知缺口固化，等待下一版处理。
5. **macOS 外观**：浅/深色切换会一次性刷新所有窗口，红点按钮干净退出 Space 全屏，macOS 26 上标题栏拖动更稳。
6. **文档默认打开**：PDF、图片、音视频、压缩包、Office 文档都走系统默认 app，不再被 VS Code 抢走。
7. **窗口细节**：非聚焦活动 pane 的空心光标、非 fancy 标签栏的顶部内距与 cell 高度、菜单里残留的"重启更新"项都做了收敛。
8. **字体缩放**：缩放未稳定前跳过 prompt 重绘，PTY 大小调整在 cell 维度稳定后才一次性下发。
9. **`kaku chat` 浮层**：每次调用都能稳定触发 AI 聊天浮层，不再被 UserVar 去重机制吞掉。
10. **轻装**：简体中文本地化整体移除（`language` 字段仍作为 deprecated 字段保留兼容），新增 `smart_tab_mode` 与可选的 `SmartPrompt` 关闭确认模式，依赖 audit clean，新增日志、clippy、提示元数据三道 CI 门禁。

> https://github.com/tw93/Kaku
