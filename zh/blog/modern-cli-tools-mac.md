# Mac 命令行工具推荐：ripgrep、fd、bat、eza、zoxide、Atuin、delta

> 七个常用命令行工具各自替换什么、怎么用 Homebrew 安装，以及 Kaku 0.25.0 实际装了哪些。

发布于 2026-11-05 · Tw93 · https://kaku.fun/zh/blog/modern-cli-tools-mac

七个小工具可以替换 Mac 终端里最常用的几个默认命令，ripgrep 对应 `grep -r`，fd 对应 `find`，bat 对应 `cat`，eza 对应 `ls`，zoxide 对应 `cd`，Atuin 对应 Ctrl + R 的历史搜索，delta 对应 `git diff`。一行 `brew install ripgrep fd bat eza zoxide atuin git-delta` 全部装上，再只挑今天正让你不舒服的那几个去配置。在 zsh 里，`kaku init` 只会提出替你装其中的 delta，装之前会先问你，git 的 pager 设置仍然要你自己加。

## 各自替换什么

| 工具 | 替换 | 最大的不同 |
|---|---|---|
| ripgrep（`rg`） | `grep -r` | 默认遵守 `.gitignore`，跳过隐藏文件和二进制文件 |
| fd | `find` | 语法更短，同样遵守 `.gitignore` |
| bat | `cat` | 语法高亮，显示 git 改动标记 |
| eza | `ls` | 有颜色，有 git 状态列，有树状视图 |
| zoxide | `cd` | 敲目录名的一段就能跳过去，按使用频率排序 |
| Atuin | shell 历史 | 带退出码和目录的 SQLite 历史，全屏搜索 |
| delta | git 的 pager | 带语法颜色和行号的可读 diff |

这些都不是必需的，你的脚本也应该继续调用 `grep`、`find` 和 `cat`，因为这些新工具在参数和跳过的内容上都和原版不同，交互时用就好。

## ripgrep

ripgrep 在当前目录里递归搜索一个正则，默认遵守 gitignore 规则，跳过隐藏的文件、目录和二进制文件，所以在仓库里搜索会跳过 `.git` 和被 gitignore 的 `node_modules`。`rg -uuu` 会把这些过滤全部关掉。

```bash
brew install ripgrep
rg 'TODO' -n              # 带行号的匹配
rg -tpy 'import requests' # 只搜 Python 文件
```

`-t` 把搜索限制在某种文件类型，`-T` 则排除它。搜不到你预期的结果时，多半是文件被忽略或被当成隐藏文件了，先用 `-uuu` 再试一次，再怀疑正则写错。每多一个 `-u` 就少一层过滤，`-u` 不再看 `.gitignore`，`-uu` 再加上隐藏文件，`-uuu` 连二进制文件也搜，从哪一档开始能搜到，就知道文件是被哪一层挡住的，`--debug` 也会说明某个文件为什么被忽略。ripgrep 不会自己去找配置文件，想每次都带上的参数，要写进 `RIPGREP_CONFIG_PATH` 指向的文件里，一行一个。

## fd

fd 按名字查找条目，默认忽略你 `.gitignore` 里的规则。

```bash
brew install fd
fd -e md                  # 所有 Markdown 文件
fd -H -E .git config      # 包含隐藏文件，排除 .git
fd -e jpg -x convert {} {.}.png
```

`-e` 按扩展名过滤，`-H` 包含隐藏文件，`-x` 对每个结果并行执行一条命令，shell 的别名和函数不能用在 `-x` 里。fd 的搜索模式是正则，默认只和文件名比，不是 glob，按扩展名找用 `-e md`，真要 glob 就写 `-g '*.md'`，想连路径一起匹配就加 `-p`。fd 也很适合给 fzf 当文件来源，[fzf 那篇](/zh/blog/fzf-zsh-mac)里有写法。

## bat

bat 带语法高亮地打印文件，并在左侧显示 git 改动。

```bash
brew install bat
bat src/main.rs
bat -p src/main.rs        # 纯文本：没有行号和标记，可以放心复制
```

行号和 git 标记会让复制输出变得麻烦，`-p` 就是为这个准备的。用 `export BAT_THEME="TwoDark"` 选主题，`bat --list-themes` 列出所有主题。bat 也可以给 fzf 当预览器，写法是 `fzf --preview "bat --color=always --style=numbers --line-range=:500 {}"`。

## eza

eza 是一个仍在维护的 `ls` 替代品。

```bash
brew install eza
eza -l --git --group-directories-first
eza --tree -L 2
```

