# Modern CLI Tools for Mac: ripgrep, fd, bat, eza, zoxide, Atuin, delta

> What seven Rust-era tools replace, the Homebrew install, one useful command and config each, and what Kaku installs or wires for you at 0.25.0.

Published 2026-11-05 · Tw93 · https://kaku.fun/blog/modern-cli-tools-mac

Seven small tools replace the defaults you use most in a Mac terminal: ripgrep for `grep -r`, fd for `find`, bat for `cat`, eza for `ls`, zoxide for `cd`, Atuin for Ctrl + R history and delta for `git diff`. Install them in one line with `brew install ripgrep fd bat eza zoxide atuin git-delta`, then wire in only the ones that fix something that annoys you today. In zsh, delta is the only one of them `kaku init` offers to install, it asks first, and git's pager setting is still yours to add.

## What each tool replaces

| Tool | Replaces | The one thing it does better |
|---|---|---|
| ripgrep (`rg`) | `grep -r` | Respects `.gitignore` and skips hidden and binary files by default |
| fd | `find` | Short syntax, also honors `.gitignore` |
| bat | `cat` | Syntax highlighting and git change markers |
| eza | `ls` | Colors, git status column, tree view |
| zoxide | `cd` | Jumps to a directory by a fragment of its name, ranked by use |
| Atuin | Shell history | SQLite history with exit codes and directories, full-screen search |
| delta | git's pager | Readable diffs with syntax colors and line numbers |

None of them is required, and your scripts should keep calling `grep`, `find` and `cat`, because these tools differ from the originals in flags and in what they skip. Use them interactively.

## ripgrep

ripgrep searches the current directory recursively for a regex. By default it respects gitignore rules and skips hidden files, directories and binary files, so a search in a repository skips `.git` and a gitignored `node_modules`. `rg -uuu` turns all of that filtering off.

```bash
brew install ripgrep
rg 'TODO' -n              # matches with line numbers
rg -tpy 'import requests' # only Python files
```

The `-t` flag limits a search to a file type, and `-T` excludes one. When a search returns nothing you expected, the usual cause is that the file is ignored or hidden, so retry with `-uuu` before assuming the pattern is wrong. Each `-u` removes one layer: `-u` stops honoring `.gitignore`, `-uu` adds hidden files and `-uuu` binary files, so the smallest one that finds the file tells you why it was skipped, and `--debug` explains why a particular file was ignored. ripgrep reads no config file on its own; flags you want every time go one per line in a file that `RIPGREP_CONFIG_PATH` points to.

## fd

fd finds entries by name and ignores patterns from your `.gitignore` by default.

```bash
brew install fd
fd -e md                  # every Markdown file
fd -H -E .git config      # include hidden files, exclude .git
fd -e jpg -x convert {} {.}.png
```

`-e` filters by extension, `-H` includes hidden files and `-x` runs a command for each result, in parallel. Shell aliases and functions cannot be used with `-x`. The search pattern is a regular expression matched against the file name only, not a glob: use `-e md` for an extension, `-g '*.md'` for a glob, and `-p` to match against the full path. fd also pairs well with fzf as a file source, which [the fzf post](/blog/fzf-zsh-mac) shows.

## bat

bat prints files with syntax highlighting and shows git modifications in the gutter.

```bash
brew install bat
bat src/main.rs
bat -p src/main.rs        # plain: no line numbers or markers, safe to copy from
```

Line numbers and git markers make copying from the output awkward, which is what `-p` is for. Choose a theme with `export BAT_THEME="TwoDark"`, and list the options with `bat --list-themes`. bat also works as a previewer for fzf, using `fzf --preview "bat --color=always --style=numbers --line-range=:500 {}"`.

## eza

eza is a maintained replacement for `ls`.

```bash
brew install eza
eza -l --git --group-directories-first
eza --tree -L 2
```

`-l` gives the long view, `--git` adds each file's git status in that view, `--tree` recurses as a tree and `-L` limits the depth. `--icons` adds icons next to names. A common setup is `alias ls='eza'` in your interactive config. Scripts run in their own shell without your aliases, so they keep calling the real `ls`.

