# Mac 上安装和使用 Gemini CLI，2026 年登录方式有变化

> 用 npm 或 Homebrew 安装，2026 年 6 月 18 日后还能用的登录方式，GEMINI.md、MCP、沙盒和 Kaku。

发布于 2026-10-26 · Tw93 · https://kaku.fun/zh/blog/gemini-cli-setup

Mac 上装 Gemini CLI 用 `npm install -g @google/gemini-cli` 或者 `brew install gemini-cli`，装好后在项目目录里运行 `gemini` 就能开始。2026 年的麻烦出在登录上，Google 宣布 2026 年 6 月 18 日起，Gemini CLI 和 Gemini Code Assist 的 IDE 扩展不再为 Google AI Pro、Ultra 用户和通过 Gemini Code Assist for individuals 免费使用的人提供服务，并引导他们改用 Antigravity CLI，已经买了 Code Assist Standard 或 Enterprise 的组织可以用到合同结束，不过 Google 从 2026 年 10 月 9 日起不再卖新的订阅，Gemini CLI 也仍然可以配合付费的 Gemini 和 Gemini Enterprise Agent Platform API key 使用。

## 2026 年发生了什么变化

[Gemini CLI 的 README](https://github.com/google-gemini/gemini-cli) 现在开头就有一条提示，说对未付费档位和 Google One 用户，它在 2026 年 6 月 18 日已被 Antigravity CLI 取代，并且它没有针对最新的 Gemini 模型做优化。Google 的[公告](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli)把界线划得很清楚，个人用户中的 Google AI Pro、Ultra 和免费 Code Assist 档位失去访问，使用 Gemini Code Assist Standard 或 Enterprise 的组织当时继续保留，Gemini CLI 也仍然可以配合付费的 Gemini 和 Gemini Enterprise Agent Platform API key 使用。后来 Google 的[停售说明](https://docs.cloud.google.com/gemini/docs/codeassist/sunset)把这两种订阅也排上了时间表，2026 年 10 月 9 日起不再卖新的，加授权和年付续费到 2027 年 1 月 31 日为止，月付续费到 2027 年 12 月 31 日为止，已有合同支持到期满，往后的方向是 Gemini Enterprise 里的 Antigravity。

这一点很重要，因为很多老教程还在写「用 Google 账号登录，每天有免费额度」，对个人账号来说这条路在 2026 年 6 月 18 日已经结束了。新的个人用户建议直接从 Antigravity CLI 开始，它的命令是 `agy`，第一次启动会找到你原来的 Gemini CLI 配置，问你要转换哪些。按 Google 的[迁移指南](https://antigravity.google/docs/cli/gcli-migration)，`GEMINI.md` 和 `AGENTS.md` 原样就能用，项目里的 skill 要从 `.gemini/skills/` 挪到 `.agents/skills/`，MCP 服务器不再写在 `settings.json` 里，改放到 `~/.gemini/config/mcp_config.json` 或者项目的 `.agents/mcp_config.json`，`url` 和 `httpUrl` 也要改成 `serverUrl`，扩展可以用 `agy plugin import gemini` 转成插件。手上有付费 API key、有组织账号，或者已经有基于 `gemini` 写的脚本，再往下看。

## 在 Mac 上安装

文档推荐的环境是 macOS 15 或更新、Node.js 20.0.0 或更新，轻度使用 4 GB 内存。README 给出的安装方式有这几种：

```bash
# 不安装，直接试用
npx @google/gemini-cli

# npm 全局安装
npm install -g @google/gemini-cli

# Homebrew 全局安装
brew install gemini-cli
```

MacPorts（`sudo port install gemini-cli`）也可以。装完用 `gemini --version` 确认。发布分三个渠道，`latest` 是每周的稳定版，`preview` 每周二发布，可能有回归，`nightly` 每天从 main 构建。想固定渠道可以用 `npm install -g @google/gemini-cli@preview`。

## 登录

运行 `gemini`，在提示里选一种方式，之后也可以用 `/auth` 切换。README 记录了三种：

| 方式 | 做法 | 适合 |
|---|---|---|
| Sign in with Google | 浏览器登录，凭据缓存在本地 | 有 Code Assist 授权的组织账号，要设置 `GOOGLE_CLOUD_PROJECT` |
| Gemini API key | 从 Google AI Studio 拿 key，`export GEMINI_API_KEY="..."` | 想指定模型、按用量计费 |
| Vertex AI | 带 key 设置 `GOOGLE_GENAI_USE_VERTEXAI=true`，或 `gcloud auth application-default login` | 企业团队 |

用 Vertex AI 还要设置 `GOOGLE_CLOUD_PROJECT` 和 `GOOGLE_CLOUD_LOCATION`，无头运行用 API key 或者 Vertex AI。key 放在 shell 配置里，不要放进仓库，把它当成凭据对待。

关于额度，付费 API key 的限额取决于你的定价档位，具体看 [Gemini API 速率限制](https://ai.google.dev/gemini-api/docs/rate-limits) 页面。会话里 `/stats model` 会显示各个模型的 token 用量和额度信息。

## 它怎么加载上下文

Gemini CLI 的上下文来自 `GEMINI.md` 文件，相当于 `CLAUDE.md`。它把找到的所有文件拼起来，随每次提问一起发给模型，分三层，全局文件是 `~/.gemini/GEMINI.md`，工作区文件从你配置的工作区目录和它们的上级目录里找，即时文件则是工具访问某个文件或目录时，扫描该目录及其上级直到受信任的根目录。

```markdown
# Project: my-api

- 缩进用 2 个空格。
- 收尾前运行 `pnpm test`。

@./docs/style-guide.md
```

`@file.md` 可以把长文件拆开。`/init` 会根据当前目录生成一份 `GEMINI.md`，`/memory show` 打印模型实际收到的拼接后的上下文，`/memory list` 列出正在用的文件，底部状态栏会显示加载了几个上下文文件。仓库里已经有 `AGENTS.md` 的话，在 `settings.json` 里把 `context.fileName` 设成 `["AGENTS.md", "GEMINI.md"]`，Gemini CLI 就会一起读。关于长度的取舍和 Claude Code 的 [`CLAUDE.md`](/zh/blog/claude-md-memory) 是同一回事。

## settings.json、斜杠命令和 MCP

设置放在 `~/.gemini/settings.json`，项目里的放在 `.gemini/settings.json`，`/settings` 可以交互式修改。命令参考里常用的斜杠命令有 `/auth`、`/model`、`/memory`、`/mcp`、`/stats`、`/permissions`、`/plan`、`/resume`、`/rewind`、`/skills` 和 `/hooks`。

MCP 服务器写在 `mcpServers` 下面，支持 Stdio、SSE 和 streamable HTTP 三种传输：

```json
{
  "mcpServers": {
    "github": {
      "command": "path/to/server",
      "args": ["--arg1", "value1"],
      "env": { "API_KEY": "$MY_API_TOKEN" },
      "trust": false
    }
  }
}
```

`gemini mcp add <name> <commandOrUrl>` 可以替你写入这一项，`/mcp list` 看连上了哪些。`trust` 保持 false，设成 true 会跳过这个服务器所有工具调用的确认。换个角度看同一件事，可以读 [Claude Code 的 MCP 服务器](/zh/blog/claude-code-mcp-servers)。

## 沙盒

沙盒把 shell 命令和文件改动与你 Mac 的其他部分隔离开。可以用 `gemini -s`、环境变量 `GEMINI_SANDBOX`（取值 `true`、`docker`、`podman` 或 `sandbox-exec`），或者在设置里写 `"tools": { "sandbox": "docker" }` 来打开。macOS 上内置的方式是通过 `sandbox-exec` 调用 Seatbelt，默认的 `permissive-open` 配置把写入限制在项目目录，读取和网络仍然放开。Docker 和 Podman 会把你的工作目录以同样的绝对路径挂进容器。

## 和 Claude Code、Codex 对比

| | Gemini CLI | Claude Code | Codex CLI |
|---|---|---|---|
| 安装 | npm 或 Homebrew | 原生安装器或 Homebrew cask | 安装脚本、npm 或 Homebrew |
| 上下文文件 | `GEMINI.md` | `CLAUDE.md` | `AGENTS.md` |
| 设置 | `~/.gemini/settings.json` | `~/.claude/settings.json` | `~/.codex/config.toml` |
| 沙盒与审批 | Seatbelt、Docker、Podman | `allow`、`ask`、`deny` 规则 | `sandbox_mode` |
| 个人订阅登录 | 2026 年 6 月 18 日终止 | Pro、Max、Team、Enterprise | ChatGPT Plus 及以上 |

Gemini CLI 是 Apache 2.0 开源的，文档里写明了 Seatbelt、Docker 和 Podman 三种沙盒。它眼下的短板在方向上，它自己的 README 就写着，想用 Gemini 模型获得最好的编程体验，应该用 Antigravity CLI。另外两个工具的安装见 [在 Mac 上安装 Claude Code](/zh/blog/install-claude-code-mac)、[Codex CLI 安装](/zh/blog/codex-cli-setup) 和 [Codex 和 Claude Code](/zh/blog/codex-vs-claude-code)。打算换到 Claude Code 的话，在会话里跑 `/import gemini --dry-run`（需要 Claude Code 2.1.213 或更新）可以先看它会从 Gemini CLI 搬过来哪些东西，包括指令文件、MCP 服务器、命令、subagent 和 skill，指令是一次性复制追加到对应的 `CLAUDE.md` 里的，之后再改 `GEMINI.md` 不会跟过去。

## 在 Kaku 里运行

`kaku ai` 会打开 AI 设置面板，里面把 Gemini CLI 和 Claude Code、Codex、Copilot CLI、Kimi Code 并列。Kaku 0.25.0 的面板会读 `~/.gemini/settings.json`，显示当前选的模型和已登录的认证类型，在面板里改模型只会写 `model.name` 这一个字段。Gemini CLI 没有单独的登录子命令，所以 Kaku 的重新登录动作就是直接启动 `gemini`。只有你在面板里改动时，Kaku 才会修改对应工具的设置。`Cmd + D` 分屏可以让 Gemini CLI 挨着一个 shell 或者另一个 agent。Kaku 的通知文档只写了 Claude Code 和 Codex，没有列 Gemini CLI。

## 常见问题

### Gemini CLI 现在还免费吗

对个人来说不再免费。Google 的公告在 2026 年 6 月 18 日终止了通过 Gemini Code Assist for individuals 的免费使用，README 的提示也把未付费档位的用户算在内。现在还能用的是付费的 Gemini 和 Gemini Enterprise Agent Platform API key。

### 用 Homebrew 装还需要 Node.js 吗

文档把 Node.js 20.0.0 或更新列为运行环境要求，Homebrew 的 formula 依赖 `node`，会替你装好，装完用 `gemini --version` 确认就行。

### Gemini CLI 的设置存在哪

用户设置在 `~/.gemini/settings.json`，项目设置在项目里的 `.gemini/settings.json`。

### 它能读我的 AGENTS.md 吗

可以，把它加进 `settings.json` 的 `context.fileName`。
