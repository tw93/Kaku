# tmux Alternative on Mac: Do You Still Need tmux?

> What tmux gives you that a terminal does not, what native splits and session restore already cover, and when to keep tmux for remote work.

Published 2026-10-01 · Tw93 · https://kaku.fun/blog/tmux-alternative-mac

If you work mostly on your own Mac, you may not need tmux anymore. Most people start it for three things: splitting one window into panes, keeping work alive when the terminal closes, and getting back to a session from somewhere else. Native splits cover the first. Session restore brings back the layout. To keep an interactive session running after the terminal closes, you can use a multiplexer such as tmux, locally or on a server. So the practical answer is to drop tmux for local splits if you like, and keep it for SSH sessions and jobs that must survive a terminal restart. This post explains where the line falls and how to set tmux up well if you keep it.

## What tmux actually does

tmux runs a server that owns your shells. The terminal is only a client that draws them. That design gives you three things:

1. **Splits and windows**: drawn inside one terminal window.
2. **Persistence**: close the terminal, or lose the SSH connection, and the shells keep running inside the tmux server. `tmux attach` brings them back exactly as they were, programs still running.
3. **Sharing**: attach from another terminal or [another machine](/blog/claude-code-remote) and see the same session.

## What a terminal now covers

**Splits.** iTerm2, Ghostty, WezTerm, Kitty and Kaku all have native splits on macOS. They are drawn by the terminal itself, so scrolling, selecting and copying behave normally, without tmux's copy mode.

**Coming back after a restart.** Some terminals bring back windows, tabs and splits after a restart. Kaku goes a step further and saves what was on screen.

![Kaku session restore: panes with agents and shells are captured in a snapshot every 60 seconds, survive a restart from an update or crash, and come back as restored text without re-running anything](https://kaku.fun/img/blog/session-restore-flow.webp)

*Restored panes show what was there; the programs themselves are not started again.*

When Kaku quits, it saves every window, its tabs, the split layout, each pane's working directory, and up to 1500 lines from each pane. While it runs, it also refreshes that snapshot every minute after the layout changes, so an unexpected quit still leaves a recent layout. On the next launch, the panes come back with their text, in their directories, with a fresh shell. Panes that were running Claude Code or Codex keep the agent's last screen, so you can see where each conversation stopped and resume it with `claude --continue` or `codex resume --last`. Before 0.28.0, restoring a session with a long scrollback could make Kaku quit about a minute later and again on every reopen; updating fixes it, with no need to turn restore off.

This is the important difference from tmux. Kaku restores what you saw, not the processes. A build that was running when the terminal quit is not running anymore. tmux keeps it running, because the process never belonged to the terminal.

**Agents.** For AI coding CLIs, the conversation is the state that matters, and both Claude Code and Codex save it themselves. A terminal that brings back the layout and the last screen gets you most of the way, and the agent's own resume command does the rest.

## When to keep tmux

- **Remote machines over SSH.** This is what tmux is for. Start long jobs and agent sessions inside tmux on the server, and a dropped connection costs you nothing.
- **Long processes on your own Mac** that must keep running across a terminal restart, such as a dev server you never want to restart.
- **Pairing in one session**, with two people attached to the same shell.
- **A workflow built on tmux keys and scripts** that you do not want to give up.

## Setting up tmux for Claude Code

If you do run Claude Code inside tmux, three settings make it behave:

```bash
# ~/.tmux.conf
set -g allow-passthrough on
set -s extended-keys on
set -as terminal-features 'xterm*:extkeys'
```

`allow-passthrough` lets sequences wrapped for tmux passthrough reach the outer terminal from a visible pane, and the other two let `Shift + Enter` arrive as its own key instead of plain Enter. Reload with `tmux source-file ~/.tmux.conf`. The [notification guide](/blog/claude-code-notifications) explains how those notifications reach macOS.

When tmux is installed, `kaku init` also adds one `source-file` line to `~/.tmux.conf` for a small managed file: it turns on the mouse, keeps the Kaku prompt in panes opened after you attach from Kaku, and makes `Shift` plus the scroll wheel enter tmux's scrollback. The three settings above are still yours to add.

## A middle path

You do not have to choose one. A common setup is native splits and session restore for everything local, and tmux only on the servers you SSH into. In Kaku, that means `Cmd + D` and `Cmd + Shift + D` for splits on the Mac, and `ssh server -t tmux new -A -s main` to land in the same remote session every time.

## FAQ

### Does Kaku replace tmux?

For local splits and getting your layout back after a restart, mostly yes. It does not keep processes running after the terminal quits, so keep tmux for SSH sessions and long-running jobs.

### Does tmux work in Kaku?

Yes. Kaku runs tmux like any terminal, and the managed tmux file adds mouse support and the Kaku prompt inside tmux sessions started from Kaku.

### How do I keep Claude Code running when SSH disconnects?

Start it inside tmux on the remote machine, for example `tmux new -A -s claude` and then `claude`. After reconnecting, `tmux attach -t claude` brings the session back.

### Can I turn off Kaku's session restore?

Yes, set `config.restore_previous_session = false` in `kaku.lua`, which turns off both saving and restoring.
