# 快捷键

窗口、标签、分屏、shell 编辑、AI 功能、Lazygit 和 Yazi 的默认快捷键。

所有快捷键都用 macOS 原生的修饰键。`Opt` = Option/Alt，`Ctrl` = Control。

## 窗口

| 操作 | 快捷键 |
| --- | --- |
| 新建窗口 | `Cmd + N` |
| 关闭分屏 / 标签 / 隐藏 | `Cmd + W` |
| 关闭当前标签 | `Cmd + Shift + W` |
| 隐藏应用 | `Cmd + H` |
| 最小化窗口 | `Cmd + M` |
| 切换全屏 | `Cmd + Ctrl + F` |
| 退出 | `Cmd + Q` |
| 切换全局窗口 | `Cmd + Opt + Ctrl + K` |

> `Cmd + W` 很聪明：有多个分屏时关闭当前分屏，有多个标签或窗口时关闭标签，否则隐藏应用。

## 标签

| 操作 | 快捷键 |
| --- | --- |
| 新建标签 | `Cmd + T` |
| 切换到标签 1–9 | `Cmd + 1` – `Cmd + 9` |
| 上一个标签 | `Cmd + Shift + [` |
| 下一个标签 | `Cmd + Shift + ]` |
| 打开 Tab Navigator | `Cmd + Shift + O` |
| 在 Tab Navigator 里关闭选中的标签 | 筛选框为空时按 `Backspace` |
| 关闭标签 | `Cmd + Shift + W` |
| 重开关闭的标签 | `Cmd + Shift + T` |
| 重命名标签 | 双击标签标题 |

## 分屏

| 操作 | 快捷键 |
| --- | --- |
| 垂直分屏 | `Cmd + D` |
| 水平分屏 | `Cmd + Shift + D` |
| 切换分屏方向 | `Cmd + Shift + S` |
| 放大 / 还原分屏 | `Cmd + Shift + Enter` |
| 在分屏间切换 | `Cmd + Opt + Arrows` |
| 调整分屏大小 | `Cmd + Ctrl + Arrows` |

## Shell 编辑

| 操作 | 快捷键 |
| --- | --- |
| 按词左移 / 右移 | `Opt + Left` / `Opt + Right` |
| 跳到行首 / 行尾 | `Cmd + Left` / `Cmd + Right` |
| 删到行首 | `Cmd + Backspace` |
| 删除一个词 | `Opt + Backspace` |
| 换行但不执行 | `Cmd + Enter` 或 `Shift + Enter` |

## 字号

| 操作 | 快捷键 |
| --- | --- |
| 放大 | `Cmd + =` |
| 缩小 | `Cmd + -` |
| 重置 | `Cmd + 0` |

## Kaku 功能

| 操作 | 快捷键 |
| --- | --- |
| 清屏 + 清回滚缓冲 | `Cmd + K` |
| 打开设置面板 | `Cmd + ,`（在面板里输入可过滤模型列表） |
| 打开 Command Palette | `Cmd + Shift + P` |
| 打开 AI 面板 | `Cmd + Shift + A` |
| 打开 AI Chat | `Cmd + L` |
| 应用 Kaku Assistant 建议 | `Cmd + Shift + E` |
| 恢复上次窗口快照 | `Cmd + Opt + Shift + T` |
| 打开 lazygit | `Cmd + Shift + G` |
| 打开 yazi 文件管理器 | `Cmd + Shift + Y` |
| 浏览远端文件（SSH） | `Cmd + Shift + R` |
| 打开 Doctor 面板 | `Ctrl + Shift + L` |

## 鼠标

| 操作 | 触发方式 |
| --- | --- |
| 复制选区到剪贴板 | 选中后松开鼠标左键 |
| 打开分屏右键菜单 | 右键 |
| 打开链接 | `Cmd + Click` |
| 把光标移到点击的列 | `Opt + Click`（同一行，仅 shell 提示符） |

## 自定义快捷键

往 `~/.config/kaku/kaku.lua` 里**追加**绑定到 `config.keys`。不要赋一个新表，那会清掉 Kaku 的默认值。

```lua
-- ~/.config/kaku/kaku.lua (after loading bundled config)
table.insert(config.keys, {
  key = 'RightArrow',
  mods = 'CMD|SHIFT',
  action = wezterm.action.ActivatePaneDirection('Right'),
})

-- Example: rebind AI Chat to Cmd+Shift+Space (original default):
table.insert(config.keys, {
  key = 'Space',
  mods = 'CMD|SHIFT',
  action = wezterm.action.EmitEvent('kaku-ai-chat'),
})
```

可用动作的完整列表见 [WezTerm KeyAssignment 参考](https://wezfurlong.org/wezterm/config/lua/keyassignment/)。

---

Source: https://kaku.fun/zh/docs/keybindings
Site index for LLMs: https://kaku.fun/llms.txt
