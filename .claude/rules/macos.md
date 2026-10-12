# Kaku macOS Platform Rules

> macOS 26 上 AppKit 自动注入是 PAC-fault / 崩溃的高频来源。改 menubar / window handling 前必读本文。

## 两套像素坐标空间，禁止混算

窗口/屏幕几何存在两套空间，只在"窗口所在屏 scale == 主屏 scale"时数值兼容，混用的 bug 只在外接屏暴露（#456 拖动跳变）：

- **Canonical ScreenPoint 空间**：`cartesian_to_screen_point` 定义（全局 point × `screens[0]` 主屏 scale、Y 翻转）。`set_window_position` / `window_position` / `MouseEvent::screen_coords` / `MouseEvent::window_origin` 都在这里。
- **窗口 backing 空间**：`MouseEvent::coords`、`dimensions.pixel_*`、UI item 布局坐标，用窗口自己屏幕的 backingScaleFactor。

规则：跨空间换算一律在 `window/src/os/macos` 层做；GUI 层需要窗口原点用 `MouseEvent::window_origin`，不要用 `screen_coords - coords` 推算；居中走 `WindowOps::center()`（原生、单屏 point 空间）。`ScreenInfo.rect`（`screens()` / `resolve_geometry` / `--position` / Lua `screens()`）已统一到 canonical 空间（`nsscreen_to_screen_info` 走 `cartesian_to_screen_point`），`Window::new_window`（`window/src/os/macos/window.rs`）不再对 resolve 出的 x/y 除 scale（除一次就是 upstream 继承的 Retina 落点折半 bug，size 的除法保留，那是 px 转 pt 给 NSWindow 用的）。

## 默认渲染后端是 WebGpu，CGL-only 的 wake 修复护不住它

bundled `kaku.lua` 设了 `front_end = 'WebGpu'`（Metal via wgpu）。`window/src/os/macos` 里的睡眠/唤醒防护（`SYSTEM_SLEEPING` flush 闸门、`BackendImpl::update`、display-change present defer）全部只作用于 CGL 路径，对默认用户是 no-op。给 sleep/wake/display-reconfig 加修复时必须同时考虑 WebGpu 路径（#458）。

WebGpu surface 失效的恢复入口在 `TermWindow::do_paint_webgpu`：休眠唤醒会让 Metal drawable 失效但窗口尺寸不变，`WebGpuState::resize` 对相同尺寸是故意 no-op（热路径），所以恢复必须走 `reconfigure_surface()` 强制 `surface.configure`，不能用 `resize(dims)` 代替。wgpu Metal 后端把 nil `nextDrawable` 映射成 `SurfaceError::Timeout`，所以 Timeout 连续出现同样要触发重配，不能永远静默跳帧。症状签名：键盘输入照常进 PTY、画面停在旧帧，说明是渲染管线死了而不是主线程卡死。

## AppKit Menu 自动注入是地雷

NSApplication 默认会向 Window menu 和 Help menu 注入自己的项（Tile / Move & Resize / Bring All to Front / Help Search），并把 NSWindowRepresentingMenuItem 挂在 Windows menu 下做 dangling reference 。在 macOS 26 上，这些注入项可能 PAC-fault：

- `-[NSMenu _performKeyEquivalentWithDelegate:]` 在 Ctrl+Cmd+F 路由时崩
- Windows menu 关闭窗口后 NSWindowRepresentingMenuItem dangling
- Help Search field 触发 menu 重新计算时崩

## 必须执行的兜底

- 不要调 `setWindowsMenu:` 让 AppKit 接管 Windows menu。Kaku 有自己的 tab/window 切换器。
- 不要调 `setHelpMenu:` 让 AppKit 注入 Help Search。Kaku 没有 help book。
- 窗口创建时调 `setExcludedFromWindowsMenu:` 让 AppKit 看不见我们的 NSWindow，避免它把窗口加到 Windows menu。

参考历史 commit：
- `1ec3823` - 第一次排查 NSWindowRepresentingMenuItem dangling
- `4b15bab` - 进一步 stop AppKit from injecting into Window/Help menus
- `fc2dfa9` - redirect AppKit Spotlight for Help to an orphan menu
- `6ba7381` - restore synchronous menubar init to prevent key event loss

## Menubar 初始化时序

Menubar **必须同步初始化**。延后到 async block 会丢早期按键事件（Cmd+Q 在启动后立刻按下会被吞）。见 `6ba7381`。

## Key Equivalent

新增 KeyAssignment 前确认走 Kaku 自己的按键分发路径，不要走 AppKit menu 的 `performKeyEquivalent`。后者会经过被 PAC-fault 的注入项。

### menu keyEquivalent 会拦 keyDown，吞掉 raw-mode TUI 的 Ctrl+letter

macOS 26 上 NSMenu 的 keyEquivalent modifier 匹配并不严格相等。给 menu item 装 `Ctrl+letter` 形式的快捷键，即使带额外 modifier（如 `NSFunctionKeyMask` 想做 `Fn+Ctrl+letter`），仍会拦截 plain `Ctrl+letter` 的 keyDown。后果：

