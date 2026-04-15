---
title: 更新日志
description: Kaku 的版本发展脉络
---

完整、可点击的版本说明请看 [GitHub Releases](https://github.com/tw93/Kaku/releases)。下面按时间倒序梳理每个版本的关键变化，帮助你快速了解 Kaku 的发展脉络。

## V0.9.0 Spark ✨ — 2026-04-04

让 AI 更贴近命令行的一次更新。

- **自然语言转命令**：提示符前输入 `# <描述>` 回车，Kaku 调 LLM 生成命令并回填到提示符，保存到 shell 历史，zsh / fish 均支持
- **Option + Click 移动光标**：点击当前行任意位置即可把光标移过去，正确处理宽字符和多字节输入
- **窗口置顶**：在 Window 菜单把任意窗口固定在最前
- **Traffic Lights 位置可配**：设置中新增 `traffic_lights` 选项定制 macOS 窗口按钮位置
- **Provider**：内置新增 MiniMax 预设
- **稳定性**：修复 Option+Click 崩溃、分屏尺寸除零、鼠标事件 unwrap panic

## V0.8.0 Fish 🐟 — 2026-03-23

正式拥抱 fish shell 用户。

- **Fish shell 完整支持**：`kaku init` 为 fish 安装 Starship、Yazi 启动器、主题同步和 conf.d 入口
- **铃声标签指示**：后台标签任务完成显示铃声前缀，支持 Dock badge
- **记住上次目录**：新窗口 / 新标签恢复上次工作目录，可在 `kaku config` 关闭
- **Update / Doctor 独立标签**：`kaku update`、`kaku doctor` 在独立标签运行，不阻塞当前会话
- **标签仅显示目录名**：新增 `tab_title_basename_only` 选项
- **滚动修复**：修复快速输出时 viewport 跳到顶部、Claude Code 使用时异常跳动

## V0.7.1 Flow 🌊 — 2026-03-13

主题、设置和 AI 工作流一起打磨。

- **自动主题切换**：跟随 macOS 明暗模式,优化透明度渲染与 Yazi 主题同步
- **更安全的关闭与交互**：标签和 pane 关闭确认、重做浮层样式、修复标题栏双击缩放干扰拖拽
- **`kaku config` 再升级**：分组更清晰、底部固定快捷键提示、配置解析和重载更稳
- **AI 配置**：`kaku ai` 支持 Antigravity 模型、额度追踪、后台加载、更可靠的 OAuth 刷新
- **Pane 输入广播**：在多个 pane 间同步输入,避免浮层输入被误广播
- **文件与编辑器**：改进文件链接打开、新增 SSH 远程文件快捷入口、尊重 `$EDITOR` 环境变量
- **圆角滚动条**：可在 `kaku config` 开启

## V0.6.0 Clarity ☀️ — 2026-03-08

浅色主题、AI 额度显示和交互式设置一把梭。

- **浅色主题**：动态字重、优化 ANSI 配色、补充 Claude Code 颜色覆盖
- **AI 用量可视化**：AI 面板更清晰显示 usage 摘要与剩余额度，新增 Kimi Code 支持和 Kimi usage 统计
- **交互式设置 TUI**：`kaku config` 变成完整的交互式设置，退出即保存、主题感知
- **标签工作流**：拖拽排序、双击重命名、`Cmd + Shift + T` 恢复关闭标签、`kaku set-tab-title` 命令
- **路径超链接**：终端输出的文件路径可直接点开
- **Shell 集成打磨**：优化无 Homebrew 首装、修复 Starship 右提示符泄漏、移除强制 `TERM=kaku`
- **macOS 输入修复**：非拉丁 IME 不再阻塞 `Cmd + 字母/数字` 快捷键；修复死键、土耳其键盘波浪号
- **内存优化**：scrollback 懒加载、背景图和渐变缓存设上限，长会话内存更稳

## V0.5.1 Kindness 🌴 — 2026-02-28

V0.5.0 后续修复。

- `y` 启动器不再与 `alias y=yarn` 冲突
- SSH 会话强制 `TERM=xterm-256color`，远端无 `kaku` terminfo 不再报错
- 修复 `Cmd + Shift + ,` 不透传到 tmux
- 修复 `kaku cli split-pane` panic、AI 分析时误报错误 toast、关闭自动更新后仍弹出的提示

## V0.5.0 Yohaku 🪽 — 2026-02-27

AI 时代正式开启。

- **AI Shell 错误修复**：命令失败自动发给 AI，终端内展示修复建议，`Cmd + Shift + E` 一键应用
- **内置 Yazi**：`Cmd + Shift + Y` 或输入 `y` 打开，布局和主题首次运行自动配好
- **命令面板**：`Cmd + Shift + P` 模糊搜索命令，原生文本编辑
- **Kaku Doctor**：`kaku doctor` 交互式检测并修复配置问题
- **全局快捷键**：`Ctrl + Opt + Cmd + K` 全局唤起 / 隐藏 Kaku
- **Shell 文本编辑**：命令行支持 `Cmd + A` 全选、`Shift + 方向键` 扩展选区
- **AI 配置统一入口**：`kaku ai` 统一管理 Kaku Assistant、Factory Droid、opencode.jsonc
- **启动提速**：Lua 字节码缓存、延迟加载、Fat LTO

## V0.4.0 AIIIIIII 🥂 — 2026-02-19

迈进 AI 的第一步 + 渲染管线大改造。

- **`kaku ai` 命令**：统一入口管理当前所有 AI Coding 工具配置
- **WebGpu 渲染默认开启**：典型场景内存从约 200 MB 降到约 80 MB，失败自动回退 OpenGL
- **内置 Lazygit**：`Cmd + Shift + G` 一键打开，Git 仓库场景提示
- **分屏体验**：当前分屏标记、`split_thickness` 配置、`Cmd + Opt + 方向键` 切换
- **`Cmd + W` 更聪明**：多 pane / 多标签 / 多窗口场景下符合直觉
- **SSH + 1Password**：远端强制 `TERM=xterm-256color`，自动识别 1Password SSH agent 并加 `IdentitiesOnly=yes`
- **分屏独立编码**：每个 pane 独立切换 UTF-8 / GBK / GB18030 / Big5 / EUC-KR / Shift-JIS
- **URL Scheme**：`kaku://open-tab?tty=<device>` 让外部脚本跳转到指定 pane

## V0.3.1 New Year 🎋 — 2026-02-16

- `Cmd + K` 清屏（保留 `Cmd + R` 兼容）
- `Cmd + Shift + S` 切换分屏方向
- SSH 会话统一用 `xterm-256color`，解决远端 terminfo 缺失
- 修复 macOS 听写 / 语音输入
- Tab 键恢复显示补全列表，右箭头接受自动建议
- `kaku init` 自动创建 `~/.config/kaku/kaku.lua`

## V0.3.0 Happy 🥙 — 2026-02-16

系统集成全面铺开。

- **全屏更顺滑**：过渡动画、分屏和标签切换更稳，Tab Bar 显示逻辑优化
- **SSH 主机名标签**：连接远程服务器时标签显示主机名
- **Finder 集成**：右键文件夹 `Open in Kaku`
- **设为系统默认终端**：菜单栏 Kaku → Set as Default Terminal
- **Shell 历史滚动**：vim / tmux 内向上滚动查看 shell 历史，向下返回
- **图片粘贴**：从其他应用复制图片直接粘贴到终端，自动保存并插入路径
- **选中自动滚动**：拖选超出视口时视图自动滚动
- **Toast 通知**：复制 / 重载配置有视觉反馈
- **Dock 批量拖放**：多个文件拖到 Dock 图标各开一个标签
- **竖线光标**：支持 Vim 模式切换，默认关闭窗口阴影降低 GPU 占用

## V0.2.0 Craft 🍺 — 2026-02-13

从"能跑"走向"装得下、用得爽"。

- **Apple 公证通过**：从此没有安全警告，开箱即用
- **Universal Binary**：Apple Silicon 和 Intel 通吃，单个 DMG
- **Homebrew 支持**：`brew install tw93/tap/kakuku`
- **统一 CLI 工具**：新增 `kaku` 命令，支持 `init` / `update` / `reset` / `config`
- **修复用户配置加载**：`~/.config/kaku/kaku.lua` 不再被默认覆盖
- **全屏时间显示**：全屏右下角显示时间，提醒按时休息
- **Git Delta 优化**：主题统一，默认并排 diff
- **中文路径支持**：标签标题正确显示中文
- **会话保持**：`Cmd + W` 只剩一个标签时隐藏窗口而非退出
- **字体缩放 / 窗口大小持久化**
- **菜单栏优化**：命令面板、设置、检查更新、系统通知
- **内置更新器**：`kaku update` 或菜单栏一键升级

## V0.1.1 Easy to use 🍤 — 2026-02-09

- **Kaku Theme**：专为 Claude / Codex 长时间编程调优的高对比度暗色主题
- 优化 macOS 字体栅格化（开启 Light Hinting），Retina 下更清晰
- 首次启动向导可一键应用 Kaku Theme
- 新增调试浮层，排查 shell 集成和配置问题更方便
- `setup_zsh.sh` 脚本带 alias 和 git 快捷方式
- Tab bar 支持路径标题和视觉指示

## V0.1.0 Freshmen 🧝‍♀️ — 2026-02-08

Kaku 首发。基于 [WezTerm](https://github.com/wez/wezterm) 深度改造，为 AI Coding 场景服务。

- GPU 加速，针对 macOS 深度优化
- 内建 shell 套件：Starship 提示符、z 智能目录跳转、zsh 语法高亮 / 自动建议
- 智能首次启动向导，自动检测环境、安全备份已有配置
- 原生动画、直觉快捷键、分屏管理和专注模式
- Universal Binary（Apple Silicon + Intel），单个轻量 DMG
