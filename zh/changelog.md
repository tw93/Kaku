# 更新日志

Kaku 正式版更新，从新到旧。Nightly 仍在 GitHub，可以用 [RSS](https://kaku.fun/feed.xml) 订阅。

## V0.22.0 Handy 2026-10-05

1. **标签栏**：远程桌面下用手指也更容易点中标签，标签栏末尾多了 + 按钮，右键标签可以新建、打开标签导航或关闭标签。
2. **输入卡顿**：Kaku 在后台保存会话时，显示过内联图片的分屏不会再让输入卡住。
3. **程序标题**：设置 `config.tab_title_use_pane_title = true` 后，标签会显示 Claude Code 这类程序设置的标题，手动重命名过的标签保留你起的名字。
4. **Kitty 键盘**：开启 `enable_kitty_keyboard` 后，Herdr 和 Neovim 里的 Esc 和向前删除恢复正常，交换 Backspace 与 Delete 的设置也会生效。
5. **输入法**：设置 `config.ime_preedit_rendering = 'BuiltinInsert'` 后，输入法正在组合的文字会插入显示，不再盖住后面的字。
6. **Powerlevel10k**：Kaku 不再在第一个提示符前输出内容，Powerlevel10k 的 instant prompt 不会再报警告。
7. **命令行**：`kaku start` 会在已经运行的 Kaku 里打开，不再另起一个，加上 `--new-tab` 会在当前窗口新建标签。
8. **分屏**：上下分屏的分隔线现在落在两侧背景的交界处，开启非活动分屏变暗后，分隔线融入背景，变暗也会铺满到窗口边缘。
9. **诊断**：`kaku doctor` 会生成一份脱敏的诊断包，提 bug 时可以直接附上。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.22.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.22.0/Kaku.dmg)

---

## V0.21.0 Ink 2026-09-26

1. **官方 Homebrew**：Kaku 终于进了 Homebrew 官方 cask，直接用 `brew install --cask kaku` 就能安装，`kaku update` 也能识别，之前通过个人 tap 安装的照样能继续更新。
2. **会话恢复**：运行时会定期保存窗口、标签和分屏，崩溃或强制退出后也能恢复上次的布局。
3. **右键菜单**：选中文字后右键菜单顶部会出现复制，关掉选中即复制时也能用鼠标复制。
4. **标签栏**：有分屏的标签也会显示序号。
5. **主题**：新安装默认使用 Kaku Dark，已有安装保持原来的主题，Kaku Light 的亮青色也更容易看清。
6. **稳定与安全**：正在关闭的标签不会再导致所有窗口一起退出，并更新 TLS 库修复一个安全公告中的问题。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.21.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.21.0/Kaku.dmg)

Special thanks to @TeamMeng for their contribution to this release.

---

## V0.20.0 Steady 2026-09-12

1. **窗口操作**：恢复 macOS 原生拖动和边缘平铺，修复关闭窗口后可能出现的崩溃，图形初始化失败后新窗口不再重复等待。
2. **Shell 集成**：修复 TUI 异常退出后鼠标移动产生乱码的问题，保留 tmux 中的提示符和智能补全，并修正 fish 的 grep 补全展开。
3. **AI 模型选择**：切换服务后不再沿用旧服务的模型列表，完整展示服务返回的模型，并保留手动配置的模型。
4. **AI 对话**：补充当前项目类型信息，远程会话不读取本机项目，取消请求时不再等待重试倒计时结束。
5. **右键菜单**：支持粘贴、搜索、AI 对话和分屏操作，也可以在设置中开启新建标签页按钮。
6. **主题设置**：独立配置也能使用内置深浅主题，通过自定义配置启动时设置会保存到对应文件，手动配色覆盖主题时会显示说明，Fancy 标签栏也会跟随主题配色。
7. **链接识别**：避免把换行后的无关输出拼进链接，保留自动折行网址的完整地址。
8. **版本查询**：图形程序的版本查询无需初始化窗口即可正确返回。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.20.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.20.0/Kaku.dmg)

Special thanks to @elonnzhang, @trxuan, and @yansigit for their contributions to this release.

---

## V0.19.0 Restored 2026-08-24

1. **移除分屏输入广播**：这个功能容易误触，会把危险命令重复到无关分屏，原有快捷键保留但不再生效。
2. **会话恢复更完整**：重新打开时窗口、分屏和各自的目录都会还原，个别分屏未能保存也不影响其余。
3. **关闭不再误伤其他分屏**：确认框始终对应打开它的分屏，关闭当前标签页后会停在预期的标签页。
4. **显示与集成修复**：浅色主题选中内容清晰可见，lazygit 支持嵌套 shell，清空历史不打断全屏程序，重命名标签页不卡住标题，慢速同步输出不再撕裂。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.19.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.19.0/Kaku.dmg)

Special thanks to @shlroland and @dufu1991 for their contributions to this release.

---

## V0.18.0 Detached 2026-08-08