- AppKit 把 keyDown 路由给 menu item action，NSWindow 收不到。
- keyUp 不走 `performKeyEquivalent:`，所以 keyUp 照常进 Kaku。
- `termwiz/src/input.rs` 的 `encode` 对 `is_down=false` 直接返回空字符串，PTY 上一个字节都不会到。
- 普通 shell `cat -v` 测试看到 `^C` 不代表 raw-mode 也工作；启动时机和 cooked vs raw 都会让 menu 拦截表现不一致。**必须**在 raw-mode TUI (claude / codex / vim / htop) 里实测。

兜底：任何"模仿系统快捷键"的 menu 项宁可 `keys: vec![]` 不设快捷键，让用户从菜单点。`kaku-gui/src/commands.rs` 里的 keyEquivalent 装配逻辑必须统一从按键候选本身取 modifiers，不要为个别 menu 项走强制 modifier 的特例化路径（历史上的特例化路径已删，不要加回）。

参考历史：`8aba4475` feat(window): add CenterWindow menu without macOS shortcut（Window > Center 刻意不装 keyEquivalent，因为 macOS 26 会把 `Fn+Ctrl+C` 当 plain Ctrl+C 拦了，导致 raw-mode 下 Ctrl+C 不退 claude/codex）。

### 排查 keyDown 丢失

`config.debug_key_events = true` 重启 Kaku，复现按键，看 `~/.local/share/kaku/kaku-gui-log-<pid>.txt`：

- grep `key_event.*CTRL`。
- 如果只有 `key_is_down: false`、没有 `key_is_down: true`，就是 menu keyEquivalent 拦了 keyDown。
- 这种情况下不要去查 termwiz / PTY / termios，先找 `set_key_equiv_modifier_mask` 装配点。

## Cocoa 对象所有权：StrongPtr::new 只接 alloc / new / copy

`StrongPtr::new` 接管的是 +1 引用，只能包 `alloc`+`init`、`new`、`copy` 拿到的对象。`+[NSMenuItem separatorItem]` 这类类方法返回的是 autorelease 的 +0 对象，必须用 `StrongPtr::retain`，否则 drop 时多 release 一次。V0.22.0 前 `MenuItem::new_separator()` 就是这么写的：主菜单栏在启动时构建，那时还没有 autorelease pool（从代码推断，未实测），多出的那次 release 被泄漏抵消，所以一直没暴露；第一个在事件处理里构建、带分隔线的菜单（右键 tab 菜单）一弹出就崩（`7b6b6ce2`，带回归测试）。

- 崩溃签名：主线程 `objc_release > AutoreleasePoolPage::releaseUntil > objc_autoreleasePoolPop > -[NSApplication run]`，紧跟在一次原生菜单动作之后。看到它先审 `StrongPtr::new` 包的是不是非 alloc/new/copy 来的对象，再怀疑 AppKit。
- 拿不准某个 API 返回 +0 还是 +1，用 `clang -fno-objc-arc` 写个十行的 MRC 程序在 `@autoreleasepool` 里打印 `retainCount`，实测，不靠记忆。
- 新增在事件处理中构建的 `NSMenu`（右键菜单等），必须在 `make app` 的包里真的右键点一次菜单项再下结论，单元测试看不到 autorelease pool 的时序。
- 配置重载后菜单栏在 macOS 上也会重建（#570）：`frontend.rs` 的 reload 回调在 `spawn_into_main_thread` 任务里调 `recreate_menubar`，不在可能持有 config 锁的回调里直接调。原先挡住它的 TODO 疑似就是上面这个分隔线多释放，修复后放开。外观切换也会触发 `config::reload()`，所以这条重建路径在每次深浅色切换时都会走。`recreate_menubar` 给 Kaku 加的每一项打 `KAKU_MENU_ITEM_TAG`，重建时只删带这个 tag 的项、保留顶层菜单对象，再从每个菜单的开头按 rank 插入新项。不要改成 `remove_all_items` 清空主菜单或换新的 NSMenu：AppKit 只在启动时往 Edit 菜单注入 AutoFill、Start Dictation、Emoji & Symbols，菜单对象一换这些项就再也回不来，再调一次 `setMainMenu:` 也不行（MRC 小程序实测）。也不要换回旧的 mark-sweep：它只扫 `kakuPerformKeyAssignment:` 项，Kaku 菜单的固定项只在子菜单新建时创建、分隔线每次都追加，复用旧菜单会丢固定项、分隔线越积越多、新项排到末尾。`config_reload_rebuilds_menubar_on_macos_in_place` 守这几点。随 mux 状态变化的命令（Attach/Detach Domain、非 local 的 New Tab (Domain X)、Switch to workspace）只进命令面板不进菜单栏：菜单栏只在配置重载时重建，启动时 domain 还没注册，放进去会在一次深浅色切换后突然多出 `~/.ssh/config` 每个 Host 两条 Attach 项，之后又随 attach 状态过时，`mux_state_commands_stay_out_of_the_menubar` 守这一点。

## 调试方式

- 怀疑 menu 注入或菜单对象相关崩溃时：读 `~/Library/Logs/DiagnosticReports/kaku-gui-*.ips` 里最近的 crash report。
- 看 `NSMenu _performKeyEquivalentWithDelegate:` 栈帧是 AppKit 注入项的标志。
- 主要复现环境是一台 macOS 26 实机。
