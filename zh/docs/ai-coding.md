# 在 Kaku 里用 Claude Code 和 Codex

Kaku 和你已经在用的 AI 编程工具怎么配合。

## 配置工具

运行 `kaku ai` 会打开 AI 设置面板，可以配置 Claude Code、Codex、Gemini CLI、Copilot CLI、Kimi Code 这些外部编程工具，看它们的套餐用量，也可以设置 Kaku Assistant。只有在这里修改时 Kaku 才会改对应工具的配置，你自己写的字段会保留，Kaku 不会主动往 agent 里加 hook 或指令。

```bash
kaku ai
```

## 通知

Claude Code 2.1.295 及以后会告诉 Kaku 自己是在干活、在等你还是已经跑完，Kaku 0.25.0 及以上不用任何设置就会提醒你，`preferredNotifChannel` 保持默认的 `auto` 就好，改成 `ghostty` 会让 Claude Code 再发一条重复的通知。Claude Code 更旧或者 Kaku 早于 0.25.0 时，`auto` 通道在这里什么都不会发，在 `~/.claude/settings.json` 里这样设置：

```json
{ "preferredNotifChannel": "ghostty" }
```

Codex 不用改，它会响终端铃声，Kaku 把铃声转成通知。想看到 Codex 自己的消息而不是一条通用提醒，在 `~/.codex/config.toml` 里加上：

```toml
[tui]
notification_method = "osc9"
```

通知只在 Kaku 处于后台时出现，正在看的那个窗格不会弹。如果一条都收不到，打开 Kaku > Notification Settings... 把 Allow Notifications 打开。

为什么 Claude Code 和 Codex 在大多数别的终端里不发通知，每种情况该改哪个设置，写在 [Claude Code 通知不弹出](https://kaku.fun/zh/blog/claude-code-notifications)那篇。

## 写提示词

`Shift + Enter` 和 `Cmd + Enter` 都是换行，长提示词可以分几行写完再发。

工具打印出来的链接和文件路径可以 `Cmd + Click` 打开，即使它正占用着鼠标也可以，带位置的路径比如 `src/main.rs:42:7` 会直接跳到那一行。想指定编辑器就设置 `file_link_editor`：

```lua
config.file_link_editor = "zed"
-- config.file_link_editor = "cursor --goto"
```

别的终端在 Claude Code 里换行、通知和点开路径做得怎么样，[Claude Code 用什么终端最好](https://kaku.fun/zh/blog/best-terminal-for-claude-code)那篇做了对比。

## 几个 agent 并排跑

用 `Cmd + D` 或 `Cmd + Shift + D` 分屏，一边跑 agent 一边留一个 shell，或者让两个 agent 各做一件事，想细看哪个窗格就按 `Cmd + Shift + Enter` 放大。前台正在跑 Claude Code、Codex 或其他长时间运行程序的窗格和标签，关闭前会先确认，手滑按了 `Cmd + W` 也不会把会话关掉。

## 重启之后

Kaku 重启后，比如装完更新，标签和分屏都会回来，之前在跑 Claude Code 或 Codex 的窗格还会保留最后一屏，接着聊之前能先看到每个对话停在哪里。

## 从另一台电脑用

运行 `kaku remote`，可以在另一台电脑或手机的浏览器里使用这些终端，正在跑的 agent 也在其中，配对和命令见[命令参考](https://kaku.fun/zh/docs/cli#cli-kaku-remote)。

## Kaku Assistant

Kaku 自己也带一个助手，在 shell 里问点小问题、修一修命令很方便，它是可选的，用的是你配置的 AI 服务，也可以直接复用 Codex 的登录，详见[功能特性](https://kaku.fun/zh/docs/features#features-kaku-assistant)。

---

Source: https://kaku.fun/zh/docs/ai-coding
Site index for LLMs: https://kaku.fun/llms.txt