1. **把标签页拉成独立窗口**：命令面板或 Window 菜单里可以把当前标签页连同里面的分屏整个移到新窗口。
2. **窗口不再钻进菜单栏**：跨显示器拖动时标题栏不会再停在菜单栏后面导致抓不住，左右并排和上下叠放都一样。
3. **更新前先确认**：从菜单重启和通过 Homebrew 升级都会先征求确认。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.18.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.18.0/Kaku.dmg)

---

## V0.17.0 Linked 2026-08-02

1. **Responses API 与原生搜索**：兼容端点可选 `api_mode = "responses"`，并开启服务商托管的 web search，不必再配单独搜索 Key。
2. **自选托管 Shell**：`kaku init` 可明确安装 zsh 或 fish，选择会写入状态并在 XDG 路径间保持一致，不再只听 `$SHELL`。
3. **更完整的点击交互**：Cmd+Click 可打开 `github.com` 这类裸域名；文件链接可用 `config.file_link_editor` 指定编辑器；Option+Click 可在当前输入行内移动光标。
4. **更安全的 AI 工具**：路径、网络请求与代码搜索边界更严；流式被截断时不会再被当成成功回合。
5. **稳定性修复**：超大绘制批次不再导致启动崩溃，窗口不会拖进菜单栏后方，顶栏标签与红绿灯对齐，字体回退更安全，Shell 状态与拖选滚动也更稳。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.17.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.17.0/Kaku.dmg)

Special thanks to @ddotz and @F1Justin for their contributions to this release.

---

## V0.16.0 Trusted 2026-07-26

1. **内联 AI 走你的模型**：`#` 生成和自动修复现在与 Cmd+L 走同一条链路，Codex、Copilot 和 API key 三种配置都能直接用。
2. **控制消息需要认证**：Shell 发给终端的控制消息带上了本地凭据，终端里的输出无法再借用你配置的密钥触发助手请求。
3. **在标签导航里关标签**：选中标签按退格即可关闭，确认提示与别处一致。
4. **显示修复**：标题栏间距随显示器调整，AI 聊天会跟随系统外观切换，分屏后浮层尺寸正确，链接下划线在悬停时不再丢失。
5. **问题修复**：Starship 配置只在 Kaku 内生效，不再接管你的其他终端；白色背景上的文字保持可读；菜单命令改用随包分发的 CLI。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.16.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.16.0/Kaku.dmg)

Special thanks to @zwrong and @mortalYoung for their contributions to this release.

---

## V0.15.0 Connected 2026-07-18

1. **远程标签**：通过 ssh 连接的标签会显示专属图标和主机名，分屏里也能看出哪个窗格在远程，mosh、autossh、et 会话同样能识别。
2. **SSH 里的 AI**：AI 聊天现在能识别当前目录在远程主机上，不再把本地命令跑在远程路径里，而是基于终端上下文直接回答。
3. **SSH 一致体验**：fish 集成不再覆盖你自定义的 ssh 函数并支持 1Password 修复，带环境变量前缀的 ssh 别名恢复可用，mosh 也获得与 ssh 相同的 terminfo 回退。
4. **会话恢复**：恢复的 ssh 窗格会回到原来的远程目录，有窗口无法恢复时会说明数量并在下次启动时重试。
5. **问题修复**：Ctrl+L 清屏后选区高亮立即消失，Cmd+, 会聚焦已有设置窗口而不是不断新开，fish 用户的 AI 命令执行恢复正常。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.15.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.15.0/Kaku.dmg)

Special thanks to @shlroland, @mortalYoung, and @darion-yaphet for their contributions to this release.

---

## V0.14.0 Focused 2026-07-11

1. **分屏导航**：Tab Navigator 现在会列出标签内的每个分屏，可以直接跳到需要的分屏；标签很窄时也会优先保留当前分屏标题。
2. **标签重命名**：重命名窗口的文字与光标表现更清楚，按 Esc 或用鼠标取消后不再留下错误选区，自动生成的分屏标题也不会被意外覆盖。
3. **标签与选择**：标签分隔线更简洁，深色主题下的选区更清楚，双击选择也会正确区分中日韩文字与拉丁文字的边界。
4. **会话恢复**：重启后不会再恢复已经关闭的标签，恢复不完整时会保留原始恢复数据，SSH 会话也能再次从快照中恢复。
5. **安全与稳定性**：已解决 `anyhow` 与 `crossbeam-epoch` 的安全公告，分屏操作和鼠标释放在界面状态变化时也会更稳。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.14.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.14.0/Kaku.dmg)

---

## V0.13.0 Faster 2026-07-05

1. **启动与提示符**：Kaku 现在会在首帧前少做一些工作，并缓存 shell、配置和字体相关结果，新窗口和提示符响应会更快。
2. **默认提示符**：内置提示符现在会显示 Git 状态和 Node 版本，同时保留原本紧凑的终端观感。
3. **标签标题**：标签默认继续以路径为主，需要时可以打开 Command Tab Titles 来显示 `project·claude` 这类正在运行的工具，分屏标题也会保持可读。
4. **AI 聊天**：长对话流式输出时不再每次都重新高亮整段聊天，AI 面板在大段会话里会更顺。
5. **窗口与终端稳定性**：跨显示器缩放、半亮文本、终端缩小时的滚动历史，以及延迟配置初始化都处理得更稳。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.13.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.13.0/Kaku.dmg)

