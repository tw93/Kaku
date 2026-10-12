# V0.22.0 Handy

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.22.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog

1. **Tab Bar**: Tabs are easier to hit with a finger over remote desktop, a + button opens new tabs, and right-clicking a tab offers New Tab, Tab Navigator, and Close Tab.
2. **Typing Lag**: Panes that once showed inline images no longer freeze typing each time Kaku saves the session in the background.
3. **App Titles**: Set `config.tab_title_use_pane_title = true` to show the titles apps like Claude Code set, while tabs you renamed keep your name.
4. **Kitty Keyboard**: With `enable_kitty_keyboard` on, Escape and forward Delete work in Herdr and Neovim, and swapping Backspace and Delete applies there too.
5. **Input Methods**: Set `config.ime_preedit_rendering = 'BuiltinInsert'` to show the text you are composing as inserted text instead of covering the next character.
6. **Powerlevel10k**: Kaku no longer prints anything before the first prompt, so Powerlevel10k instant prompt stops warning.
7. **Command Line**: `kaku start` now opens in the Kaku that is already running instead of launching a second copy, and `--new-tab` adds a tab to the current window.
8. **Split Panes**: The line between top and bottom panes now sits where their backgrounds meet, and with inactive panes dimmed it blends into the background while the dimming reaches the window edges.
9. **Diagnostics**: `kaku doctor` saves a redacted diagnostic bundle you can attach to a bug report.

### 更新日志

1. **标签栏**：远程桌面下用手指也更容易点中标签，标签栏末尾多了 + 按钮，右键标签可以新建、打开标签导航或关闭标签。
2. **输入卡顿**：Kaku 在后台保存会话时，显示过内联图片的分屏不会再让输入卡住。
3. **程序标题**：设置 `config.tab_title_use_pane_title = true` 后，标签会显示 Claude Code 这类程序设置的标题，手动重命名过的标签保留你起的名字。
4. **Kitty 键盘**：开启 `enable_kitty_keyboard` 后，Herdr 和 Neovim 里的 Esc 和向前删除恢复正常，交换 Backspace 与 Delete 的设置也会生效。
5. **输入法**：设置 `config.ime_preedit_rendering = 'BuiltinInsert'` 后，输入法正在组合的文字会插入显示，不再盖住后面的字。
6. **Powerlevel10k**：Kaku 不再在第一个提示符前输出内容，Powerlevel10k 的 instant prompt 不会再报警告。
7. **命令行**：`kaku start` 会在已经运行的 Kaku 里打开，不再另起一个，加上 `--new-tab` 会在当前窗口新建标签。
8. **分屏**：上下分屏的分隔线现在落在两侧背景的交界处，开启非活动分屏变暗后，分隔线融入背景，变暗也会铺满到窗口边缘。
9. **诊断**：`kaku doctor` 会生成一份脱敏的诊断包，提 bug 时可以直接附上。

> https://github.com/tw93/Kaku
