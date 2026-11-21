# Claude Code Remote: Three Ways to Reach a Running Session

> Anthropic's Remote Control, cloud sessions, and opening your Mac's terminal from another computer with kaku remote: what each needs and where it runs.

Published 2026-09-30 · Tw93 · https://kaku.fun/blog/claude-code-remote

There are three ways to keep working with Claude Code when you are away from the Mac it runs on. Anthropic's Remote Control lets you steer a session on your Mac from claude.ai/code or the Claude app on your phone. Cloud sessions move the work to Anthropic's machines, so nothing has to stay running at home. And `kaku remote` opens your Mac's terminal, every tab and split included, in a browser on another computer. Checked against Claude Code 2.1.302 in October 2026.

## The short version

| | Remote Control | Cloud session | kaku remote |
|---|---|---|---|
| Where Claude runs | Your Mac | Anthropic's machines | Your Mac |
| What you see | The Claude conversation | The Claude conversation | The whole terminal |
| Where you use it | claude.ai/code, the Claude app | claude.ai/code, the Claude app, the CLI | A browser on a computer or phone |
| What it needs | A paid Claude plan, the Mac awake | A paid Claude plan, a GitHub repository | Kaku running, the Mac awake |
| Other programs | No | No | Yes, any shell, editor or agent |

## Remote Control: the same session from your phone

Remote Control keeps Claude Code running on your Mac, with your files, tools and credentials, and lets you read and answer it from claude.ai/code or the Claude mobile app. Start a session with it turned on:

```bash
claude --remote-control
```

Or run `claude remote-control` in a project, and sessions you start from claude.ai/code or the app run there on your Mac; `claude remote-control --continue` reattaches to the last session started in that directory. It needs a paid Claude plan with a claude.ai login, since API keys are not supported, and the Mac has to stay awake and online for as long as you want to reach it.

It is the right choice when the work depends on your machine, such as a local database, a private network, or a build that only runs there, and all you want from the phone is to approve a step or send the next instruction.

## Cloud sessions: nothing has to stay on

A cloud session runs Claude Code on a virtual machine Anthropic provides, usually against a GitHub repository; `claude --cloud` run in a repository with no git remote, or one the Claude GitHub App is not installed on, uploads your local repository instead, though pull requests still need GitHub. You can start one from claude.ai/code or the app, or from the terminal:

```bash
claude --cloud "fix the flaky login test"
```

Because it does not run on your Mac, closing the laptop does not stop it. When you want to continue locally, `claude --teleport` pulls a cloud session into your terminal. The trade-off is the environment: the session sees the repository, not your machine, so anything that only exists locally is out of reach.

## kaku remote: the whole terminal in a browser

Both of the options above show you the Claude conversation. Sometimes you need the rest of the terminal too: the dev server log in the next split, a Codex session in another tab, or a plain shell to check something. `kaku remote` opens the terminal itself in a browser on another computer.

![A browser on another computer connects through a relay to Kaku on your Mac, with what you type and see encrypted from the browser to Kaku](https://kaku.fun/img/blog/remote-access-architecture.webp)

*The relay passes the connection along without being able to read it.*

Turn it on and pair a browser:

```bash
kaku remote
```

Kaku asks before turning remote access on, then prints a pairing link. Open it in the browser you want to pair, check that the page and the terminal show the same six-digit code, and press Enter in the terminal to let that browser in, or Ctrl+C to refuse it. A link works once and expires after 10 minutes, so run `kaku remote` again for each device. Once paired, bookmark the page to return to it without a new link.

What you type and what you see is encrypted between that browser and Kaku, and the relay in the middle only forwards it. Because a new browser needs that Enter at the Mac, a link that leaks through a screenshot or saved scrollback is useless on its own, and Kaku posts a notification when a paired browser connects again after 30 minutes or more away. Managing access stays on the Mac:

```bash
kaku remote devices        # list paired browsers
kaku remote revoke <id>    # remove one; its open sessions end within seconds
kaku remote off            # turn remote access off
kaku remote --reset        # remove every paired browser
```

The page works in a desktop browser and, since Kaku 0.29.0, has its own layout on a phone: tabs sit in a side drawer that shows each agent's topic and status, the key row gives you Esc, the arrow keys, Tab, Shift + Tab and Ctrl + C, and the keyboard comes up only from the keyboard key at the start of that row, so scrolling the terminal does not raise it. There is no QR code for pairing, so get the link onto the phone yourself, for example with AirDrop, and open it there. Like Remote Control, it reaches your actual Mac, so the Mac needs to be awake with Kaku running.

## Which one to use

- **Approving a step from your phone:** Remote Control.
- **Work that should continue with the laptop closed:** a cloud session.
- **Using the terminal from another computer, with agents and everything else in it:** `kaku remote`.

They also combine. A session started with `claude --remote-control` inside Kaku can be answered from the phone during the day and picked up in the browser at another desk in the evening.

## FAQ

### Can I use Claude Code from my phone?

Yes. Start the session with `claude --remote-control` on your Mac, then open it in the Claude app or at claude.ai/code. Cloud sessions can also be started and followed from the app. To use the whole terminal instead of one conversation, open a paired `kaku remote` page in the phone's browser, which has a phone layout since Kaku 0.29.0.

### Does Claude Code keep running if I close my laptop?

A local session, including one under Remote Control, stops when the Mac sleeps. A cloud session runs on Anthropic's machines and keeps going.

### Can I SSH into my Mac and run Claude Code instead?

Yes, if you have set up SSH and a way to reach the Mac. Run it inside tmux so the session survives a dropped connection. [Notifications](/blog/claude-code-notifications) can reach the terminal you connected from through tmux passthrough if the sender wraps the sequences, tmux permits them and that terminal supports the notification channel.

### Does kaku remote work with Codex and other CLI tools?

Yes. It shows the terminal itself, so whatever runs in a Kaku tab, whether Claude Code, Codex, an editor or a shell, appears in the browser.
