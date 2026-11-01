# 终端模拟器是怎么工作的

> 终端管着 pty 和字符网格，shell 是写字节的子进程。顺着这条路径，字节变成格子，再变成像素。

发布于 2026-10-16 · Tw93 · https://kaku.fun/zh/blog/how-terminal-emulators-work

终端模拟器管的是窗口，它先申请一对伪终端，再把 shell 当作子进程接到其中一端，shell 写出来的字节先落成一块字符网格，然后才变成字形。shell 碰不到像素。颜色、光标和全屏程序都是字节序列，`TERM` 设错、窗口尺寸没跟上、或者一个连字占了好几格，坏的都是画面。

## shell 是被拉起来的子进程

在 Kaku 里新开一个窗格时，它先建好伪终端，再把 slave 这一端当作标准输入、标准输出和标准错误去启动 shell。`exec` 之前，子进程先 `setsid` 成为会话首进程，再 `ioctl(TIOCSCTTY)` 把这个 pty 设成控制终端，父进程留下 master，按键就写在这一端。[pty(7)](https://man7.org/linux/man-pages/man7/pty.7.html) 写的是同一件事，写进 master 的字节会像键盘输入一样出现在 slave 上，slave 写出的字节则可以从 master 读回来。

换 shell 不用换终端，[tmux](/zh/blog/tmux-alternative-mac) 把这层关系倒过来用，pty 归 tmux，终端只负责画。Kaku 的会话恢复带回来的是布局，然后新起一个 shell。

这个子进程的环境里，Kaku 把 `TERM` 设成 `xterm-256color`，`COLORTERM` 设成 `truecolor`，`TERM_PROGRAM` 设成 `Kaku`。`TERM_PROGRAM_VERSION` 是当前构建的版本字符串。自带配置坚持用这个 `TERM`，是因为远程机器上没有 `kaku` 这条 terminfo，配置里的注释写明，设成 `config.term = 'kaku'` 之后，SSH 过去的退格和删除会坏。

在 macOS 上，Kaku 调的是 `libc::openpty`，也就是 `openpty(3)`，一次调用返回 primary 和 replica 两个描述符，同时可以把窗口尺寸设上。手册里克隆设备是 `/dev/ptmx`，replica 设备是 `/dev/ttys` 加三位数字。Linux 的 pty(7) 把两端叫做 master 和 slave，slave 出现在 `/dev/pts/` 下面。Kaku 的类型名仍是 `UnixMasterPty` 和 `UnixSlavePty`，终端读写 master，shell 看到的终端是 slave。

尺寸通过 `TIOCSWINSZ` 写成一个 `winsize`，里面是行数、列数、像素宽和像素高，macOS 的 tty(4) 说多数全屏程序就是靠这份窗口尺寸知道画面有多大。`TIOCSCTTY` 旁边的注释写着，没有控制终端就不会收到 `SIGWINCH`，程序会一直用旧的行列。slave 关掉之后，读 master 会得到 `EIO`，Kaku 把它当成文件结束。

![shell 输出的字节经过 PTY 主设备进入转义序列解析器，解析器更新格子网格，HarfBuzz 把格子排成字形交给 WebGPU 窗口绘制，按键字节再经 PTY 主设备回到 shell](https://kaku.fun/img/blog/how-terminal-emulators-work.webp)

*shell、PTY 和终端窗口。*

输出字节从 shell 出来，经过 master，进解析器和字符网格，再经 HarfBuzz 到 WebGPU 窗口。按键字节从窗口写回 master，再交给 slave 上的 shell。

## 转义序列改的是格子

多数字节是正文。[console_codes(4)](https://man7.org/linux/man-pages/man4/console_codes.4.html) 记的是 Linux 控制台实现的那一套 VT102 和 ECMA-48 控制。`ESC`（`0x1b`）开始一条序列，`CSI` 是 `0x9b`，或者 `ESC` 后面跟一个 `[`。[xterm 的 ctlseqs](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html) 更新到补丁 411，解析按状态机走，参数一直收到最后那个决定动作的字节，遇到不该出现的字节就回到初始状态。

CSI 序列是 `ESC [`，后面可以有用分号隔开的数字，再跟一个结尾字符。`H`（CUP）把光标移到某一行某一列，原点是 1,1。`J` 清屏，`K` 清行，`m` 是 SGR，`0` 重置，`1` 是粗体，`31` 是红色前景，所以 `ESC [ 1 ; 31 m` 是粗体红字，`ESC [ 0 m` 清掉这些属性。`38;2;r;g;b` 是 24 位前景色。Kaku 设 `COLORTERM=truecolor`，就是告诉程序可以发这种序列，只认 terminfo 的程序则停在 `setaf` 描述的颜色上。

`Terminal` 拿着这个解析器，每条动作更新终端状态。画面模型是一个 `Screen`，行放在双端队列里，最后 `physical_rows` 行在屏幕上，更早的行是回滚。每个格子存一个字素、它的宽度，以及粗细、颜色这类属性，控制字符在建格子时被写成空格，不会原样留在网格里。主屏保留回滚，备用屏是另一个 `Screen`，回滚关掉，这和 ctlseqs 里 xterm 备用缓冲区只跟画面一样大是同一回事。

`CSI ? 1049 h` 保存光标、切到备用屏并清掉它，`CSI ? 1049 l` 切回来。terminfo 里这两段叫 `smcup` 和 `rmcup`，全屏程序进来时打前一段，退出时打后一段，所以提示符会回来。自带配置把主屏的 `scrollback_lines` 设成 10000，结构体在这份配置生效前的默认值是 3500，备用屏仍然没有回滚。自带的 `line_height` 是 `1.28`，格子被拉高，用方块拼出来的图形会一起变高。[配置](/zh/docs/configuration)和[常见问题](/zh/docs/faq)里写了，想让这些图形更接近正方，可以设到 `1.0` 到 `1.1`。窗口一变大小，行会按新的列数重排，同时发出 `TIOCSWINSZ`，处理 `SIGWINCH` 的程序会重画。

## 在 shell 里看这几层

```bash
tty
echo "$TERM"
stty -a
infocmp
```

`tty` 打出标准输入的 `ttyname(3)`，标准输入不是终端时打出 `not a tty`。本地窗格里这个名字就是 `openpty(3)` 分到的 replica。`echo "$TERM"` 在没改过 `config.term` 时是 `xterm-256color`。

`stty -a` 按 POSIX 的格式打出当前 termios。值得看的是 `rows`、`columns`、`erase`、`intr`、`echo` 和 `icanon`。`icanon` 开着时，内核会先处理退格和杀行，程序读到的是一整行，全屏程序会把它关掉，这样每个字节都能看见。`stty -g` 打出一份以后可以原样喂回 `stty` 的快照。

`infocmp` 不带参数时，用 terminfo 的名字打出 `$TERM` 那一条。可以找 `cup`（光标定位）、`clear`（清屏并回到原点）、`smcup` 和 `rmcup`（进出备用屏）、`setaf`（前景色）。下面这行会画出粗体红字再恢复。

```bash
printf '\033[1;31mred\033[0m\n'
```

`\033` 是 ESC，`[1;31m` 就是上面的 SGR，`[0m` 重置。想看原始字节，把 printf 重定向到文件，别让它进终端。

## 整形之后才到 GPU

格子是模型，画是后面的事。Kaku 把一段格子交给 HarfBuzz，WezTerm 的字体整形说明把这一步定义成展开连字、并应用字体里其他 OpenType 特性。每个字形记下它来自哪一个回退字体、FreeType 的字形索引、像素前进量和 `num_cells`，也就是这一个字形占几格。整形代码的注释拿 `!=` 当例子，两个字符合成一个字形，`num_cells` 用来让按格子走的光标和这个字形对齐。

自带字体栈是 JetBrains Mono，然后是 PingFang SC、Apple Symbols 和 Apple Color Emoji。Kaku 还会在后面追加几个回退字体，其中包括随包带的 Symbols Nerd Font Mono。浅色主题用 Medium，粗体用 SemiBold，深色主题用 Regular，粗体用 Medium，斜体按正体来画。图标变成空框，是这条链上的字体都没有那个码位，[Mac 上的 Nerd Font](/zh/blog/nerd-fonts-mac) 讲的就是这个缺口。

自带配置里，连字是关的。

```lua
config.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0' }
```

`calt`、`clig`、`liga` 就是 WezTerm 整形页上用来关掉连字的三项。配置页写明，把这项设成空表会重新打开连字。结构体在没有这行自带配置时的默认列表是 `kern`、`liga` 和 `clig`。覆盖写在 `~/.config/kaku/kaku.lua`，这个文件先加载自带默认，再应用你的设置。[一份实用的 WezTerm 配置](/zh/blog/wezterm-config-for-mac) 从上游 WezTerm 那边讲同样的字体和行距。

`front_end` 可以是 `OpenGL`、`WebGpu` 或 `Software`。WezTerm 文档里的默认是 `OpenGL`。`WebGpu` 在 macOS 上走 Metal，也可以走 Vulkan，Windows 上走 DirectX 12。`Software` 是 CPU 光栅化。Kaku 随包带的是这两行。

```lua
config.front_end = 'WebGpu'
config.webgpu_power_preference = 'LowPower'
```

`LowPower` 选集成 GPU，`HighPerformance` 选独立 GPU，而且只有 `front_end` 是 `WebGpu` 时这项才起作用。`max_fps` 和 `animation_fps` 都是 60。

## 按键怎么变成字节

按键被编成字节串，写到 master。默认编码是 xterm。`enable_csi_u_key_encoding` 默认关着，Kitty 键盘协议也关着，除非把 `enable_kitty_keyboard` 设成 true，配置页建议 Neovim 用户打开它。

newline 模式启动时是关的，这时 Enter 发送回车，字节是 `\r`，程序把 newline 模式打开之后，回车后面还会再跟一个换行。退格发送 DEL，也就是 `\x7f`，编码器注释把这叫做通常的 `VERASE`。Ctrl 加字母走 ASCII 控制字符映射，Alt 加 ASCII 字母或标点会先送一个 ESC。自带配置把左 Option 当成这个 Meta，右 Option 用来组合字符，字节到达 shell 之后的行编辑键在[Mac 终端快捷键](/zh/blog/mac-terminal-shortcuts)里。输入法在上屏之前不走这条字节路径，预编辑默认盖住光标下那一格，`ime_preedit_rendering = "BuiltinInsert"` 会把后面的格子往后让。

## 画面和程序对不上

颜色、方向键或清屏不对，先打出 `TERM` 再跑 `infocmp`。远程主机会用它自己的 terminfo 去解释你传过去的名字，所以除非那台机器上有对应条目，否则把 `TERM` 留在 `xterm-256color`。

换行位置不对，就看 `stty -a` 里的 `rows` 和 `columns` 跟窗格是否一致，它们在 `TIOCSWINSZ` 之后才变，不处理 `SIGWINCH` 的程序不会重画。回显停了，或者全屏程序崩溃后还停在备用屏，`tput reset` 会按 terminfo 把终端重新初始化，事先存下的 `stty -g` 那一行可以把模式标志设回去。

两个字符画成一个字形，说明 `calt`、`clig`、`liga` 不再是 0。空框是缺字形。QR code 被拉高，是 `line_height` 停在 `1.28`。窗格跟着 shell 一起消失，是 master 上读到了 `EIO`。进程要在窗口关掉之后还活着，那是 tmux 的事，Kaku 恢复的是布局和文字，然后新开一个 shell。

`openpty` 在没有空闲伪终端时返回 `EAGAIN`，碰到 `kern.tty.ptmx_max` 这个 sysctl 上限时返回 `ENXIO`。这个上限在 macOS 上，不在 `kaku.lua` 里。

## 常见问题

### tty 为什么会打出一个设备名？

它打出的是 `openpty` 给这个窗格分的 replica，这个节点表现得像终端，`/dev/ttys` 后面并没有串口。

### 退格为什么是 DEL？

退格编成 `0x7f`，`stty -a` 里它记在 `erase` 上。BS 是 `0x08`，这条路径只有按住 Control 时才把退格编成 BS。远程 terminfo 如果指望 BS，删字的方向会对不上，直到两边一致。

### GPU 会改变程序能打出什么吗？

用 `WebGpu` 时，Kaku 在网格更新之后经 Metal 把字形画出来。程序发的仍是 `xterm-256color` 里的序列，前端选的是怎么生成像素，pty 和解析器不动。

### 全屏程序里的回滚去哪了？

`smcup` 请求的是备用屏。xterm 的 `1049` 模式不保存额外的行，Kaku 的备用 `Screen` 也把回滚关掉。`rmcup` 回到主屏，自带配置里那 10000 行还在。
