# Claude Code Cost: Pricing, /usage, ccusage and Ways to Cut It

> How Claude Code is billed on plans and API keys, where /usage and ccusage show spend, what drives cost, and the habits that cut it.

Published 2026-10-23 · Tw93 · https://kaku.fun/blog/claude-code-cost

Claude Code is billed one of two ways. On a Pro, Max, Team or Enterprise plan your usage draws from a plan allowance, and on an API key, the Claude Console or a cloud provider you pay per token. Either way the cost follows one thing, how much context Claude has to read on every request. Check where you stand with `/usage` (`/cost` is an alias), keep sessions short with `/clear`, and match the model to the task. Anthropic's costs page puts the average across enterprise deployments at around $13 per developer per active day, with 90% of users below $30.

## How you are billed

The [costs page](https://code.claude.com/docs/en/costs), read on 19 October 2026, starts with the split. Claude Code charges by API token consumption, and for subscription plan pricing it sends you to [claude.com/pricing](https://claude.com/pricing). That pricing page lists Claude Code under the Pro and Max plans and as included on Team and Enterprise, but not on Free. This post copies no plan prices, because they change and that page is the source.

What differs is where the meter is:

| Your setup | What is metered | Where you see spend |
|---|---|---|
| Pro or Max | A plan allowance shared with claude.ai | Plan usage bars in `/usage` |
| Team or Enterprise | A per-seat allowance on a rolling five-hour window and a weekly window | Spend report in org analytics |
| Claude Console (API key) | Tokens, billed to your organization | Console usage page |
| Bedrock, Google Cloud's Agent Platform, Foundry | Tokens, billed to your cloud account | Your cloud billing console |

On a plan, you can also turn on usage credits to keep working past the allowance. `/usage-credits` opens the settings for them, or, on a Team or Enterprise seat without billing access, sends a request to your admins.

## Where to see what you spent

`/usage` is the command to know. Its Session block shows token counts per model and a dollar figure for the current session. Claude Code works that figure out locally from token counts at list price, so the docs call it an estimate, and the Usage page in the Claude Console is the authoritative number for API billing. On Pro and Max the dollar figure is not what you are billed, since usage is included in the subscription. Those plans get plan usage bars and a breakdown instead, with recent usage attributed to skills, subagents, plugins and individual MCP servers. Press `d` or `w` to switch between 24 hours and 7 days. The breakdown also flags behaviors such as long context or cache misses once one accounts for 10% or more of recent usage. Its figures are approximate, computed from session history on this machine, so usage from other devices or claude.ai is not in them.

The session totals reset when `/clear` starts a new session. Newer versions also add a `Prompt cache (main)` line that shows what share of input came from cache and whether the cache is warm, which helps when a session suddenly feels expensive.

`/insights` answers a different question. It writes an HTML report to `~/.claude/usage-data/report.html` about how you work, with friction points and suggestions, and its analysis uses tokens from your plan or API usage.

If you want the numbers on screen all the time, the [status line](/blog/claude-code-statusline) can show cost and context usage continuously. Kaku's `kaku ai` panel also shows plan usage for Claude Code without installing a status line.

## ccusage for history

`/usage` covers the current session and recent plan usage. For a history by day, month or session, [ccusage](https://github.com/ccusage/ccusage) reads the usage data Claude Code leaves on your machine and prints reports:

```bash
npx ccusage@latest            # all detected sources by day
npx ccusage claude daily      # Claude Code only
npx ccusage monthly --json
npx ccusage blocks            # Claude Code 5-hour billing windows
npx ccusage claude daily --instances   # grouped by project
```

Its README says it analyzes token usage and costs from local data, reports cache creation and cache read tokens separately, and has an `--offline` mode that uses cached pricing. It is a third-party project, not an Anthropic tool, so treat its dollar figures the way you treat `/usage`, as an estimate from list prices.

## What drives the cost

Token cost scales with context size. Claude Code sends the whole conversation with every request, and every tool call is another request carrying the results so far. Prompt caching makes re-reading that history cheaper, not free, which is why a one-line question in a session that has been open all day still draws usage for the whole conversation.

The costs page lists the usual reasons usage climbs:

- **Long context.** Stale conversation from three tasks ago is sent again on every message.
- **Cache misses.** The first message after a break longer than the cache lifetime reprocesses the full context. The lifetime is one hour on a subscription, dropping to five minutes once you draw on usage credits, and five minutes on an API key or cloud provider by default.
- **Model choice.** The docs say Sonnet handles most coding tasks and costs less than Opus, and that unexpectedly high API spend usually traces back to long sessions that were never cleared or to Opus left as the default. Each model also has its own prompt cache, so switching with `/model` partway through a conversation makes the next request read the whole history with no cache hits; change models at the start of a task or right after `/clear`.
- **Extended thinking.** Thinking tokens are billed as output tokens, and the default budget can be tens of thousands of tokens per request.
- **Subagents and teams.** Each subagent sends its own requests on top of the main conversation. [Agent teams](/blog/claude-code-agent-teams) run separate instances, and the docs say they use about 7 times more tokens than a standard session when teammates run in plan mode.
- **Scheduled and idle work.** Scheduled tasks, cross-session messages and goal check-ins send your full context even while you are away.
- **MCP tools.** Tool definitions are deferred by default, so only names and server instructions enter context until a tool is used, but `/context` shows what is taking space.

## Ways to cut it

These are the habits the costs doc recommends.

1. **Clear between tasks.** Run `/clear` when you switch to unrelated work. Use `/rename` first and `/resume` later if you want to come back. `/clear` costs nothing, while `/compact` has to read the whole conversation it summarizes.
2. **Compact with instructions.** `/compact Focus on code samples and API usage` tells Claude what to keep. You can also put a "Compact instructions" section in `CLAUDE.md`.
3. **Pick the model for the job.** Switch with `/model` or set a default in `/config`. For simple subagent tasks, put `model: haiku` in the [subagent file](/blog/claude-code-subagents).
4. **Lower the effort on easy tasks.** `/effort` or `/model` adjusts it, and `MAX_THINKING_TOKENS=8000` caps the budget on models with a fixed thinking budget. Adaptive-reasoning models ignore that budget, so use effort levels there.
5. **Trim MCP servers.** Run `/mcp` and disable what you are not using, and prefer CLI tools such as `gh` when they exist, since they add no per-tool listing. See [MCP servers](/blog/claude-code-mcp-servers).
6. **Filter output with hooks.** A PreToolUse [hook](/blog/claude-code-hooks) can cut a test run down to the failures before Claude sees it. The docs' example of a hook that greps a long log for errors takes context from tens of thousands of tokens to hundreds.
7. **Keep CLAUDE.md small.** It loads at session start. The docs suggest moving workflow-specific instructions into [skills](/blog/claude-code-skills), which load on demand, and keeping CLAUDE.md under 200 lines.
8. **Be specific.** "Add input validation to the login function in auth.ts" triggers far less scanning than "improve this codebase".
9. **Plan first on big tasks.** Shift+Tab cycles to [plan mode](/blog/claude-code-plan-mode), which prevents expensive rework when the first direction is wrong. Escape stops a wrong turn early, and `/rewind` restores a checkpoint.

## Limits for teams

On Team and Enterprise plans the seat allowance is the default ceiling. Admins can turn on usage credits and set spend limits at the organization, group or member level, and the spend report in org analytics shows estimated spend per user and per model with CSV export. Usage inside the seat allowance is not metered in dollars.

On the Console, spend limits are set per workspace, and the first sign-in creates a workspace called Claude Code for tracking. The docs give per-user rate limit recommendations by team size, from 200k to 300k tokens per minute per user for 1 to 5 users down to 10k to 15k for 500 or more. On cloud providers, Claude Code does not send metrics back to Anthropic, so per-user numbers come from OpenTelemetry or an LLM gateway.

Users hit different walls, and the messages differ. "You've hit your session limit" or "weekly limit" is a seat window shared across models, so switching models does not help. A spend limit message means usage credits hit a cap an admin set. A context or auto-compact warning is not a usage limit at all.

## FAQ

### Is Claude Code free?

Not on its own. The pricing page lists it under the paid plans, and with an API key you pay per token. Check [claude.com/pricing](https://claude.com/pricing) for current plans.

### Does /usage show my real bill?

For API billing it is an estimate at list price, and the Console usage page is authoritative. On Pro and Max the session dollar figure is not what you are charged, so read the plan usage bars.

### Why did one short question use so much?

Because the whole conversation travels with it. After a break longer than the cache lifetime the full context is reprocessed too. Run `/clear` between tasks.