---

## V0.12.4 Safer 2026-06-30

1. **打开链接**：在 Claude、Codex、vim 这类会捕获鼠标的工具里，Cmd+Click 现在也能打开悬停的链接，各处行为一致。
2. **窗口平铺**：窗口现在支持 macOS 原生的拖到屏幕边缘平铺，会贴进系统的分屏布局。
3. **AI 文件编辑**：助手给超大文件打补丁时不再可能耗尽内存。
4. **依赖更新**：更新内置的 git 与文件处理库，修复三个已报告的安全隐患。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.4) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.4/Kaku.dmg)

---

## V0.12.3 Cleaner 2026-06-21

1. **系统代理**：开启系统代理时，访问本地和自托管模型的 AI 请求不再失败，私有地址和 loopback 会直连。
2. **终端重绘**：在 skills 选择器这类工具里上下滚动列表，不再残留错乱的行。
3. **凭证**：API key 和 token 不再通过命令行参数传递，不会出现在进程列表里。
4. **透明度**：开启窗口透明后，顶部条和圆角会与窗口其余部分一致，不再出现更深的色带或黑色角块。
5. **路径高亮**：命令行里已存在的目录不再带一条多余的下划线，悬停时也不会再叠出第二条。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.3) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.3/Kaku.dmg)

---

## V0.12.2 Steadier 2026-06-13

1. **休眠唤醒**：合盖休眠或拔掉外接显示器后，Kaku 不再停在旧画面上假死，渲染会立即恢复，正在跑的任务不受影响。
2. **外接屏**：在外接显示器上拖动窗口不再在起手时跳一下，屏幕缩放不同时 Window Center 和 `--position` 也会落在正确位置。
3. **选区**：在 Claude Code 这类启用鼠标的应用里，按住左键滚动不再误画出无法清除的选区高亮，普通单击会清掉残留的高亮。
4. **Yazi**：新版 Yazi 会拒绝旧版 Kaku 写入的配置导致无法启动，现在这些配置文件会被自动修复。
5. **窗口**：在全屏 Space 中新开窗口会立即铺满整屏，冷启动也不再闪现第二个空窗口。
6. **更新**：点击更新通知会先弹出确认，不会再因误点直接关闭所有窗口、打断正在运行的任务。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.2) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.2/Kaku.dmg)

---

## V0.12.1 Smoother 2026-06-08

1. **Shell 初始化**：从 0.12 升级时，zsh 设置过程不再出现 `[comment]: command not found` 报错。
2. **提示符颜色**：Starship、tmux、Powerline 和 box-drawing 分隔符会保留原本颜色，不再被提亮成突兀色块。
3. **SmartPrompt**：停在空闲 shell 时，Cmd+Q 不会再因为 `gitstatusd` 这类后台进程弹确认。
4. **滚动行为**：AI 长输出过程中滚动或调整窗口，不会再突然跳到历史最顶端，会继续跟住当前输出。
5. **Smart Tab**：新配置和 bundled 默认现在改为 Tab 优先接受灰色建议，没有建议时再回退到补全；想保留旧行为可设置 `smart_tab_mode = 'completion_first'`。
6. **Nightly 包**：Nightly 预览包现在是签名并公证过的 DMG，安装和验证最新修复时更接近正式版体验。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.1/Kaku.dmg)

---

## V0.12.0 Sharper ✂️ 2026-06-05

1. **会话历史恢复**：重新打开窗口后，会恢复每个面板最多 1,500 行滚动历史，列宽变化时也会自动重排。
2. **Codex 后端**：Kaku Assistant 现在可以直接复用你已有的 Codex / ChatGPT 登录，不用再单独配置一个 API key。
3. **AI 聊天**：聊天面板的回答实时流式输出，可以用 `/suggest` 预测你接下来想发的内容，每次也都能稳定重新打开。
4. **AI Shell**：`#` 生成命令新增了注入检测，工具访问会在任意目录拦截 `.env`、SSH 私钥等敏感文件。
5. **SmartPrompt**：所有面板都停在 shell 提示符时，Cmd+Q 会直接退出；仍有 agent 或编辑器在运行时，则会先询问一下。
6. **macOS 窗口**：浅深色切换会一次性刷新所有窗口，退出全屏更干净，标题栏单击也不再误触最大化。
7. **文档默认打开**：PDF、图片、音视频、压缩包、Office 文档现在都用系统默认 app 打开，不再被 VS Code 抢走。
8. **轻装**：新增 `smart_tab_mode`，移除简体中文本地化，zsh 注释高亮更清晰，并新增了多道 CI 检查。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.12.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.12.0/Kaku.dmg)

Special thanks to @t0m-car for the non-fancy tab bar fixes.

---

