# 隐私

Kaku 没有账号，也不收集使用数据，这一页列出 App 可能发出的每一个网络请求。

## 概要

Kaku 是跑在你 Mac 本地的应用，不用注册，也没有使用统计和崩溃上报，平时当终端用，终端内容、命令历史和文件都不会传给项目方，至于你运行的命令要不要联网，由命令自己决定。可选的 AI 功能会把用到的上下文发给下面列出的服务。

## App 会发起的网络请求

- **更新检查。**Kaku 会定期向 GitHub 的 `api.github.com/repos/tw93/Kaku/releases/latest` 查询有没有新版本，有的话可能先在后台从 GitHub 下好安装包，等你确认后才安装，对 GitHub 来说这只是一次来自你 IP 的普通 HTTPS 请求。不需要的话在 `~/.config/kaku/kaku.lua` 里设 `config.check_for_updates = false`，检查频率用 `config.check_for_updates_interval_seconds` 调。
- **AI 助手。**用 `kaku ai` 配好 Kaku Assistant 之后，prompt 和对话上下文会直接发给你选的 AI 服务商，用的是你的 API key 或已有的登录，上下文里可能有命令输出和助手工具读到的文件内容，中间不经过项目方的服务器，数据怎么处理看服务商的隐私政策。`#` 生成命令、自动修复命令这类从 shell 触发的请求要先过本地 capability 校验，分屏里只是打印出来的文字触发不了。
- **`kaku ai` 里的用量。**打开 `kaku ai` 时，它会显示你已经装好的 AI 编程工具的套餐用量，比如 Claude Code、Codex、Copilot 和 Kimi，做法是借用这些工具现成的登录，直接去问对应的服务商（`api.anthropic.com`、`chatgpt.com`、`api.github.com`、`api.kimi.com`），token 过期时会找服务商续期，结果缓存在 `~/.cache/kaku/`。模型列表来自 `models.dev` 的公开目录，这个请求不带任何账号信息。
- **AI 网页工具。**助手搜索或读网页时，查询词和网址可能发给你配置的 Brave、Tavily 或 Pipellm（`api.search.brave.com`、`api.tavily.com`、`api.pipellm.ai` 或 `api.pipellm.com`）。默认的网页读取器把网址发给 `defuddle.md`，不行再换 `r.jina.ai`，自定义读取脚本用哪些服务由脚本自己决定。HTTP 请求工具直接访问目标网址，带上助手指定的请求头和请求体，这些服务都适用各自的隐私政策。
- **可选工具安装。**首次启动的设置、`kaku init` 和 shell 集成更新都会先问你，同意后才可能用 Homebrew 安装缺少的可选工具，比如 Starship、Delta、Lazygit 和 Yazi，下载由 Homebrew 完成。如果连 Homebrew 都没装，Kaku 会再单独问一次，然后运行 `raw.githubusercontent.com` 上的 Homebrew 官方安装脚本。
- **没有遥测。**Kaku 不发心跳、安装统计、功能使用事件，也不上报错误。

## 本机存了什么

Kaku 把设置和 AI 数据放在 `~/.config/kaku/` 下，包括 `kaku.lua`（你的 Lua 配置）、`assistant.toml`（助手设置，含你填的 API key）、shell 集成文件、保存下来的会话布局、`Cmd + L` 和 `kaku chat` 用到的对话与记忆文件，以及用登录代替 API key 时缓存的服务商 token。日志、缓存、下载好的更新和粘贴进终端的图片放在 `~/Library/Application Support/kaku/`、`~/Library/Caches/kaku/`、`~/.cache/kaku/` 和 `~/.local/share/kaku/`。这些都是归你用户账户所有的普通文件，Kaku 不会把它们同步到项目服务器，只有作为 AI 上下文用到的对话和记忆内容会发给你配置的服务商。

`kaku reset` 会移除 Kaku 管理的 shell 和 tmux 集成、它写入的 git delta 默认配置、部分 Kaku 状态和托管的主题块，托管块以外你自己写的 Lua 会原样保留。想彻底清干净，删掉上面这几个目录和 `/Applications/Kaku.app` 就行。

## 关于本站

kaku.fun 是静态站点，不设 cookie，不跑统计和追踪脚本，不嵌第三方组件，也没有登录，字体、脚本和图片都从本域名加载，不走第三方 CDN。站点托管在 Vercel，Vercel 作为基础设施会照常保留访问日志（IP、User-Agent、请求路径）用于运维。点出去的 GitHub、X 等外链，适用对方自己的政策。

## 疑问与变更

Kaku 是 MIT 开源的，本页每一条都可以对着 [github.com/tw93/Kaku](https://github.com/tw93/Kaku) 的代码核对，发现对不上就是 bug，欢迎[提 issue](https://github.com/tw93/Kaku/issues)。本页有实质变化时，会写进对应版本的发布说明。

最后核对版本：Kaku v0.20.0。

---

Source: https://kaku.fun/zh/privacy
Site index for LLMs: https://kaku.fun/llms.txt
