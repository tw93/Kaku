# V0.28.0 Steady

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.28.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog
1. **Crash Fix**: Kaku no longer quits a minute after restoring a session with a long scrollback, which made it crash again on every reopen, and no longer quits when a menu action reaches a fullscreen window.
2. **Safe Updates**: Restart to Update always leaves a working Kaku.app in Applications, even when the update is interrupted halfway.
3. **Chinese Input**: The input method's candidate window now sits below the line you are typing, instead of covering half of the pinyin.
4. **Inline Images**: Images placed with kitty Unicode placeholders, such as the diagrams from Claude Code's figures plugin, now draw instead of showing boxes.
5. **Dim Text**: With a minimum contrast set, dim text stays dimmer than normal text on dark backgrounds, and a font named with a weight such as Light keeps that face for dim text.
6. **Folder Links**: Cmd+Click on a folder path opens it in Finder instead of VS Code.
7. **Remote Terminal**: Option+click an image path to view it over the page, hover previews open faster and larger, the close confirmation sits by its tab, and the page sends far less data while an agent is working.
8. **Freeze Recovery**: When keys and scrolling stop responding while menus still open, Kaku now tries to get its window running again by itself.
9. **App Icon**: The icon drops its baked-in shadows and sits on a darker ground, so it looks at home beside the system icons in the Dock.

### 更新日志
1. **崩溃修复**：恢复带很长滚动记录的会话后，Kaku 不会再在一分钟后退出、重新打开又接着崩溃，菜单操作落到全屏窗口上时也不会再退出。
2. **安全更新**：重启以更新即使中途被打断，应用程序里也总会留着一个能用的 Kaku.app。
3. **中文输入**：输入法候选框现在出现在正在输入的那一行下方，不会再挡住一半拼音。
4. **终端图片**：用 kitty Unicode 占位符放置的图片，比如 Claude Code 的 figures 插件画的图，现在可以正常显示，不再是一排方框。
5. **暗淡文字**：设置了最低对比度时，深色背景上的暗淡文字仍然比普通文字暗，字体名带 Light 这类字重的，暗淡文字也会继续用这个字重。
6. **文件夹链接**：Cmd+点击文件夹路径会在访达里打开，不再用 VS Code 打开。
7. **远程终端**：按住 Option 点击图片路径可以直接在页面上看图，悬停预览更快也更大，关闭确认出现在对应标签旁边，Agent 干活时页面传输的数据也少了很多。
8. **卡死恢复**：按键和滚动都没反应、菜单却还能打开时，Kaku 现在会自己尝试让窗口恢复运行。
9. **应用图标**：图标去掉了原来的阴影，底色更深，放在程序坞里和系统图标更协调。

Special thanks to @multics, @sunbird89629 and @HiAriesZhou for their contributions to this release.

> https://github.com/tw93/Kaku