## V0.11.0 Steady 🧭 2026-05-17

1. **AI 推理**：DeepSeek 兼容、GLM、Kimi 和 Fireworks 的推理流会保持隐藏，最终回答更干净。
2. **Kaku Chat**：`kaku chat` 在隐藏推理时显示紧凑 thinking 状态，并避免记录空的 AI 回合。
3. **中文界面**：Kaku 现在内置简体中文，覆盖应用外壳、命令面板、设置和 AI 对话。
4. **Shell 初始化**：`kaku init` 对只读配置路径、已有跳转工具、Cmd+Backspace、Yazi 主题和 `#` AI 查询更稳。
5. **会话恢复**：新的恢复设置让窗口快照更容易理解，也更方便从配置里控制。
6. **窗口控制**：全屏最后一个 tab 下 Cmd+W 会正确关闭页面，标题栏拖动也减少误触发 snap 或最大化。
7. **鼠标与标签页**：顶部标签栏命中、集成标题按钮、标签拖拽动画和 scrollback 选择都更稳定。
8. **渲染细节**：条形光标、低 DPI toast 文本、彩色 emoji 尺寸和 pane 背景对齐都做了收紧。
9. **AI 传输**：流式输出、输入法组合、空 API key、代理处理和 shell 查询初始化在更多 provider 下更稳。
10. **维护工作**：Agent 指南、配置文档、release 检查和 contributor 元数据都同步到当前维护流程。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.11.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.11.0/Kaku.dmg)

Special thanks to @t0m-car for the low-DPI toast clipping fix.

---

## V0.10.0 Chat 🪄 2026-05-10

1. **AI 对话**：同一套引擎，两种入口：`Cmd+L` 在 Kaku 内打开对话面板，执行 `k` 或 `kaku chat` 把同一套引擎放进 alternate-screen TUI，任何终端或 SSH 远端都能用，流式 Markdown、语法高亮、shell 上下文、项目工具、网页搜索、本地记忆样样齐全，主题自动识别、取消和审批语义更稳，`#` 查询自动存入 shell 历史。
2. **AI 配置与安全**：Assistant 设置改为 Simple Model 与 Deep Model，支持在线模型加载、代理感知请求、OAuth 配置以及更多 provider 响应格式，shell 审批更严，敏感路径保护扩展到搜索类工具，文件写入与 patch 上限收紧，失败命令上下文和解析错误一并更稳。
3. **智能关闭保护**：`Cmd+W` 和 `Cmd+Shift+W` 在 pane 里跑着 claude、codex、cursor-agent、gemini、vim、cargo、npm 这类有状态进程时会先弹确认，bare shell 仍然直接关。
4. **窗口快照**：Kaku 自动保存多 tab、多 pane 布局，需要时按 `Cmd+Option+Shift+T`，或从 Shell → Restore Previous Window、命令面板恢复。
5. **AppleScript 字典**：Kaku 内置完整 AppleScript dictionary，窗口、tab、pane 都能被 Shortcuts、Hammerspoon 等自动化工具脚本化驱动。
6. **拖拽标签页动画**：拖动 tab 时相邻 tab 会平滑让位，不再生硬切换，重新排序变成一个连贯的动作。
7. **深色主题更柔和**：Kaku Dark 默认调色板降低了高亮色饱和度，前景文字微微调暗，长时间盯屏更不刺眼，整体还是 Kaku 的风格。
8. **冷启动与 Shell 速度**：通过 Lua 字节码缓存、字体与配置延迟初始化、shell 用户变量缓存，启动更轻。
9. **后台更新**：更新改为后台下载，checksum 不通过则失败关闭，代理与 MacPorts 检测一并更稳。
10. **问题修复**：修复全屏崩溃和卡住、显示器竞态、resize 缝隙、光标 reflow、链接、选择、浅色主题可读性和 TUI 复制。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.10.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.10.0/Kaku.dmg)

Special thanks to @s010s, @SherlockSalvatore, @darion-yaphet, @ddotz, @beautifulrem, @yxspace, and @fanweixiao for their contributions to this release.

---

## V0.9.0 Spark ✨ 2026-04-04

1. **自然语言生成命令**：输入 `# <描述>` 后按回车，Kaku 将生成的命令注入回提示符，确认后即可运行，并保存到 shell 历史，支持 zsh 和 fish。
2. **Option+Click 移动光标**：点击当前行任意位置即可将光标移动到该位置，正确处理宽字符和多字节输入。
3. **窗口置顶**：通过 Window 菜单将窗口固定在最前，随时可切换开关。
4. **Traffic Lights 位置**：设置中新增 `traffic_lights` 选项，可自定义 macOS 窗口控制按钮的位置。
5. **性能优化**：Tab 标题 Lua 回调改为批量处理，单次序列化 Config。
6. **稳定性修复**：修复 Option+Click 崩溃、分屏尺寸计算除零、鼠标事件 unwrap panic。
7. **Shell 与 Assistant**：内置新增 MiniMax provider preset；修复 zsh-z 更新显示和 heredoc 引号边界问题。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.9.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.9.0/Kaku.dmg)

