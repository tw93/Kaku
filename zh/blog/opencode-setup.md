# OpenCode 在 Mac 上怎么安装和配置

> 在 Mac 上安装 OpenCode，用 /connect 连模型服务商，/init 生成 AGENTS.md，配权限，以及在 Kaku 里怎么用。

发布于 2026-11-02 · Tw93 · https://kaku.fun/zh/blog/opencode-setup

OpenCode 是 [opencode.ai](https://opencode.ai) 出的开源终端编程 agent，在 Mac 上用官方脚本装好，进项目目录运行 `opencode`，用 `/connect` 连上一个模型服务商，再跑一次 `/init` 让它给仓库生成 `AGENTS.md`，就能开始用了。它和 Claude Code 最大的两点不同，一是不绑定某一家模型厂商，二是大多数操作默认直接放行，要它先问你得自己配。这篇讲的是 OpenCode 2，也就是项目 README 现在默认安装的版本，内容包括安装、配置文件、agent 和权限，以及它在 Kaku 里怎么用，命令按官方文档 2026 年 10 月的内容核对。

## 先确认是哪个 opencode

叫 opencode 的东西不止一个，这篇讲的是 opencode.ai 上的这个，仓库是 [github.com/anomalyco/opencode](https://github.com/anomalyco/opencode)，MIT 协议，旧的 `sst/opencode` 链接会跳到这个仓库。另有一个 Go 写的 `opencode-ai/opencode`，[2025 年 9 月已经归档](https://github.com/opencode-ai/opencode)，它的 README 说后续以 Crush 的名字继续开发，如果某篇教程让你从那个仓库安装，讲的就是老项目。

## 安装

[OpenCode 2 的文档](https://opencode.ai/v2/docs/)列了好几种方式，Mac 上常用的是这三种：

```bash
# 官方脚本
curl -fsSL https://opencode.ai/v2/install | bash

# Homebrew，用项目自己的 tap
brew install anomalyco/tap/opencode-v2

# npm
npm install -g @opencode/cli
```

README 推荐用 `anomalyco/tap` 这个 formula，Homebrew 核心仓库里的 `opencode` 更新得慢一些。OpenCode 1 还在发版，老教程里的 `opencode.ai/install`、`anomalyco/tap/opencode` 和 npm 上的 `opencode-ai` 装的都是它。两个版本用的都是 `opencode` 这个命令，只能留一个，V2 的安装脚本会直接替换 V1 的程序，文档也要求先卸掉用包管理器装的 V1。装完用 `opencode --version` 检查。

## 连接模型服务商

在项目目录里启动 OpenCode，然后输入 `/connect`：

```bash
cd ~/code/my-project
opencode
```

在终端界面里，`/connect` 会列出服务商，选一个按提示操作，通常是粘贴 API key，从 shell 里用 `opencode auth login` 效果一样，`opencode auth list` 能看到配了哪些。保存的 key 放在 OpenCode 的本地数据库里，默认是 `~/.local/share/opencode/opencode.db`，老版本存在 `auth.json` 里的 key 会在装上 V2 后导入进来。之后 `/models` 从已连接的服务商里选模型，`opencode models` 会列出可用的模型。

有些服务商提供浏览器登录，不用 key。比如 OpenAI，`/connect` 里除了 API key 还可以用 ChatGPT 账号登录。

OpenCode 自己也卖模型服务，一个是 OpenCode Go，团队测试过的编程模型的订阅，另一个是 OpenCode Console，都不是必须的。

## 终端界面

不带参数运行 `opencode` 就是终端界面，先学这几样：

- **`Shift + Tab`** 在主 agent `build` 和 `plan` 之间轮换，`/agents` 可以列出全部。
- **`@`** 在项目里模糊搜索文件，把选中的文件附到消息里。
- **空输入框开头打 `!`** 进入 shell 模式，命令的输出留在会话里。
- **`/undo` 和 `/redo`** 把一条消息连同它改的文件一起撤回，或者再恢复回来。恢复文件要求项目是 Git 仓库，不在仓库里的话 `/undo` 只能撤回对话。
- **`/sessions`** 列出并切换会话，`/new` 新开一个。默认的 leader 键是 `ctrl+x`，所以 `ctrl+x u` 是撤销，`ctrl+x m` 打开模型列表。

在 shell 里，`opencode run "你的提示词"` 不开界面直接跑一条提示，`opencode -c` 接着上一个会话继续。

## 用 AGENTS.md 写规则

在项目里运行 `/init`，OpenCode 会读一遍仓库，写出 `AGENTS.md`，代码回答不了的地方它会问你几个问题，文件已经存在就原地改进。文档建议把它提交进 Git，里面放的是构建命令和约定。

全局规则放在 `~/.config/opencode/AGENTS.md`，OpenCode 会加载它，再加上从当前目录往上到项目根目录的每一个 `AGENTS.md`，这些文件是合并使用的，不会互相替换。配置里的 `instructions` 还能再加文件，比如 `CONTRIBUTING.md`，或者 `docs/guidelines/*.md` 这样的通配。

从 Claude Code 过来的人要注意一点，OpenCode 2 只认 `AGENTS.md`，不再兜底读 `CLAUDE.md`，要么把规则挪进 `AGENTS.md`，要么在 `instructions` 里写上 `CLAUDE.md`。一份 `AGENTS.md` 可以同时给 OpenCode 和 [Codex](/zh/blog/codex-cli-setup) 用，Claude Code 仍然读它自己的 `CLAUDE.md`。

## 配置文件

OpenCode 会把几处来源的设置合并起来，而不是用一个替换另一个。常用的是全局的 `~/.config/opencode/opencode.json` 和项目里的 `opencode.json`，键冲突时项目文件覆盖全局文件，想写注释的话两处都可以用 `.jsonc` 后缀。

```json
{
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4-5",
  "permissions": [
    { "action": "edit", "resource": "*", "effect": "ask" },
    { "action": "shell", "resource": "*", "effect": "ask" }
  ]
}
```

`$schema` 这行让编辑器能做校验，`model` 是主模型，写成 `provider/model` 的形式。这里的模型名取自文档里的示例，换成 `opencode models` 实际列出的你自己服务商的模型。

## agent 和权限

自带两个主 agent，`build` 是默认的，跑工具不问你，`plan` 负责摸清代码和出方案，不能改项目文件，跑 shell 命令按你配的权限规则走。还有两个子 agent，主 agent 可以把活交给它们，`general` 做调研和多步骤任务，`explore` 只搜索和读代码，不改文件。自己写的话，在 `~/.config/opencode/agents/` 或 `.opencode/agents/` 里放一个 Markdown 文件，文件名就是 agent 名。思路和 [Claude Code 的 subagent](/zh/blog/claude-code-subagents) 是一样的。

每条权限规则有 `action`、`resource` 和 `effect` 三个字段，`effect` 取 `allow`、`ask` 或 `deny`。默认几乎都是放行，包括访问项目以外的路径，读 `.env` 文件会先问。更严格的写法是这样：

```json
{
  "permissions": [
    { "action": "*", "resource": "*", "effect": "ask" },
    { "action": "read", "resource": "*", "effect": "allow" },
    { "action": "shell", "resource": "git status *", "effect": "allow" },
    { "action": "external_directory", "resource": "*", "effect": "ask" }
  ]
}
```

最后一条命中的规则生效，所以宽泛的规则写在前面，例外写在后面。弹窗的回答有允许一次、一直允许（按项目保存）和拒绝，用 `opencode --auto` 启动会批准原本要询问的请求，显式的 `deny` 规则仍然有效。一直允许存下来的是工具提出的模式，不是这一次的原样请求，批准 `git status --short` 可能存下的是 `git status *`，存下来的批准永远压不过配置里的 `deny`，选拒绝则会把这个会话里其他还在等的请求一起拒掉。OpenCode 1 那种用 `permission` 和 `bash` 写的配置照样能加载，不过新写的规则按上面的格式更好读。

只针对某个 agent 的规则写在 `agents.<id>.permissions` 下，排在顶层规则后面，所以对那个 agent 来说它们说了算，自定义的 subagent 用的是它自己的规则，不是父 agent 规则的子集。shell 规则比对的是命令文本，OpenCode 从命令里推断会碰到哪些目录也只是尽力而为，所以文档建议给 shell 命令写一份短的放行清单，而不是想靠模式把每种危险命令都拦下来。

## 和 Claude Code、Codex 的区别

| | OpenCode | Claude Code | Codex CLI |
|---|---|---|---|
| 模型 | 多家服务商，用 `/connect` 选 | Anthropic 的模型 | ChatGPT 套餐或 OpenAI API key |
| 改文件和跑命令 | 默认放行，设成 `ask` 才问 | 默认模式下先问 | 由 `approval_policy` 和 `sandbox_mode` 决定 |
| 规则文件 | `AGENTS.md` | `CLAUDE.md` | 见 [Codex 配置那篇](/zh/blog/codex-cli-setup) |

OpenCode 是 MIT 协议，Codex CLI 是 Apache-2.0。选哪个通常看模型和审查习惯，想用一个终端工具对接好几家服务商就选 OpenCode，依赖 [hooks](/zh/blog/claude-code-hooks) 和 [plan mode](/zh/blog/claude-code-plan-mode) 的话就留在 Claude Code，这篇没有把它们逐项对应到 OpenCode 上。[Codex 和 Claude Code 的对比](/zh/blog/codex-vs-claude-code)讲了后两者。

## 在 Kaku 里用

OpenCode 是普通的终端程序，在 Kaku 里和在别的终端里一样能跑，Kaku 加的是外围的东西。`Cmd + D` 和 `Cmd + Shift + D` 分屏，适合一边跑 `build` 一边留一个 shell，`Cmd + Shift + Enter` 放大当前正在看的窗格，`Shift + Enter` 和 `Cmd + Enter` 在提示词里换行。关闭一个前台正在跑长任务的窗格时会先确认。重启之后 Kaku 会恢复标签和分屏，但不会留下 OpenCode 的画面和进程，全屏程序的最后一屏只给 Claude Code 和 Codex 保留，所以要重新启动 OpenCode，用 `opencode -c` 接上。

`kaku ai` 能配置 Claude Code、Codex、Gemini CLI、Kimi Code 这些工具，0.25.0 版里没有 OpenCode，它的服务商和配置仍然留在 OpenCode 自己的文件里。

## 常见问题排查

- **装错了项目或版本。** 看 `opencode --version`，确认安装命令来自 opencode.ai 或 `anomalyco` 仓库。
- **模型列表是空的。** `/models` 只显示你连接过的服务商，用 `opencode auth list` 看存了哪些。
- **改文件没有先问。** 这是默认行为，加上面的 `permissions` 规则，或者用 `Shift + Tab` 切到 `plan`。
- **`/undo` 只撤回了提示词，文件没变回去。** 恢复文件要求是 Git 仓库。
- **`CLAUDE.md` 没被读到。** OpenCode 2 只认 `AGENTS.md`，在 `instructions` 里写上 `CLAUDE.md`，或者把规则挪过去。

## 常见问题

### OpenCode 免费吗？

软件是 MIT 协议，但你连接的服务商照常收费，或者用服务商提供的账号登录。

### OpenCode 的配置文件在 Mac 的哪里？

用户级是 `~/.config/opencode/opencode.json`，项目级是项目里的 `opencode.json`，保存的服务商 key 在 `~/.local/share/opencode/opencode.db`。

### OpenCode 会读 CLAUDE.md 吗？

OpenCode 2 不会自己去读，要在 `opencode.json` 的 `instructions` 里加上它，或者把内容挪进 `AGENTS.md`。

### 怎么接着上一个会话继续？

在 shell 里用 `opencode -c`，或者在界面里用 `/sessions`。
