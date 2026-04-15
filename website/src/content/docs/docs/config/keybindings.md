---
title: 快捷键
description: Kaku 完整快捷键速查
---

# 快捷键

所有快捷键都使用 macOS 原生修饰键。`Opt` = Option/Alt，`Ctrl` = Control。

## 窗口

| 操作 | 快捷键 |
| :--- | :--- |
| 新建窗口 | `Cmd + N` |
| 关闭 pane / 标签 / 隐藏 | `Cmd + W` |
| 关闭当前标签 | `Cmd + Shift + W` |
| 隐藏应用 | `Cmd + H` |
| 最小化窗口 | `Cmd + M` |
| 切换全屏 | `Cmd + Ctrl + F` |
| 退出 | `Cmd + Q` |
| 切换全局窗口 | `Cmd + Opt + Ctrl + K` |

> `Cmd + W` 是智能的：有多个 pane 时关闭当前 pane，有多个标签或窗口时关闭当前标签，否则隐藏应用。

## 标签

| 操作 | 快捷键 |
| :--- | :--- |
| 新建标签 | `Cmd + T` |
| 切换到第 1–9 个标签 | `Cmd + 1` – `Cmd + 9` |
| 上一个标签 | `Cmd + Shift + [` |
| 下一个标签 | `Cmd + Shift + ]` |
| 关闭标签 | `Cmd + Shift + W` |
| 恢复关闭的标签 | `Cmd + Shift + T` |
| 重命名标签 | 双击标签标题 |

## 分屏

| 操作 | 快捷键 |
| :--- | :--- |
| 垂直分屏 | `Cmd + D` |
| 水平分屏 | `Cmd + Shift + D` |
| 切换分屏方向 | `Cmd + Shift + S` |
| 放大 / 还原 pane | `Cmd + Shift + Enter` |
| 在 pane 间导航 | `Cmd + Opt + 方向键` |
| 调整 pane 大小 | `Cmd + Ctrl + 方向键` |
| 广播输入到当前标签 | `Cmd + Opt + I` |
| 广播输入到所有标签 | `Cmd + Shift + I` |

## Shell 编辑

| 操作 | 快捷键 |
| :--- | :--- |
| 按单词向左 / 向右跳转 | `Opt + Left` / `Opt + Right` |
| 跳到行首 / 行尾 | `Cmd + Left` / `Cmd + Right` |
| 删除到行首 | `Cmd + Backspace` |
| 删除单词 | `Opt + Backspace` |
| 换行但不执行 | `Cmd + Enter` 或 `Shift + Enter` |

## 字号

| 操作 | 快捷键 |
| :--- | :--- |
| 放大 | `Cmd + =` |
| 缩小 | `Cmd + -` |
| 重置 | `Cmd + 0` |

## Kaku 特色功能

| 操作 | 快捷键 |
| :--- | :--- |
| 清屏并清空回滚缓冲 | `Cmd + K` |
| 打开设置面板 | `Cmd + ,` |
| 打开 AI 面板 | `Cmd + Shift + A` |
| 应用 Kaku Assistant 建议 | `Cmd + Shift + E` |
| 打开 lazygit | `Cmd + Shift + G` |
| 打开 yazi 文件管理器 | `Cmd + Shift + Y` |
| 浏览远程文件（SSH） | `Cmd + Shift + R` |
| 打开 Doctor 面板 | `Ctrl + Shift + L` |

## 鼠标

| 操作 | 触发方式 |
| :--- | :--- |
| 复制选中内容到剪贴板 | 松开鼠标左键完成选择 |
| 打开链接 | `Cmd + 点击` |
| 将光标移动到点击的列 | `Opt + 点击`（仅限同一行 shell 提示符内） |

## 自定义快捷键

在 `~/.config/kaku/kaku.lua` 中通过**追加**的方式向 `config.keys` 添加绑定。不要直接赋值一个新表，否则会覆盖 Kaku 的默认快捷键。

```lua
-- ~/.config/kaku/kaku.lua（在加载内建配置之后）
table.insert(config.keys, {
  key = 'RightArrow',
  mods = 'CMD|SHIFT',
  action = wezterm.action.ActivatePaneDirection('Right'),
})
```

完整的 action 列表参见 [WezTerm KeyAssignment 文档](https://wezfurlong.org/wezterm/config/lua/keyassignment/)。