Special thanks to @fanweixiao and @LanternCX for their contributions to this release.

---

## V0.8.0 Fish 🐟 2026-03-23

1. **Fish Shell 完整支持**：`kaku init` 现支持 fish shell 完整引导，含 Starship 提示符、Yazi 启动器、主题同步及 conf.d 入口。
2. **铃声标签指示器**：后台标签任务完成时显示铃声前缀，支持可选 Dock badge 和标签前缀开关。
3. **记住上次目录**：新标签和新窗口打开时自动恢复上次工作目录，可通过 `kaku config` 关闭。
4. **Update/Doctor 在标签中运行**：`kaku update` 和 `kaku doctor` 在独立标签中打开，不阻塞当前会话。
5. **仅显示目录名标签**：新增 `tab_title_basename_only` 选项，只显示目录名而非完整路径。
6. **滚动修复**：修复快速输出时 viewport 跳到顶部、往上滚动后自动跳回底部，以及 Claude Code 使用时 viewport 异常跳动的问题。
7. **Bug 修复**：修复窗口隐藏、Cmd+Click 链接、剪贴板粘贴、emoji 宽度、SSH alias 冲突。关闭确认弹窗现支持回车确认、Esc 取消。修复 macOS 26 上 Cmd+Q 崩溃和透明圆角渲染问题。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.8.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.8.0/Kaku.dmg)

Special thanks to @mystersu, @ddotz, @rookie-ricardo, @s010s, @anzksdk, @cynosurech, and @XinCao for their contributions to this release.

---

## V0.7.1 Flow 🌊 2026-03-13

1. **自动主题与界面优化**：Kaku 现在会跟随 macOS 自动切换深色和浅色模式，并优化了透明度渲染和 Yazi 主题同步体验。
2. **更安全的关闭与交互**：新增标签页和窗格关闭确认，重做了关闭浮层样式，并修复标题栏交互问题，双击缩放后也不会影响窗口拖拽。
3. **更好的设置工作流**：`kaku config` 现在提供更清晰的分组结构、固定底部快捷键提示、更稳健的配置解析和更可靠的配置重载。
4. **AI 配置增强**：`kaku ai` 现在支持 Antigravity 模型配置、额度追踪、后台加载和更可靠的 OAuth token 刷新。
5. **可选开启的圆角滚动条**：新增自制圆角滚动条，并可通过 `kaku config` 开启。
6. **窗格输入广播**：新增窗格输入广播模式，可在多个窗格间同步输入，同时避免将浮层输入误广播到其他窗格。
7. **文件与编辑器工作流优化**：改进文件路径链接打开，SSH 会话增加远程文件快捷入口，并在配置编辑流程中更好地尊重 `$EDITOR` 环境变量。
8. **Bug 修复与稳定性提升**：修复 AeroSpace/yabai 调整窗口大小时的闪烁问题，加固托管 shell 和 tmux 集成，Smart Tab 默认仅限 Kaku 会话，并改进 Starship RPROMPT 回退行为。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.7.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.7.1/Kaku.dmg)

Special thanks to @frankekn, @crossly, @iwen-conf, and @zxh326 for their contributions to this release.

---

## V0.6.0 Clarity ☀️ 2026-03-08

1. **浅色主题优化**：Kaku 这次重点打磨了浅色主题，支持动态字重，优化了 ANSI 配色，并补充了针对 Claude Code 的颜色覆盖，包括更自然的引用背景显示效果。
2. **AI 剩余用量显示**：AI 设置面板现在可以更直观地显示 usage 摘要和剩余额度，新增了 Kimi Code 支持、Kimi usage 统计、后台加载，并改进了 Claude OAuth token 刷新后的持久化逻辑。同时支持在配置文件中设置 `custom_headers`，方便私有化或代理部署场景使用。
3. **交互式设置 TUI**：`kaku config` 现在是更完整的交互式设置界面，支持退出即保存、主题感知更新，以及更稳定的配置同步体验。
4. **标签页工作流升级**：现在支持拖拽调整标签顺序、双击内联重命名、通过 `Cmd + Shift + T` 重新打开刚关闭的标签页，以及通过 `kaku set-tab-title` 命令从脚本或外部工具重命名标签页。
5. **路径超链接支持**：终端输出中的文件路径现在可以直接点击打开，同时改进了相对路径和解码后路径的处理。
6. **Shell 集成改进**：优化了无 Homebrew 场景下的首次 shell 初始化，修复 zsh source line 转义问题、Starship 在 `Ctrl + C` 后泄漏右提示符文本的问题，移除了强制 `TERM=kaku` 设置（该设置会导致 SSH 远端 Delete 键失效），并提升了 `TERM` 回退到 `xterm-256color` 时 AI shell hooks 的兼容性。
7. **选区与界面细节优化**：终端输出过程中不再破坏文本选区，修复陈旧 window padding 覆盖问题，优化全屏 inset 与 padding 对齐，并统一了不同显示器下的选区颜色表现。
8. **macOS 输入与窗口修复**：修复了非拉丁输入法阻塞 `Cmd + 字母/数字` 快捷键、`Cmd + Shift + Symbol` 组合键不响应、死键处理异常、土耳其键盘波浪号输入问题，并改进了全屏、最大化、显示器刷新和全局快捷键恢复等行为。
9. **终端、内存与通知体验优化**：新增未读 bell 的 Dock badge，Scrollback 改为懒加载并对背景图和渐变渲染缓存设置大小上限，长时间使用后内存占用更稳定，同时继续打磨更新菜单、命令执行和终端交互链路中的稳定性。
10. **发布与升级链路加固**：补强了自动发布校验，统一了配置迁移元数据，并将配置迁移版本提升到 `14`，确保老用户能正确收到这次 shell 集成刷新和 prompt helper 修复。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.6.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.6.0/Kaku.dmg)

