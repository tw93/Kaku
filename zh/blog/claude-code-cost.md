# Claude Code 费用：计费方式、/usage、ccusage 和省钱做法

> Claude Code 的订阅与 API 计费区别，/usage 和 ccusage 怎么看花费，费用从哪来，以及怎么省。

发布于 2026-10-23 · Tw93 · https://kaku.fun/zh/blog/claude-code-cost

Claude Code 有两种计费方式，用 Pro、Max、Team 或 Enterprise 订阅时，用量从套餐额度里扣，用 API key、Claude Console 或云厂商时，按 token 付费。不管哪种，费用都跟着同一件事走，也就是每次请求 Claude 要读多少上下文。用 `/usage` 看现状（`/cost` 是它的别名），任务之间用 `/clear` 把会话切短，再按任务挑模型，就能省下大部分。Anthropic 的费用页给的数字是，企业部署里平均每个开发者每个活跃日大约 13 美元，90% 的用户低于 30 美元。

## 怎么计费

[费用文档](https://code.claude.com/docs/en/costs)（2026 年 10 月 19 日读的）开头先把两种方式分开，Claude Code 按 API token 消耗计费，订阅套餐的价格则指向 [claude.com/pricing](https://claude.com/pricing)。那个价格页里，Claude Code 列在 Pro 和 Max 套餐下，Team 和 Enterprise 也包含，Free 没有。这里不抄具体金额，因为会变，以那个页面为准。

区别在计量的位置：

| 你的方式 | 计量什么 | 在哪看花费 |
|---|---|---|
| Pro 或 Max | 与 claude.ai 共用的套餐额度 | `/usage` 里的套餐用量条 |
| Team 或 Enterprise | 每个席位的额度，按滚动五小时窗口和每周窗口计 | 组织分析里的花费报告 |
| Claude Console（API key） | token，记在你的组织账上 | Console 的用量页 |
| Bedrock、Google Cloud 的 Agent Platform、Foundry | token，记在云账号上 | 云厂商的账单控制台 |

订阅用户还可以打开 usage credits，额度用完后继续干活，`/usage-credits` 会打开对应的设置页，Team 或 Enterprise 里没有账单权限的成员运行它，则是给管理员发一个申请。

## 在哪里看花了多少

最该记住的是 `/usage`。它的 Session 部分按模型列出 token 数，并给出当前会话的美元数。这个数是 Claude Code 在本地按标价从 token 数算出来的，文档自己也说是估算，API 计费以 Claude Console 的 Usage 页为准。Pro 和 Max 的用量包含在订阅里，所以那个美元数不是你实际被扣的钱，这两种套餐看的是套餐用量条和明细，里面把近期用量归到 skill、subagent、插件和具体的 MCP 服务器上，按 `d` 或 `w` 可以在 24 小时和 7 天之间切换。明细里还会标出长上下文、缓存未命中这类行为，只要其中一项占到近期用量的 10% 以上。这些数字是根据这台机器上的会话记录估算的，别的设备和 claude.ai 上用掉的量不在里面。

`/clear` 开新会话时，会话合计会清零。较新的版本还会多一行 `Prompt cache (main)`，显示输入里有多少来自缓存、缓存现在是否还热，会话突然变贵时看它很有用。

`/insights` 回答的是另一个问题，它在 `~/.claude/usage-data/report.html` 写一份 HTML 报告，讲你是怎么用的，有哪些卡点和建议，分析本身也会消耗你套餐或 API 的 token。

想让数字一直挂在屏幕上，可以用[状态栏](/zh/blog/claude-code-statusline)持续显示花费和上下文用量。Kaku 的 `kaku ai` 面板也会显示 Claude Code 的套餐用量，不需要装状态栏。

## 用 ccusage 看历史

`/usage` 管当前会话和近期套餐用量，想按天、按月、按会话看历史，可以用 [ccusage](https://github.com/ccusage/ccusage)，它读 Claude Code 留在你机器上的用量数据并输出报表：

```bash
npx ccusage@latest            # 所有检测到的来源，按天
npx ccusage claude daily      # 只看 Claude Code
npx ccusage monthly --json
npx ccusage blocks            # Claude Code 的 5 小时计费窗口
npx ccusage claude daily --instances   # 按项目分组
```

它的 README 说自己从本地数据分析 token 用量和成本，缓存创建和缓存读取的 token 分开统计，还有用预先缓存的价格数据、不需要联网的 `--offline` 模式。它是第三方项目，不是 Anthropic 的工具，所以它给的美元数和 `/usage` 一样，当作按标价算的估算。

## 费用从哪来

token 成本随上下文大小增长。Claude Code 每次请求都会带上整段对话，每次工具调用又是一次带着已有结果的新请求，prompt 缓存让重读这些历史更便宜，但不是免费，所以在一个开了一整天的会话里问一句话，也要为整段对话付用量。

费用文档列出的常见原因有这些：

- **上下文太长。** 三个任务之前的旧对话，每条消息都会再发一遍。
- **缓存未命中。** 停顿超过缓存有效期之后的第一条消息，要把完整上下文重新处理一遍，订阅用户的有效期是一小时，开始消耗 usage credits 后降到五分钟，API key 和云厂商默认是五分钟。
- **模型选择。** 文档说 Sonnet 能应付大多数编码任务，也比 Opus 便宜，API 或云上出乎意料的高花费，通常是会话长期没清理，或者默认模型一直是 Opus。每个模型的 prompt cache 也是各自独立的，对话进行到一半用 `/model` 切换，下一次请求会把整段历史重新读一遍，一点缓存都命中不了，所以要换模型，最好在任务开始时或者刚 `/clear` 完就换。
- **扩展思考。** 思考 token 按输出 token 计费，默认预算每次请求可能有几万 token。
- **subagent 和团队。** 每个 subagent 在主对话之外各自发请求，[agent teams](/zh/blog/claude-code-agent-teams) 是独立的实例，文档说队友在计划模式下大约比普通会话多用 7 倍 token。
- **定时任务和空闲时的活动。** 定时任务、跨会话消息和 goal 检查，即使你不在也会带着完整上下文发请求。
- **MCP 工具。** 工具定义默认延迟加载，用到之前只有工具名和服务器说明进上下文，`/context` 能看到什么在占空间。

## 省钱的做法

下面是费用文档推荐的习惯。

1. **任务之间 `/clear`。** 换到无关的工作就 `/clear`，想以后回来就先 `/rename`，再用 `/resume`。`/clear` 不花钱，而 `/compact` 要读完它所总结的整段对话。
2. **带说明地 compact。** `/compact Focus on code samples and API usage` 告诉 Claude 该保留什么，也可以在 `CLAUDE.md` 里写一段 Compact instructions。
3. **按任务选模型。** 用 `/model` 切换，或在 `/config` 里设默认值，简单的 subagent 任务在 [subagent 文件](/zh/blog/claude-code-subagents)里写 `model: haiku`。
4. **简单任务降低 effort。** `/effort` 或 `/model` 可以调，固定思考预算的模型上还可以用 `MAX_THINKING_TOKENS=8000` 封顶，自适应推理的模型会忽略这个预算，在那些模型上改用 effort。
5. **精简 MCP 服务器。** 运行 `/mcp` 禁用不用的，有 `gh` 这类命令行工具就优先用它，因为它不会增加每个工具的列表，见 [MCP 服务器](/zh/blog/claude-code-mcp-servers)。
6. **用 hook 过滤输出。** PreToolUse 的 [hook](/zh/blog/claude-code-hooks) 可以在 Claude 看到之前，把一次测试输出过滤到只剩失败项，文档里用 hook 从长日志里 grep 错误的例子，把上下文从几万 token 降到了几百。
7. **CLAUDE.md 保持精简。** 它在会话开始时加载，文档建议把特定流程的说明挪进按需加载的 [skill](/zh/blog/claude-code-skills)，CLAUDE.md 控制在 200 行以内。
8. **提示要具体。** 让 Claude 给 auth.ts 里的登录函数加输入校验，比让它改进这个代码库触发的扫描少得多。
9. **大任务先计划。** Shift+Tab 切到[计划模式](/zh/blog/claude-code-plan-mode)，能避免第一步方向错了之后的昂贵返工，Escape 可以立刻叫停，`/rewind` 恢复到检查点。

## 团队的限额

Team 和 Enterprise 计划里，席位额度是默认上限，管理员可以打开 usage credits，并在组织、分组或成员层面设花费上限，组织分析里的花费报告按用户和模型给出估算花费，可以导出 CSV，席位额度之内的用量不按美元计量。

Console 上，花费上限按 workspace 设置，第一次登录会自动建一个叫 Claude Code 的 workspace 用来追踪。文档还按团队规模给了每用户的速率限制建议，1 到 5 人是每用户每分钟 20 万到 30 万 token，500 人以上降到 1 万到 1.5 万。云厂商上 Claude Code 不会把指标回传给 Anthropic，按人统计要靠 OpenTelemetry 或 LLM 网关。

用户撞到的墙不一样，提示也不同。看到 You've hit your session limit 或 weekly limit，说明撞到的是跨模型共享的席位窗口，换模型没用，花费上限的提示说明 usage credits 撞到了管理员设的上限，上下文或 auto-compact 警告则根本不是用量限制。

## 常见问题

### Claude Code 免费吗

单独用不免费。价格页把它列在付费套餐下，用 API key 则按 token 付费，当前套餐看 [claude.com/pricing](https://claude.com/pricing)。

### /usage 显示的是我真实的账单吗

API 计费上它是按标价的估算，以 Console 用量页为准，Pro 和 Max 上会话的美元数不是实际扣费，要看套餐用量条。

### 为什么一个很短的问题也用了很多

因为整段对话都跟着它一起发，停顿超过缓存有效期后，完整上下文还要重新处理一遍，任务之间记得 `/clear`。
