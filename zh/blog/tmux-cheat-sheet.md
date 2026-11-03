# tmux 快捷键速查，附复制模式和最小配置

> tmux 默认前缀和会话、窗口、窗格、复制模式的快捷键，一份最小的 ~/.tmux.conf，以及在 Kaku 里怎么用。

发布于 2026-11-03 · Tw93 · https://kaku.fun/zh/blog/tmux-cheat-sheet

tmux 的每条命令都以前缀键开头，默认是 `Ctrl + B`，按下再松开，然后再按命令键。最先要记住的是 `c` 新建窗口、`%` 和 `"` 分屏、方向键在窗格之间移动、`d` 脱离会话、`[` 进入复制模式，这页其余的内容都是这几个的变化，对照 [tmux 手册](https://man.openbsd.org/tmux) 2026 年 10 月核对过。后面还有一份很短的 `~/.tmux.conf`，把鼠标打开，并且把前缀键换到更顺手的位置。

## 前缀键是怎么工作的

tmux 跑着一个 server，你的 shell 归它管，终端只是一个负责画面的 client。你按下前缀键之后，client 会切换到一张按键表，下一个键就被当作 tmux 命令来读，不再发给 shell。需要往程序里输入一个真正的 `Ctrl + B` 时，连按两次。Claude Code 就是这样的程序，它用 `Ctrl + B` 把正在跑的命令放到后台，文档里专门写了 tmux 用户要按两次。`prefix` 然后 `?` 会列出你这台机器上所有的绑定，这很有用，因为配置文件可以改掉下面任何一个默认值。

## 会话

会话是一组窗口，终端关掉它也还在。下面是 shell 里的命令，不是前缀键：

```bash
tmux new -s work          # 新建名为 work 的会话
tmux new -A -s work       # work 存在就接入，不存在就新建
tmux ls                   # 列出会话
tmux attach -t work       # 接入已有会话
tmux kill-session -t work # 销毁一个会话
tmux kill-server          # 销毁 server 和所有会话
```

在 tmux 里，`prefix` 然后 `d` 脱离会话并让一切继续运行，`prefix` 然后 `s` 从列表里选另一个会话，`prefix` 然后 `$` 重命名当前会话。

## 窗口

窗口相当于会话里的一个全屏标签页。

| 前缀之后按 | 作用 |
|---|---|
| `c` | 新建窗口 |
| `,` | 重命名窗口 |
| `n` 和 `p` | 下一个和上一个窗口 |
| `0` 到 `9` | 跳到对应编号的窗口 |
| `l` | 回到上一次选中的窗口 |
| `w` | 从列表里选窗口 |
| `&` | 关闭窗口 |

## 窗格

窗格是窗口里的一块分屏，名字很容易混，`%` 是把窗格分成左右两半，`"` 是分成上下两半。

| 前缀之后按 | 作用 |
|---|---|
| `%` | 左右分屏 |
| `"` | 上下分屏 |
| 方向键 | 移到那个方向的窗格 |
| `o` | 下一个窗格 |
| `q` | 短暂显示窗格编号 |
| `z` | 窗格放大到整个窗口，再按一次还原 |
| `x` | 关闭窗格 |
| `!` | 把窗格拆成独立窗口 |
| `Space` | 轮换预设布局 |
| `Ctrl + 方向键` | 调整一格 |
| `Alt + 方向键` | 调整五格 |

`Ctrl + 方向键` 和 `Alt + 方向键` 也是在按完前缀之后按的，如果被 macOS 的快捷键或者终端拦走，tmux 就收不到，用 `prefix` 然后 `?` 可以快速确认这个绑定存在。

## 复制模式

tmux 有自己的回滚缓冲区，所以终端自带的滚轮滚不动它。这个缓冲区默认每个窗格只留 2000 行，也就是 tmux 3.8 上 `tmux show -g history-limit` 打出来的值，一段长的构建日志或者 agent 会话，开头早就被挤掉了，在 `~/.tmux.conf` 里加一行 `set -g history-limit 50000` 能多留一些。`prefix` 然后 `[` 进入复制模式，`Page Up` 也能进入并向上翻一页。默认按键是 emacs 风格，把 `mode-keys` 设成 `vi` 就换成 vi 风格，如果你的 `VISUAL` 或 `EDITOR` 变量里含有 vi，tmux 本来就会选 vi 键。

| vi 模式下的键 | 作用 |
|---|---|
| `h` `j` `k` `l` | 移动光标 |
| `Ctrl + U` 和 `Ctrl + D` | 向上和向下翻半页 |
| `g` 和 `G` | 历史的顶部和底部 |
| `/` 和 `?` | 向前和向后搜索 |
| `n` 和 `N` | 下一个和上一个匹配 |
| `Space` | 开始选择 |
| `v` | 切换矩形选择 |
| `Enter` | 复制选中内容并退出复制模式 |
| `q` | 退出复制模式 |

`prefix` 然后 `]` 粘贴最近一次复制的缓冲区。这个缓冲区在 tmux 内部，它是否同时进入 macOS 剪贴板，要看终端和 tmux 的剪贴板设置，这页没有展开。

## 一份最小的 ~/.tmux.conf

tmux 不会自己创建这个文件，新建一个并写入：

```bash
# ~/.tmux.conf
set -g mouse on
set -g mode-keys vi
set -g base-index 1
set -g pane-base-index 1

# 可选：把前缀换成 Ctrl + A
set -g prefix C-a
unbind C-b
bind C-a send-prefix
```

`mouse on` 打开鼠标支持，手册说它默认是关的。`base-index 1` 和 `pane-base-index 1` 让编号从 1 开始，和键盘上的数字行对得上，默认是从 0 开始。`send-prefix` 让你按 `prefix` 再按 `Ctrl + A` 时，把一个真正的 `Ctrl + A` 发给 shell，这是需要的，因为 `Ctrl + A` 在 shell 里同时是回到行首。不重启就重新加载：

```bash
tmux source-file ~/.tmux.conf
```

如果你在 tmux 里跑 Claude Code，通知和 `Shift + Enter` 需要额外三行设置，写在 [tmux 替代方案那篇](/zh/blog/tmux-alternative-mac)里，这里不再重复。

## 在 Kaku 里用 tmux

tmux 在 Kaku 里和在别的终端里一样能跑，不需要上面的配置，不过装了 tmux 的话，`kaku init` 会往你的 `~/.tmux.conf` 里加一行 `source-file`，指向一个托管文件 `~/.config/kaku/tmux/kaku.tmux.conf`。这个文件打开 `mouse`，把 `KAKU_SESSION` 传进来，让你从 Kaku 接入之后新开的窗格保留 Kaku 的提示符，并且让 `Shift` 加滚轮进入复制模式。别手动改它，Kaku 可能会覆盖，你自己的设置写在 `~/.tmux.conf` 的其余部分。

Kaku 自己的 `Cmd + D` 和 `Cmd + Shift + D` 分屏和 tmux 窗格是两回事，本地工作用前者，需要比终端活得更久的会话用 tmux，分界线在哪里，[Mac 上的 tmux 替代方案](/zh/blog/tmux-alternative-mac)讲过，从别处接回一个会话可以看 [Claude Code remote](/zh/blog/claude-code-remote)。

## 常见问题排查

- **按了键没反应。** 前缀键没有先按下再松开，按住 `Ctrl`，点一下 `B`，两个都松开，再按下一个键。
- **鼠标能选文字但滚不动。** 鼠标支持是关的，加上 `set -g mouse on` 再重新加载。
- **改了配置像没生效。** `source-file` 是在正在运行的 server 里执行这个文件，每次编辑后都要跑一次 `tmux source-file ~/.tmux.conf`，或者 `tmux kill-server` 重新开始。
- **会话不见了。** 重启电脑之后 `tmux ls` 什么都没有，因为 server 是一个进程，不会跨重启存活。

## 常见问题

### tmux 默认的前缀键是什么？

`Ctrl + B`，在 `~/.tmux.conf` 里用 `set -g prefix` 改。

### 在 tmux 里怎么滚动？

用 `prefix` 然后 `[` 或者 `Page Up` 进入复制模式，再用方向键、`Page Up` 或上面的 vi 键，打开 `mouse on` 之后滚轮也行。

### 怎么关一个窗格或者结束一个会话？

在 shell 里输入 `exit`，或者 `prefix` 然后 `x` 关掉窗格，`tmux kill-session -t name` 结束会话。

### 怎么让 tmux 继续跑，之后再回来？

按 `prefix` 然后 `d`，回来时运行 `tmux attach`，在服务器上用 `tmux new -A -s main` 开始，这样同一条命令永远能用。
