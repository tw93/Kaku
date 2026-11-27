# V0.31.0 Bridge

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.31.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog
1. **Images Over SSH**: When `ssh` runs in the pane, Cmd+V with a screenshot copies it to the remote host and pastes its path, so Claude Code and Codex there attach it as an image.
2. **Reopening Sessions**: After reopening Kaku, the restored history shows above the new prompt with no second copy of the old one, including two-line zsh prompts and the last line of a command still running at quit.
3. **Font Size and Screen Edges**: Increasing the font size in an unmaximized window keeps it inside the screen, and a window reopens where it fits.
4. **Dock Bounce on Bell**: Set `config.bell_dock_bounce` to bounce the Dock icon when a bell rings while Kaku is in the background, once per pane or until you come back.
5. **Notifications While Focused**: With `notification_handling = "AlwaysShow"`, OSC 9 and OSC 777 notifications show again while the window is focused.
6. **Cursor After Full Screen Apps**: The cursor an app shows before leaving its full screen view stays visible, such as after connecting through sshs.
7. **Paths and Links**: Double-click selects a path with dashes in full, Cmd-hover links file paths that contain spaces, and plain text such as `3/4` or `and/or` no longer shows a link.
8. **Nerd Font Icons**: The bundled Symbols Nerd Font is updated to 3.5.1, so newer CLI icons render without a font warning.

### 更新日志
1. **SSH 里粘贴图片**：在 ssh 会话里按 Cmd+V 粘贴截图，Kaku 会把图片传到远端并贴上路径，远端的 Claude Code 和 Codex 会把它当成图片附件。
2. **重新打开会话**：重开 Kaku 后，恢复的内容显示在新提示符上方，不会多出一份旧提示符，两行的 zsh 提示符和退出时还在运行的命令最后一行也都正常保留。
3. **字号与屏幕边缘**：未最大化的窗口放大字号时不会超出屏幕，重新打开时窗口也会落在屏幕内。
4. **响铃时 Dock 跳动**：设置 `config.bell_dock_bounce` 后，Kaku 在后台时有面板响铃，Dock 图标会跳动一次或一直跳到你回来。
5. **前台时的通知**：`notification_handling` 设为 `"AlwaysShow"` 时，窗口在前台也会显示 OSC 9 和 OSC 777 通知。
6. **全屏程序退出后的光标**：程序退出全屏界面前显示的光标会保留下来，比如通过 sshs 连接后光标不再消失。
7. **路径与链接**：双击能选中带短横线的完整路径，Cmd 悬停能识别带空格的文件路径，`3/4`、`and/or` 这类普通文字也不再显示成链接。
8. **Nerd Font 图标**：内置的 Symbols Nerd Font 升级到 3.5.1，较新的 CLI 图标能正常显示，不再弹出字体提示。

Special thanks to @nuyhman and @Erchoc for their contributions to this release.

> https://github.com/tw93/Kaku
