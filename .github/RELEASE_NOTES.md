# V0.9.0

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.9.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog

1. **Lower CPU and Smarter Titles**: Reduced long-running idle CPU by batching tab title callbacks, coalescing title and status refreshes, and filtering mux notifications per window. Window titles now track the active tab and cwd more reliably.
2. **Mouse and Link Handling**: `Option+Click` moves the shell cursor to the clicked column on the same line, with wide characters handled correctly. Wrapped hyperlinks now open as full URLs instead of truncated fragments.
3. **Rendering and Theme Fixes**: Block cursor rendering now matches the full cell height in editors like Nvim. Dim text contrast is improved, and the bundled theme follows macOS light and dark appearance correctly by default.
4. **macOS Polish**: Added a Traffic Lights toggle in Settings, exposed Always on Top in the Window menu, normalized Finder service file targets to their parent directory, and improved dock grouping and existing-window handoff behavior.
5. **Shell Integration Hardening**: Zsh updates now preserve custom inline blocks, bundle `zsh-z`, improve `cd` + `Tab` fallback behavior, and surface clearer update feedback during managed shell refreshes.
6. **Assistant and Provider Setup**: Added MiniMax as a built-in provider preset, improved provider auto-detection in `kaku ai`, and send `User-Agent: Kaku/<version>` on assistant API requests for better provider-side attribution.

### 更新日志

1. **更低 CPU 占用与更稳定的标题更新**：通过批量处理 tab title 回调、合并 title 和 status 刷新、按窗口过滤 mux 通知，修复了长时间运行后的高 CPU 问题。窗口标题现在也能更稳定地跟随当前 tab 和 cwd 更新。
2. **鼠标与链接交互改进**：支持同一行上的 `Option+Click` 光标定位，并正确处理宽字符。换行后的长链接现在会作为完整 URL 打开，不再只打开被截断的一段。
3. **渲染与主题修复**：Block cursor 在 Nvim 等编辑器里的高度现与完整 cell 对齐。半亮文本对比度更合理，内置主题在默认情况下也会正确跟随 macOS 明暗外观切换。
4. **macOS 体验打磨**：设置中新增 Traffic Lights 开关，Window 菜单加入 Always on Top，Finder 服务会把文件目标规范化为父目录，并改进了 dock 分组和复用现有窗口的行为。
5. **Shell 集成加固**：Zsh 更新流程现在会保留带有自定义内容的 inline block，默认集成 `zsh-z`，改进 `cd` + `Tab` 的兜底补全行为，并在受管 shell 刷新时提供更清晰的更新反馈。
6. **Assistant 与 Provider 配置改进**：内置新增 MiniMax provider preset，`kaku ai` 中的 provider 自动识别更可靠，并会在 assistant API 请求中附带 `User-Agent: Kaku/<version>` 头。

> https://github.com/tw93/Kaku
