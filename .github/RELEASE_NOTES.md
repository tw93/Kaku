# V0.30.0 Steady

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.30.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog
1. **Full Screen Freeze**: After an update notice in full screen, Kaku keeps responding to keys, scrolling and tab switches, and showing the window from a Lua action no longer crashes.
2. **Chinese and Emoji Backspace**: In programs that read whole lines, such as `read` and `cat`, Backspace removes a whole Chinese character or emoji instead of leaving broken bytes.
3. **Quieter Agent Title**: When Claude Code already shows the session topic on its input box border, the window's top right and the remote page keep only the agent mark.
4. **Unbound Cmd Keys**: A Cmd shortcut with no binding no longer types its letter into the program, such as a stray j in the Claude Code prompt.
5. **Updating While Open**: When `brew upgrade` replaces the app under a running Kaku, reloading the config keeps every default setting instead of switching to a cream tab bar.
6. **Password Prompts**: The cursor stays as it is while you type a password, and `config.detect_password_input = true` brings back the lock.
7. **Tab Fixes**: Hovering the left edge of a retro tab highlights that tab, and switching on the Mac to a tab a browser had resized fills the window again.

### 更新日志
1. **全屏卡死**：全屏时点开更新提示后，按键、滚动和切换标签都能正常响应，通过 Lua 动作显示窗口也不会再崩溃。
2. **中文与表情删除**：在 `read`、`cat` 这类按行读取输入的程序里，Backspace 会删掉整个中文字符或表情，不再留下残缺字节。
3. **Agent 标题去重**：Claude Code 的输入框边框已经显示会话话题时，窗口右上角和远程页面只保留 Agent 图标。
4. **未绑定的 Cmd 快捷键**：按下没有绑定的 Cmd 组合键不会再把字母打进程序，比如 Claude Code 输入框里多出的 j。
5. **运行中升级**：Kaku 开着时被 `brew upgrade` 更新，重载配置后默认设置都会保留，标签栏不再变成米色样式。
6. **密码输入**：输入密码时光标保持原样，需要锁形光标可以设置 `config.detect_password_input = true`。
7. **标签修复**：鼠标悬停在复古标签左侧时高亮的就是这个标签，在 Mac 上切到被浏览器调整过尺寸的标签也会重新铺满窗口。

Special thanks to @HiAriesZhou for the contributions to this release.

> https://github.com/tw93/Kaku