`-l` 是详细视图，`--git` 在这个视图里加上每个文件的 git 状态，`--tree` 以树状递归，`-L` 限制深度。`--icons` 会在名字旁边加图标。常见的做法是在交互式配置里写 `alias ls='eza'`，脚本在自己的 shell 里运行，读不到你的别名，调用的还是原来的 `ls`。

## zoxide

zoxide 记住你去过的目录并给它们排序，所以敲名字的一小段就够了。

```bash
brew install zoxide
eval "$(zoxide init zsh)"   # 写进 ~/.zshrc
z proj                      # 跳到最匹配 proj 的目录
zi proj                     # 交互式选择（需要 fzf）
```

README 说要把 init 那一行放在配置文件的末尾。`zoxide import zsh-z` 可以导入已有的 zsh-z 数据，`--cmd j` 会把命令改成 `j` 和 `ji`，想换个名字时用。它只认识你已经去过的目录，所以新项目要先 `cd` 进去一次，第一次 `z` 才会有结果。

## Atuin

Atuin 用一个 SQLite 数据库接管你的 shell 历史，除了命令本身还记录退出码、工作目录、主机和耗时。它会把 Ctrl + R 和上箭头改绑到全屏搜索，可以按当前会话、当前目录或全局来过滤。

```bash
brew install atuin
echo 'eval "$(atuin init zsh)"' >> ~/.zshrc
atuin import auto
atuin search --exit 0 --after "yesterday 3pm" make
```

同步是可选的，端到端加密，也可以完全不用。Atuin 不会替换你原来的历史文件，已有的历史要靠 `atuin import auto` 导入。完全不想注册云账号的话，文档里有离线用法和自建服务器的说明。

## delta

delta 是 git、diff 和 grep 输出的 pager。Homebrew 里的包叫 `git-delta`，命令是 `delta`。项目推荐的 `~/.gitconfig` 如下：

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

`navigate` 让你用 `n` 和 `N` 在 diff 的各个文件之间跳转。delta 在 git 界面里怎么用，见 [Lazygit 和 delta](/zh/blog/lazygit-and-delta)，lazygit 那一侧需要单独的设置。

## Kaku 装了什么、配了什么

Kaku 的 shell 配置比这份清单窄得多，值得确切知道它做了什么。0.25.0 版本里，`kaku init` 会先询问，再通过 Homebrew 安装缺少的可选工具，也就是 Starship、git-delta、Lazygit 和 Yazi。zoxide 只在目标 shell 是 fish 时才装，装了之后 Kaku 的 fish 配置会在找到这个命令时执行 `zoxide init fish | source`。ripgrep、fd、bat、eza 和 Atuin，Kaku 既不安装也不配置。装上 delta 也不会改 git 的设置，Kaku 0.25.0 不会写 `core.pager`，要让 `git diff` 走 delta，得自己加上前面那几行 `[core]` 和 `[interactive]` 配置。

zsh 这边，Kaku 内置的是 zsh-z 而不是 zoxide，所以在 Kaku 的默认 shell 里 `z <dir>` 本来就能用。Kaku 在自己那一行执行时检查 zoxide 的函数，已经存在才跳过 zsh-z，所以想让 zoxide 接手，它的 init 行要写在 `kaku init` 追加到 `.zshrc` 末尾的 Kaku 那一行上面，而不是像 zoxide README 说的放在最末尾，写在 Kaku 那一行下面的话 zsh-z 照样会加载，用 `type z` 可以确认。Kaku 还把上下箭头绑到了前缀历史搜索，当另一个历史工具（比如 Atuin）在这一步已经占了上箭头，它就跳过这个绑定。其余内置的东西见 [不用 Oh My Zsh 的 zsh 配置](/zh/blog/zsh-without-oh-my-zsh)，[Yazi](/zh/blog/yazi-file-manager) 在装了 fd、ripgrep 和 zoxide 时会用到它们。

## 常见问题

### 这些会弄坏我的脚本吗

只要脚本继续调用原版命令就不会。`alias ls=eza` 这类别名只放在交互式配置里，另外 `rg` 和 `fd` 会跳过被忽略的文件，结果和 `grep -r`、`find` 并不一样。

### 七个都要装吗

不用。ripgrep 和 delta 对开发者见效最快，经常在项目之间跳的人适合 zoxide，常翻历史的人适合 Atuin，bat、eza 和 fd 属于用着更舒服的升级。

### zoxide 还是 zsh-z

两者都按使用频率给目录排序，zsh-z 是 Kaku 在 zsh 里内置的，zoxide 是独立的二进制，在 fish 和其他 shell 里也能用。切换只需要一行写在 Kaku 那一行上面的 init，再跑一次 `zoxide import zsh-z`。

### Atuin 会把我的历史发到哪里吗

只有你注册并同步才会。README 说同步的历史是端到端加密的，也支持纯离线使用。
