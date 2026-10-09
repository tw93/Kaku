# 上手指南

Kaku 日常怎么用，从第一个窗口一直讲到 AI 助手。

> 完整的选项分别在[功能特性](https://kaku.fun/zh/docs/features)、[快捷键](https://kaku.fun/zh/docs/keybindings)和[配置](https://kaku.fun/zh/docs/configuration)这几页。

![Kaku 终端，显示标签栏、分屏和 shell 提示符](https://kaku.fun/shots/kaku-dark.webp)标签在底部，分屏左右并排。

## 第一次打开

打开 Kaku 就是一个窗口加一个 shell 提示符，没有设置向导，也不用注册账号。

- **默认深色。**新安装用的是 Kaku Dark，在设置里可以换成 Kaku Light，或者选 Auto 跟着系统切换。
- **没有标题栏。**关闭、最小化、缩放三个按钮就在窗口左上角。
- **标签栏平时不占地方。**只有一个标签时它是藏起来的，开第二个标签时才出来。

如果 shell 找不到 `kaku` 命令，运行 `kaku doctor` 查一下，首次运行的细节见[安装](https://kaku.fun/zh/docs/)。

## 标签与分屏

标签和分屏用的都是 Mac 上常见的那套快捷键。

| 想做什么 | 快捷键 | 结果 |
| --- | --- | --- |
| 开新标签 | `Cmd + T` | 在当前目录开一个新标签，标签栏随之出现。 |
| 查找标签或分屏 | `Cmd + Shift + O` | 打开 Tab Navigator，输入关键词筛选，直接跳到任意分屏。 |
| 左右分屏 | `Cmd + D` | 在当前分屏旁边并排再开一个。 |
| 上下分屏 | `Cmd + Shift + D` | 在当前分屏下方再开一个。 |
| 在分屏间切换 | `Cmd + Opt + 方向键` | 焦点移到那个方向的分屏。 |
| 关闭分屏 / 标签 | `Cmd + W` | 关掉当前分屏，标签里只剩这一个分屏时关掉标签，如果已经是最后一个窗口的最后一个标签，就只隐藏 Kaku。 |
| 重开刚关的标签 | `Cmd + Shift + T` | 恢复最后关掉的那个标签，目录还是原来的。 |
| 重命名标签 | 双击标签标题 | 输入名字后回车。 |

在 Tab Navigator 里对选中的标签按 `Backspace` 可以关掉它，确认提示和别处一样，不过只在筛选框为空时才会关，打错的字照样能删。

SSH 连接到远程机器后，标签会自动显示主机名而非本地路径，便于一眼区分。更多远程会话特性见[远程会话](https://kaku.fun/zh/docs/features#features-remote-sessions)。

重新打开 Kaku 时，窗口、分屏和每个分屏的工作目录都会恢复，就算某个分屏没存下来，其余的也照常回来。

在分屏内右键，可以复制选中的文字、粘贴、搜索、打开 AI 对话或命令面板、往任意方向拆分，或者关掉当前分屏（接管鼠标的 TUI 程序仍保留原生鼠标操作）。右键标签则可以新建标签、打开标签导航或关闭该标签；点击标签栏末尾的 + 按钮也能快速新建标签。

## 开箱即用的 Shell

Kaku 首次启动时会把 zsh 或 fish 配好，不用自己去改配置文件。

- **自动建议。**边打字边在光标后面给出一条来自历史记录的灰色建议。
- **Smart Tab。**有灰色建议时按 `Tab` 接受建议，没有时打开补全列表，这是默认行为，可以在[配置](https://kaku.fun/zh/docs/configuration)里改。
- **快速跳目录。**输入 `z proj` 就能跳到常去的目录，不用敲完整路径。
- **彩色输出。**命令和报错边输入边着色，装了 Delta 的话 `git diff` 会用它来分页。

选中文字松开鼠标就复制好了，`Cmd + 点击`用默认应用打开文件路径或网址，网址在终端边缘自动折行也能完整打开，真正换行之后的无关文字不会被拼进链接。

## AI 助手

助手默认处于关闭状态。开启前 Kaku 不会发出任何 AI 请求；开启后，命令执行失败时会自动给出修复建议，而命令生成与 AI 聊天则仅在主动触发时发起。

开启只需运行一次 `kaku ai`，启用 Kaku Assistant，选择 Auth Type（`codex` 复用 Codex 登录凭据，`api_key` 连接 OpenAI 兼容端点），并配置好 Simple Model 与 Deep Model 即可，之后有三种用法：

1. **修复失败的命令。**命令报错退出后，Kaku 会在提示符下方给出一条修复建议，按 `Cmd + Shift + E` 粘进来，`rm -rf` 这类危险命令只会粘出来给你看，不会自己执行。
2. **自然语言生成命令。**输入 `#` 加一句话（例如 `# 找到并杀掉占用 3000 端口的进程`），回车后 Kaku 会将其转换为真正的命令回填在提示符处，供你确认后再运行。
3. **打开聊天面板。**按 `Cmd + L` 打开聊天，回答是流式输出的，代码有高亮，还能读到当前终端的内容，在任意 shell 里用 `k "..."` 或 `kaku chat` 打开的也是同一段对话。

AI 服务由你自己配置，Kaku 不提供也不中转，完整设置见[功能特性](https://kaku.fun/zh/docs/features)。

## 内置工具

Lazygit 和 Yazi 都在当前面板打开，退出后回到 shell，远程文件则会新开一个标签。

| 工具 | 快捷键 | 作用 |
| --- | --- | --- |
| Lazygit | `Cmd + Shift + G` | 当前仓库的可视化 git 界面。 |
| Yazi | `Cmd + Shift + Y` | 文件管理器，退出时停在你选中的目录。 |
| 远程文件 | `Cmd + Shift + R` | 把当前 SSH 主机的文件挂载到本地，用 Yazi 打开。 |

缺 Lazygit 或 Yazi 时运行 `kaku init`，它会问你要不要装，远程文件还需要 sshfs，这部分和主题、行为细节都在[功能特性](https://kaku.fun/zh/docs/features)里。

## 设置与体检

- **设置面板。**按 `Cmd + ,` 或运行 `kaku config` 打开设置 TUI，滚动条、Smart Tab 模式、字号、窗口透明度这些都在这里调，AI 模型则在 `kaku ai` 里设。
- **Lua 配置。**自定义快捷键和其他 Lua 覆盖写在 `~/.config/kaku/kaku.lua`，在设置 TUI 里按 `e` 就能用编辑器打开它，见[配置](https://kaku.fun/zh/docs/configuration)。
- **检查环境。**感觉哪里不对就运行 `kaku doctor`，它会检查 Kaku 程序、PATH 和 shell 集成。

---

Source: https://kaku.fun/zh/docs/guide
Site index for LLMs: https://kaku.fun/llms.txt