Special thanks to @frankekn, @ddnio, @GavinRiver, and @nguyenphutrong for their contributions to this release. If Kaku helps you, please consider giving it a star and recommending it to your friends.

---

## V0.5.1 Kindness 🌴 2026-02-28

1. Fixed shell alias conflict, `y` launcher no longer clashes with existing aliases like `alias y=yarn`
2. Fixed SSH remote display, sessions now forward `TERM=xterm-256color` to prevent broken rendering on servers without Kaku's terminfo
3. Fixed `Cmd + Shift + ,` keybinding not passing through to tmux and other terminal apps
4. Fixed CLI panic when running `kaku cli split-pane`
5. Fixed misleading "error" toast appearing in AI assistant during analysis
6. Fixed update notification appearing on startup even when check\_for\_updates is disabled

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.5.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.5.1/Kaku.dmg)

---

## V0.5.0 Yohaku 🪽 2026-02-27

1. **AI Shell 错误修复**：命令失败时，Kaku 自动把命令发给 AI，直接在终端里展示修复建议，按 `Cmd + Shift + E` 一键应用。
2. **内置 Yazi**：按 `Cmd + Shift + Y` 或直接输入 `y` 打开 Yazi，布局和主题首次运行自动配好。`cd + Tab` 在文件系统无匹配时还会回退到 zsh-z 历史目录。
3. **命令面板**：按 `Cmd + Shift + P` 快速搜索命令，支持模糊匹配和原生文本编辑。
4. **Kaku Doctor**：运行 `kaku doctor` 检测配置问题，支持交互式一键修复。
5. **全局快捷键**：默认 `Ctrl + Opt + Cmd + K` 从任意应用唤起或隐藏 Kaku，支持通过 `macos_global_hotkey` 自定义。
6. **Shell 文本编辑**：在命令行中使用 `Cmd + A` 全选、`Shift + 方向键` 扩展选区、直接输入替换选中内容，和原生文本编辑器一样的体验。
7. **AI 配置统一入口**：`kaku ai` 界面现在统一管理 Kaku Assistant、Factory Droid 和 opencode.jsonc。
8. **标签页响铃提示**：切到其他标签时，有结果或通知的标签会显示闪烁圆点。
9. **滚动与输入优化**：`less`、`man` 等场景下触控板滚动更顺滑，支持 DECSET 1007 备用滚动模式。修复了 Typeless 等 CJK 输入法弹窗位置偏移的问题。
10. **窗口阴影恢复**：macOS 原生窗口阴影已恢复，此前因早期版本 GPU 占用问题被临时关闭。
11. **启动提速与包体积大幅优化**：Lua 字节码缓存和延迟加载缩短启动时间，Fat LTO 和单 codegen unit 减小二进制体积。
12. **Shell 集成检测修复**：Shell 集成现在在所有终端环境下都能正常加载，不再依赖 `$TERM` 是否为 kaku。用户自行加载的插件不再被重复加载。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.5.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.5.0/Kaku.dmg)

Special thanks to @jaost, @nguyenphutrong, @mickyyy68, and @guohai for their contributions to this release. If Kaku helps you, please consider giving it a star and recommending it to your friends.

---

## V0.4.0 AIIIIIII 🥂 2026-02-19