## zoxide

zoxide remembers the directories you visit and ranks them, so a fragment of a name is enough.

```bash
brew install zoxide
eval "$(zoxide init zsh)"   # in ~/.zshrc
z proj                      # jump to the best match for "proj"
zi proj                     # pick interactively (needs fzf)
```

The README says to put the init line at the end of your config. `zoxide import zsh-z` brings over an existing zsh-z database, and `--cmd j` makes the commands `j` and `ji` if you want a different name. It only knows directories you have already visited, so the first `z` for a new project fails until you `cd` there once.

## Atuin

Atuin replaces your shell history with a SQLite database that also records exit code, working directory, host and duration. It rebinds Ctrl + R and the Up arrow to a full-screen search, and you can filter by session, directory or globally.

```bash
brew install atuin
echo 'eval "$(atuin init zsh)"' >> ~/.zshrc
atuin import auto
atuin search --exit 0 --after "yesterday 3pm" make
```

Sync is optional and end-to-end encrypted, and you can skip it. Atuin does not replace your old history file, so `atuin import auto` is how existing history comes across. If you do not want a cloud account at all, the docs cover an offline setup and a self-hosted server.

## delta

delta is a pager for git, diff and grep output. The Homebrew package is `git-delta`, and the command is `delta`. The project's recommended `~/.gitconfig`:

```ini
[core]
    pager = delta
[interactive]
    diffFilter = delta --color-only
[delta]
    navigate = true
    dark = true
[merge]
    conflictStyle = zdiff3
```

`navigate` lets `n` and `N` jump between files in a diff. For how delta works inside the git UI, see [Lazygit and delta](/blog/lazygit-and-delta), which covers the lazygit side that needs its own setting.

## What Kaku installs and wires

Kaku's shell setup is narrower than this list, and it is worth knowing exactly what it does. In version 0.25.0, `kaku init` asks before installing missing optional tools through Homebrew: Starship, git-delta, Lazygit and Yazi. zoxide is installed only when the target shell is fish, and Kaku's fish setup then runs `zoxide init fish | source` if it finds the command. ripgrep, fd, bat, eza and Atuin are not installed or configured by Kaku. Installing delta does not change git's settings either: Kaku 0.25.0 writes no `core.pager`, so add the `[core]` and `[interactive]` lines above yourself before `git diff` pages through delta.

For zsh, Kaku bundles zsh-z instead of zoxide, so `z <dir>` already works in a stock Kaku shell. Kaku checks for zoxide's functions at the moment its own line runs and skips zsh-z only if they already exist. So for zoxide to take over, its init line has to sit above the Kaku line that `kaku init` appends to the end of `.zshrc`, not at the very end as zoxide's README suggests; below the Kaku line, zsh-z still loads. Check with `type z`. Kaku also binds Up and Down to prefix history search, and it skips that binding when another history tool, Atuin among them, already owns Up at that point in the file. [zsh without Oh My Zsh](/blog/zsh-without-oh-my-zsh) covers the rest of the bundle, and [Yazi](/blog/yazi-file-manager) uses fd, ripgrep and zoxide when they are installed.

## FAQ

### Will these break my scripts?

Not if the scripts keep calling the originals. Use aliases like `alias ls=eza` only in interactive config, and note that `rg` and `fd` skip ignored files, so they return different results from `grep -r` and `find`.

### Do I need all seven?

No. ripgrep and delta pay off fastest for developers, zoxide for anyone who jumps between projects, and Atuin when you search history often. bat, eza and fd are comfort upgrades.

### zoxide or zsh-z?

Both rank directories by use. zsh-z is what Kaku bundles for zsh, zoxide is a standalone binary that also works in fish and other shells. Switching is one init line, placed above the Kaku line, and `zoxide import zsh-z`.

### Does Atuin send my history anywhere?

Only if you register and sync. Synced history is end-to-end encrypted according to the README, and offline use is supported.
