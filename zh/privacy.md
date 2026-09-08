# 隐私

Kaku 没有账号，也不采集使用数据。这一页列出 App 可能发起的全部网络请求。

## 概要

Kaku 是一个本地 macOS 应用，不需要 Kaku 账号或注册，没有使用统计和崩溃上报。日常终端使用不会把终端内容、命令历史或文件上传给项目方，你运行的命令可以自行联网。可选的 AI 功能会把使用到的上下文发送给下面列出的服务。

## App 会发起的网络请求

- **更新检查。**Kaku 会定期向 `api.github.com/repos/tw93/Kaku/releases/latest` 询问是否有新版本，发现新版本后可在后台从 GitHub 下载发布包，安装前会请你确认。对 GitHub 来说这就是一次来自你 IP 的普通 HTTPS 请求。在 `~/.config/kaku/kaku.lua` 里设 `config.check_for_updates = false` 可以关掉，用 `config.check_for_updates_interval_seconds` 可以改频率。
- **AI 助手。**用 `kaku ai` 开启 Kaku Assistant 后，prompt 和对话上下文会发给你配置的 AI 服务商，使用你的 API key 或 CLI 登录。上下文可能包含命令输出和助手工具读取的文件内容，项目方不通过自己的服务器中转，数据处理适用服务商的隐私政策。助手需要配置后才能使用，`#` 生成和自动修复命令等 shell 请求带有本地 capability 校验，仅打印在分屏里的文本无法触发这些请求。
- **AI 网页工具。**助手搜索或读取网页时，查询词和网址可能发送给你配置的 Brave、Tavily 或 Pipellm 服务（`api.search.brave.com`、`api.tavily.com`、`api.pipellm.ai` 或 `api.pipellm.com`）。默认网页读取器会把目标网址发送给 `defuddle.md`，失败时改用 `r.jina.ai`；自定义读取脚本使用哪些服务，由脚本决定。HTTP 请求工具直接访问目标网址，并发送指定的请求头和请求体，这些服务会收到请求，适用各自的隐私政策。
- **可选工具安装。**`kaku init` 在安装 Starship、Delta、Lazygit、Yazi 这类缺失的可选工具前会先询问。你同意后由 Homebrew 完成下载，Kaku 本身不下载任何东西。
- **没有遥测。**Kaku 不发心跳、安装 ping、功能使用事件或错误报告。

## 本机存了什么

Kaku 的状态都放在 `~/.config/kaku/` 下，包括 `kaku.lua`（你的 Lua 配置）、`assistant.toml`（助手设置，含你填入的 API key）、shell 集成文件、`Cmd + L` 和 `kaku chat` 用到的 AI 对话与记忆文件，以及用 CLI 登录（而非 API key）时缓存的服务商 token。这些都是归你的用户账户所有的普通本地文件，Kaku 不会把整个目录同步到项目服务器；作为 AI 上下文使用的对话和记忆内容，会发送给你配置的服务商。

`kaku reset` 会移除 Kaku 托管的 shell 和 tmux 集成、Kaku 托管的 git delta 默认值、部分 Kaku 状态，以及托管的主题块，托管块以外你自己写的 Lua 会保留。想彻底清干净，删掉 `~/.config/kaku/` 和 `/Applications/Kaku.app` 即可。

## 关于本站

kaku.fun 是静态站点。不设 cookie，不跑统计或追踪脚本，不嵌第三方组件，没有登录。也不从第三方 CDN 加载字体、脚本或图片，所有资源都来自本域名。站点托管在 Vercel，作为基础设施方，Vercel 会按常规保留服务器访问日志（IP、User-Agent、请求路径）用于运维。指向 GitHub、X 等站点的外链，适用对方自己的政策。

## 疑问与变更

Kaku 是 MIT 开源的，本页每一条说法都可以拿 [github.com/tw93/Kaku](https://github.com/tw93/Kaku) 的代码核对。如果发现对不上，那就是 bug，欢迎[提 issue](https://github.com/tw93/Kaku/issues)。本页的实质性变更会写进对应版本的发布说明里。

最后核对版本：Kaku v0.19.0。

---

Source: https://kaku.fun/zh/privacy
Site index for LLMs: https://kaku.fun/llms.txt