1. **新增 `kaku ai` 命令**：提供统一入口管理当前所有 AI Coding 工具配置，优化编辑交互、账号展示和 OAuth 重认证流程。
2. **优化 macOS 启动与渲染体验**：默认使用 WebGpu，失败时自动回退 OpenGL，渲染效果更好；典型场景内存从约 200MB 降到约 80MB。
3. **内置 Lazygit 工作流**：新增 `Cmd + Shift + G` 快捷键、终端内启动流程和 Git 仓库场景提示。
4. **分屏体验增强**：新增当前分屏标记，一眼看出当前分屏；支持 `split_thickness` 配置分割线粗细，优化分割线交叉对齐。
5. **窗口与分屏操作优化**：`Cmd + W` 在分屏、标签页和多窗口场景下关闭逻辑更符合直觉；新增 `Cmd + Opt + Arrows` 快速在分屏间切换。
6. **Shell 安装与更新优化**：重构首次安装和更新流程，支持可选 Homebrew CLI 工具安装，并优化 Tab 补全策略。
7. **SSH 兼容性和 1Password 支持**：远端 SSH 会强制 `TERM=xterm-256color` 避免 terminfo 问题。Shell 集成自动识别 1Password SSH Agent，自动加上 `IdentitiesOnly=yes`，解决密钥太多导致的认证失败。
8. **分屏独立编码支持**：新增每个 Pane 独立切换编码，支持 `UTF-8`、`GBK`、`GB18030`、`Big5`、`EUC-KR`、`Shift-JIS`。
9. **URL Scheme 激活指定终端**：新增 `kaku://open-tab?tty=<device>`，外部工具、脚本或 AI agent 可以直接通过 TTY 名跳转到对应的终端 Pane。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.4.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.4.0/Kaku.dmg)

Thanks to @WongLoki, @liu-kan, @OiAnthony, @XueshiQiao, and @CTHua for the contributions.

---

## V0.3.1 New Year 🎋 2026-02-16

1. **Cmd+K 清屏**：使用 `Cmd+K` 清屏，同时保留 `Cmd+R` 兼容。
2. **切换分屏方向**：按 `Cmd+Shift+S` 可在水平和垂直布局之间切换分屏方向，正确处理缩放状态和最小尺寸。
3. **SSH 兼容性修复**：SSH 会话现在统一使用 `xterm-256color` 作为 TERM，包括内置 SSH Domain 和在终端中直接运行 `ssh` 命令两种场景，远端不再因缺少 `kaku` terminfo 而报错。
4. **macOS 听写支持**：修复 `selected_range` 返回合法光标位置，macOS 听写和语音输入现在可以正常工作。
5. **Tab 补全恢复**：移除 Smart Tab 行为，Tab 键恢复为显示补全列表，使用右箭头键接受自动建议。
6. **自动创建用户配置**：`kaku init` 在配置文件不存在时自动创建 `~/.config/kaku/kaku.lua`。
7. **OpenCode 主题配置**：`kaku init` 可选配置与 Kaku 匹配的 OpenCode 主题，写入前会询问用户确认。
8. **蓝牙权限声明**：在 Info.plist 中添加蓝牙权限描述，支持在终端中进行 BLE 开发。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.3.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.3.1/Kaku.dmg)

Thanks to @GavinRiver for the contribution. If you find Kaku helpful, feel free to star the project and share it with your friends.

---

## V0.3.0 Happy 🥙 2026-02-16

1. **全屏体验优化**：切换动画更稳定，减少闪烁，全屏下分屏或新建标签时边距保持正确，同时优化了全屏下 Tab Bar 显示逻辑和分屏分割线渲染。
2. **窗口恢复稳定性**：移动/缩放时会持久化窗口尺寸与位置，并按显示器状态恢复，多屏场景更稳一点。
3. **Toast 通知**：复制文本显示 `Copied!`，重载配置有确认提示，操作结果看明白。
4. **SSH 主机名标签**：连接远程服务器时 Tab 栏显示主机名，多服务器管理不再混淆。
5. **Finder 右键打开**：右键文件夹选择 `Open in Kaku`，直接在该目录打开终端。启动阶段会先排队请求，GUI 就绪后再处理。
6. **设为默认终端**：通过菜单栏 `Kaku` → `Set as Default Terminal` 设为系统默认，shell 脚本及终端相关文件类型将用 Kaku 打开而非 Terminal.app。
7. **Shell 历史滚动**：在 vim 或 tmux 中向上滚动查看 shell 历史，向下返回，无需退出当前应用。
8. **图片粘贴支持**：从其他应用复制图片，粘贴到 Kaku 的终端应用中，自动保存并插入路径。
9. **选中自动滚动**：拖拽选中文本时向上超出视口，视图会自动滚动以便选择上方更多内容。
10. **透明窗口修复**：修复窗口透明时的边缘缝隙，用背景色正确填充。
11. **Dock 批量拖放**：拖拽多个文件到 Dock 图标，每个文件在单独标签页打开。
12. **字体渲染修复**：修复 macOS 字体压缩和宽字符挤压问题，图标字体正常显示，对低分辨率使用更小字体。
13. **竖线光标**：默认改为竖线光标，更符合现代终端习惯，支持 Vim 模式光标切换，恢复闪烁行为，优化了非活动 Pane 的光标问题。
14. **分屏标签清晰化**：菜单改为 `Split Pane Top/Bottom` 和 `Split Pane Left/Right`，表述更清晰。
15. **`kaku update` 更安全**：修复 Homebrew 误升级错误软件的问题，并仅探测 caskroom 管理的安装。
16. **color\_scheme 覆盖修复**：用户配置的 color\_scheme 现在可以正确覆盖内置 `Kaku` 主题。
17. **降低 GPU 占用**：默认关闭窗口阴影以减少 macOS 下的 GPU 开销，这样窗口显示效果更简洁好看。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.3.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.3.0/Kaku.dmg)

