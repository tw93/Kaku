---
name: kaku
description: Drive Kaku panes from a coding agent running inside Kaku - split a pane for a dev server or tests, type into another pane, read its screen, and wait for another agent or command to finish. Use when TERM_PROGRAM is Kaku and WEZTERM_PANE is set. Not for other terminals.
---

# Kaku for coding agents

You are running inside Kaku when `TERM_PROGRAM` is `Kaku`; `WEZTERM_PANE` holds your own pane id. Every command below goes through `kaku cli` and works on panes of the Kaku window you are in. Never close or type into a pane you did not start unless the user asks.

## Run something beside yourself

```sh
pane=$(kaku cli split-pane --bottom --percent 30 -- npm run dev)   # prints the new pane id
kaku cli split-pane --right -- claude                              # a second agent
```

A pane closes when its command ends, so send output you need later to a file:

```sh
pane=$(kaku cli split-pane --bottom -- sh -c 'go test ./... > /tmp/kaku-test.log 2>&1')
kaku cli wait --pane-id "$pane" --for exit --timeout 10m
tail -n 80 /tmp/kaku-test.log
```

## Type into a pane and read it

```sh
kaku cli send-text --pane-id 3 --no-paste $'npm run migrate\r'   # \r presses Enter
kaku cli get-text --pane-id 3                                     # what is on screen now
kaku cli get-text --pane-id 3 --start-line -200                   # with 200 rows of scrollback
kaku cli list --format json                                       # every pane, its title, cwd and state
```

## Wait for another agent

Claude Code (2.1.295 and later) and Codex (0.163 and later) report their state to Kaku (OSC 7501): `working`, `blocked` (waiting for the user), `done`, `error` or `idle`. `kaku cli list --format json` shows it as `program_status`.

```sh
kaku cli send-text --pane-id 5 --no-paste $'review the changes in src/auth\r'
kaku cli wait --pane-id 5 --for done,blocked,error --next --timeout 20m
kaku cli get-text --pane-id 5 --start-line -100
```

- `wait` prints the state that ended it and exits 0, exits 2 on `--timeout`, and exits 1 when it cannot reach Kaku or the pane does not exist.
- `--next` only counts a state once the pane has left the one it was in, so a `done` left from the agent's previous turn does not end the wait before the agent starts working again.
- `exit` in `--for` means the pane closed, including before `wait` started.
- A program that never reports a state (a plain shell, a build) only ends a wait with `exit`.

## Watch every pane

```sh
kaku cli events                      # "pane 5 claude: working -> blocked", until interrupted
kaku cli events --for blocked --once # stop at the first pane that needs the user
kaku cli events --json               # {"pane_id":5,"title":"claude","from":"working","to":"blocked"}
kaku cli events --pane-id 5          # one pane; ends when it closes
```

When you are blocked and need the user, say so in your own pane: Kaku already marks the tab and notifies them.
