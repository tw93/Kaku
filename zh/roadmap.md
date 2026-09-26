# 路线图

Kaku 现在是 0.20.x 版本线，这一轮主要做三件事，让会话在崩溃后也能恢复，把右键菜单补齐，再修掉启动和关窗口时几处可能崩溃的地方。

## Now

1. 运行时就定期保存会话，Kaku 崩溃或被强制退出后，窗口、分屏和工作目录也能照常恢复。
2. 继续补右键菜单，先加上复制，顺手修掉分屏标签页不显示序号这类标签栏细节。
3. 修掉启动和关闭窗口时几处可能导致崩溃的地方。

---

## Next

1. 让终端设置更好找，也更好懂。
2. 继续收紧 AI 工具的沙箱和文件访问规则。
3. Homebrew 已经改用官方 cask，接下来继续打磨 macOS 上的安装、更新和首次启动。

---

## Later

1. 评估非 macOS 平台，不给时间表。
2. 会话录制和回放，前提是全程在本地，用起来一看就懂。
3. 和 IDE、tmux、远程开发做更多集成。

发布说明在 [GitHub Releases](https://github.com/tw93/Kaku/releases)，具体问题和实现讨论放在 [GitHub Issues](https://github.com/tw93/Kaku/issues)。

---

Source: https://kaku.fun/zh/roadmap
Site index for LLMs: https://kaku.fun/llms.txt