Thanks to @mickyyy68, @Viking602, @GavinRiver, @QuentinHsu for their contributions. If you find Kaku helpful, feel free to star the project and share it with your friends.

---

## V0.2.0 Craft 🍺 2026-02-13

1. **通用安装包**：支持 Apple Silicon 和 Intel，无需区分架构下载。深度优化 Rust 打包，体积更小、性能更好。
2. **Apple 公证**：3年申请一直没通过，这次直接给库克写信投诉，终于解决了！698元也花出去了，就为了让大伙使用没有安全警告，开箱即用。
3. **修复配置加载**：`~/.config/kaku/kaku.lua` 用户配置现在正确加载，不再被默认配置覆盖。
4. **Homebrew 安装支持**：`brew install tw93/tap/kakuku` 一键安装，`brew upgrade kakuku` 轻松更新。
5. **全屏时间右下角显示时间**：即使你沉浸在 AI 编程的心流中，也要记得关注时间、按时休息，身体永远是第一位的，并优化全屏切换动画。
6. **更智能的窗口控制**：双击标题栏或标签区域快速缩放，改进窗口恢复，优化标题栏拖拽防止误选中文本滚动，分屏间距可配置，对齐更精准。
7. **统一命令行工具**：新增 `kaku` 命令，支持 `init`、`update`、`reset`、`config` 等快捷操作。
8. **Git Delta 优化**：主题更统一，默认并排显示 diff，头部信息更简洁，代码审查体验更佳。
9. **中文路径支持**：Tab 标签页标题正确显示中文路径，不再出现 URL 编码。
10. **会话保持**：`Cmd+W` 关闭当前分屏或标签页，仅剩一个时隐藏窗口而非退出，保留终端会话。
11. **体验优化**：字体缩放和窗口大小自动记忆，重启后依然生效。Tab 智能补全优先匹配文件系统，减少历史干扰。
12. **视觉打磨**：修复分屏对齐偏差及下划线溢出，消除标签切换卡顿，升级 Unicode 至 v14，Emoji 兼容性更好。
13. **菜单栏优化**：集成命令面板、设置、检查更新及系统通知。
14. **内置更新**：自动更新提醒，点击菜单栏 Kaku → Check for Updates，或在终端运行 `kaku update` 一键升级。

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.2.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.2.0/Kaku.dmg)

Since the changes in this version are pretty crazy, please let me know if you run into any problems. Thanks to @yikZero, @iwen-conf, @GavinRiver, @dongowu, @yamamel for their contributions. If you find Kaku helpful, feel free to star this project.

---

## V0.1.1 Easy to use 🍤 2026-02-09

1. **Crisp Font Rendering**: Optimized macOS font rasterization settings (enabled Light Hinting) for clearer, sharper text, especially on Retina displays.
2. **Kaku Theme**: Introducing the signature Kaku Theme, a high-contrast, AI-optimized dark theme designed for long coding sessions with Claude/Codex.
3. **Enhanced Onboarding**: The first-run wizard now offers to apply the Kaku Theme automatically, giving you a perfect environment instantly.
4. **Zero Config Polish**: Improved default settings and streamlined the initial setup process for an even smoother out-of-the-box experience.
5. **Debug Overlay**: Added a new debug overlay for easier troubleshooting of shell integration and configuration issues.
6. **Shell Integration**: Launched a robust `setup_zsh.sh` script with common aliases and git shortcuts to supercharge your terminal workflow.
7. **Tab Bar Improvements**: Custom tab bar now supports path-based titles and visual indicators; fixed runtime errors with `get_current_working_dir`.
8. **Under the Hood**: Automated version synchronization and various internal improvements for stability. Addressed macOS permission prompts for a smoother experience. Renamed internal project references to `kaku` for consistent branding.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.1.1) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.1.1/Kaku.dmg)

If macOS blocks the app, go to System Settings → Privacy & Security → click Open Anyway.

---

## V0.1.0 Freshmen 🧝‍♀️ 2026-02-08

1. First Release: Kaku is now live! A fast, out-of-the-box terminal built for AI coding, deeply customized for macOS with GPU acceleration.
2. Built-in Shell Suite: Comes with pre-configured Starship prompt, z smart directory jumper, Syntax Highlighting, Autosuggestions.
3. Smart Onboarding: Features an intelligent first-run wizard that detects your environment, safely backs up existing configs.
4. Native Experience: Optimized for macOS with smooth animations, intuitive shortcuts, split pane management, and focus mode.
5. Universal Binary: Fully optimized support for both Apple Silicon and Intel Macs in a single lightweight DMG.

[GitHub](https://github.com/tw93/Kaku/releases/tag/V0.1.0) · [DMG](https://github.com/tw93/Kaku/releases/download/V0.1.0/Kaku.dmg)

If macOS blocks the app, go to System Settings → Privacy & Security → click "Open Anyway".

---

Source: https://kaku.fun/zh/changelog
Site index for LLMs: https://kaku.fun/llms.txt
