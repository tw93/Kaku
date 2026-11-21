# How Terminal Emulators Work

> A terminal owns the pty and the cell grid. The shell is a child that writes bytes. Follow that path from those bytes to pixels.

Published 2026-10-16 · Tw93 · https://kaku.fun/blog/how-terminal-emulators-work

A terminal emulator owns the window. It allocates a pseudoterminal, starts your shell as a child on one end of that pair, and turns the bytes the shell writes into a grid of cells and then into glyphs. The shell never touches pixels. Color, cursor motion and full-screen apps are byte sequences, so a wrong `TERM`, a stale window size or a ligature covering several cells shows up as a broken screen.

## The shell is a child process

Kaku creates a pseudoterminal and spawns the shell with the slave side as standard input, standard output and standard error. Before `exec`, the child calls `setsid` and then `ioctl(TIOCSCTTY)`, so the pty is its controlling terminal. The parent keeps the master and writes keystrokes there. [pty(7)](https://man7.org/linux/man-pages/man7/pty.7.html) describes the same split: bytes written on the master arrive at the slave as if they had been typed, and bytes the slave writes can be read from the master.

The shell can be swapped without swapping the terminal. [tmux](/blog/tmux-alternative-mac) uses the same split the other way: tmux owns the pty, and the terminal only draws. Kaku's session restore brings the layout back and starts a new shell.

On that child, Kaku sets `TERM` to `xterm-256color`, `COLORTERM` to `truecolor`, and `TERM_PROGRAM` to `Kaku`. `TERM_PROGRAM_VERSION` is the running build's version string. The bundled config keeps that `TERM` because a remote host has no `kaku` terminfo entry. The comment in that file names broken backspace and delete over SSH as what `config.term = 'kaku'` causes.

On macOS, Kaku calls `libc::openpty` (`openpty(3)`). The call returns a primary descriptor and a replica, and it can set the window size at the same time. The man page names `/dev/ptmx` as the clone device and `/dev/ttys` plus three digits as the replicas. Linux pty(7) calls those ends master and slave, with slaves under `/dev/pts/`. Kaku's types are `UnixMasterPty` and `UnixSlavePty`: the terminal uses the master, and the shell sees the slave.

Size goes out as a `winsize` (rows, columns, pixel width, pixel height) through `TIOCSWINSZ`. macOS tty(4) says this window size is how most full-screen programs learn the screen size. The comment beside `TIOCSCTTY` says that without a controlling terminal, `SIGWINCH` is not delivered, so those programs keep the old size. Once the slave closes, a read on the master returns `EIO`, which Kaku treats as end of file.

![Bytes from the shell pass through the PTY master into an escape parser that updates a cell grid, HarfBuzz shapes those cells into glyphs that the WebGPU window draws, and keystrokes travel back through the PTY master to the shell](https://kaku.fun/img/blog/how-terminal-emulators-work.webp)

*The shell, PTY and terminal window.*

Output bytes go from the shell, through the master, into the parser and the cell grid, then through HarfBuzz to the WebGPU window. Key bytes go from the window back through the master to the shell.

## Escape sequences update a grid

Most bytes are text. [console_codes(4)](https://man7.org/linux/man-pages/man4/console_codes.4.html) documents the VT102 and ECMA-48 controls the Linux console implements. `ESC` (`0x1b`) starts a sequence. `CSI` is `0x9b`, or the two bytes `ESC` `[`. [xterm ctlseqs](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html), through patch 411, treats parsing as a state machine: parameters run until a final byte, and an unexpected byte returns to the ground state.

A CSI sequence is `ESC [`, optional numbers separated by semicolons, and a final character. `H` (CUP) moves the cursor to a row and column, origin at 1,1. `J` erases the display, `K` erases the line, and `m` is SGR: `0` resets, `1` is bold, `31` is red, so `ESC [ 1 ; 31 m` is bold red and `ESC [ 0 m` clears it. `38;2;r;g;b` is a 24-bit foreground. `COLORTERM=truecolor` is how Kaku tells programs to send it. A program that only trusts terminfo stays with `setaf`.

`Terminal` holds the parser, and each action updates terminal state. The model is a `Screen`, a deque of lines: the last `physical_rows` are visible, and earlier lines are scrollback. A cell stores a grapheme, its width and attributes such as intensity and color. Controls are stored as a space. The primary screen keeps scrollback. The alternate screen is a second `Screen` with scrollback off, matching ctlseqs, where xterm's alternate buffer is only as large as the display.

`CSI ? 1049 h` saves the cursor, switches to that screen and clears it. `CSI ? 1049 l` switches back. Terminfo names the strings `smcup` and `rmcup`. Full-screen programs print them on entry and exit, which is why the prompt returns. Bundled `scrollback_lines` is 10000. The struct default, before that file runs, is 3500. The alternate screen still has none. Bundled `line_height` is `1.28`, so block graphics stretch with the taller row. The [configuration page](/docs/configuration) and the [FAQ](/docs/faq) cover `1.0` to `1.1` when those graphics should be closer to square. A resize rewraps lines and sends `TIOCSWINSZ`. Programs that handle `SIGWINCH` redraw.

## Commands that show each layer

```bash
tty
echo "$TERM"
stty -a
infocmp
```

`tty` prints the `ttyname(3)` of stdin, or `not a tty`. In a local pane that is a replica from `openpty(3)`. `echo "$TERM"` prints `xterm-256color` unless `config.term` changed.

`stty -a` prints the termios state in POSIX form. Check `rows`, `columns`, `erase`, `intr`, `echo` and `icanon`. With `icanon` set, the kernel handles erase and kill before the program reads a line. Full-screen programs turn it off. `stty -g` prints a snapshot a later `stty` can restore.

`infocmp` prints the terminfo source for `$TERM`. The useful names are `cup`, `clear`, `smcup`, `rmcup` and `setaf`. This draws bold red, then resets:

```bash
printf '\033[1;31mred\033[0m\n'
```

`\033` is ESC, `[1;31m` is the SGR above, and `[0m` resets. Redirect printf to a file to see the bytes with no parsing.

## Shaping, then the GPU

Cells are the model. Kaku then shapes runs with HarfBuzz, which WezTerm's notes describe as the ligature and OpenType step. A glyph records its fallback font, FreeType index, pixel advance and `num_cells`. The shaper comment's example is `!=` becoming one glyph, and `num_cells` keeps the cell cursor aligned with it.

The bundled list is JetBrains Mono, then PingFang SC, Apple Symbols and Apple Color Emoji, and Kaku appends more fallbacks after it, including its bundled Symbols Nerd Font Mono. Light themes use Medium (SemiBold for bold), dark themes Regular (Medium for bold), and italics are drawn upright. An empty box is a code point none of these fonts contain. [Nerd Fonts on Mac](/blog/nerd-fonts-mac) is about that gap.

Ligatures are off in the shipped config:

```lua
config.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0' }
```

Those are the three features WezTerm's shaping page tells you to disable. The configuration page says an empty table turns ligatures back on. The struct default, used when that bundled line is absent, is `kern`, `liga` and `clig`. The override goes in `~/.config/kaku/kaku.lua`, after the bundled defaults load. [A practical WezTerm config](/blog/wezterm-config-for-mac) goes through the same font and spacing settings on upstream WezTerm.

`front_end` may be `OpenGL`, `WebGpu` or `Software`. WezTerm's documented default is `OpenGL`. `WebGpu` uses Metal on macOS, Vulkan, or DirectX 12 on Windows. `Software` rasterizes on the CPU. Kaku ships:

```lua
config.front_end = 'WebGpu'
config.webgpu_power_preference = 'LowPower'
```

`LowPower` selects the integrated GPU and `HighPerformance` a discrete one, only when `front_end` is `WebGpu`. `max_fps` and `animation_fps` are both 60.

## How keys become bytes

A key is encoded and written to the master. The default encoding is xterm. `enable_csi_u_key_encoding` defaults off, and the Kitty keyboard protocol stays off unless `enable_kitty_keyboard` is true, which the configuration page suggests for Neovim.

Newline mode starts off, so Enter sends carriage return, `\r`. If a program turns newline mode on, a line feed follows. Backspace sends DEL, `\x7f`, which the encoder treats as the usual `VERASE`. Control plus a letter uses the ASCII control map. Alt plus an ASCII letter or punctuation prefixes ESC. The bundled config makes the left Option key act as Meta and the right Option key a compose key. [Mac terminal shortcuts](/blog/mac-terminal-shortcuts) picks up once those bytes reach the shell. Until an input method commits, the preedit covers the cell under the cursor. `ime_preedit_rendering = "BuiltinInsert"` shifts the following cells instead.

## When the screen and the program disagree

If colors, cursor keys or a clear go wrong, print `TERM` and run `infocmp`. A remote host uses its own terminfo for the forwarded name, so leave `TERM` at `xterm-256color` unless that host has the entry.

If lines wrap in the wrong place, `rows` and `columns` from `stty -a` should match the pane. They change with `TIOCSWINSZ`, and a program that ignores `SIGWINCH` will not redraw. If echo has stopped, or a crash left the alternate screen up, `tput reset` reinitializes from terminfo. A saved `stty -g` line puts the old mode flags back.

Two characters drawn as one glyph means `calt`, `clig` and `liga` are no longer 0. Empty boxes are missing glyphs. A tall QR code is `line_height` at `1.28`. A pane that closes with the shell saw `EIO`. tmux keeps the process. Kaku restores the layout and the text, then starts a shell.

`openpty` fails with `EAGAIN` when no pseudoterminal is free, and with `ENXIO` at the `kern.tty.ptmx_max` sysctl. That limit is macOS, outside `kaku.lua`.

## FAQ

### Why does tty print a device name with nothing plugged in?

It prints the replica `openpty` allocated for this pane. That node behaves like a terminal. There is no serial port behind a `/dev/ttys` name.

### Why is the erase character DEL?

Backspace encodes as `0x7f`, the `erase` value in `stty -a`. BS (`0x08`) is sent for Backspace only with Control held. A remote terminfo that expects BS erases the wrong way until both sides match. With `icanon` on, the kernel erases one byte per Backspace unless the `iutf8` flag is set. A new macOS pty starts with `-iutf8`, and Kaku 0.30.0 or later turns it on for every pane, so Backspace in `read` or `cat` removes a whole Chinese character or emoji instead of leaving broken bytes.

### Does the GPU change what a program can print?

With `WebGpu`, Kaku draws the glyphs through Metal after the grid updates. Programs still send `xterm-256color` sequences. The front end does not change the pty or the parser.

### Where does scrollback go in a full-screen program?

`smcup` requests the alternate screen. xterm mode `1049` keeps no saved lines, and Kaku's alternate `Screen` has scrollback off. `rmcup` brings back the primary screen and its 10000 lines.
