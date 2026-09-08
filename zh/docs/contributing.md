# 贡献

本地环境、构建命令、PR 流程和 CI 检查。

## 环境准备

克隆仓库，装 Rust 工具链，再装本地开发需要的工具和 git hook。

```bash
git clone https://github.com/tw93/Kaku.git
cd Kaku

# Homebrew 把 rustup 装在 keg-only，需要手动 export PATH
brew install rustup
echo "export PATH=\"$(brew --prefix rustup)/bin:\$HOME/.cargo/bin:\$PATH\"" >> ~/.zprofile
exec zsh -l
rustup toolchain install 1.95.0

# 装 cargo-nextest、cargo-watch、nightly rustfmt
make install-tools

# 装 pre-commit hook：每次提交前会跑 format + test
make install-hooks
```

环境准备好后，`make app` 会把本地 debug app bundle 编译到 `dist/Kaku.app`。它用于开发和验证，不是普通用户的安装方式。

## 日常开发

常用 Make 目标如下，覆盖格式化、类型检查、测试和本地运行。

| 命令 | 用途 |
| --- | --- |
| `make fmt` | 自动格式化代码，需要 nightly Rust |
| `make fmt-check` | 只检查格式，不修改文件 |
| `make check` | cargo check，捕获类型和语法错误 |
| `make test` | 跑单元测试 |
| `make dev` | 本地快速 debug 运行，从 `target/debug` 启动 `kaku-gui` |
| `make build` | 编译二进制，不打 app bundle |
| `make app` | 编译本地测试用的 debug app bundle 到 `dist/Kaku.app` |

推荐的开发循环：

```bash
make fmt        # 先格式化
make check      # 确认能编译
make test       # 跑测试
make dev        # 本地快速运行，不打包
```

需要调整日志等级时给 `make dev` 加环境变量：

```bash
RUST_LOG=debug make dev
```

## 发布构建

本地复现发布产物的几条命令，正式发布走 `scripts/release.sh`。

```bash
# 完整发布：app + DMG，universal 二进制
./scripts/build.sh
# 输出：dist/Kaku.app 和 dist/Kaku.dmg

# 只为当前架构构建，速度更快，本地测试用
./scripts/build.sh --native-arch

# 只打 app bundle，跳过 DMG
./scripts/build.sh --native-arch --app-only

# 构建完直接打开 app
./scripts/build.sh --native-arch --open
```

## Pull Request

1. Fork 仓库，从 `main` 切出分支。
2. 改代码。
3. 本地跑 `make fmt && make check && make test`。
4. 提交并推送。
5. 开 PR，目标分支选 `main`。

代码改动会触发格式、编译和测试检查。Universal 构建单独运行，由构建流程变更、定时任务或手动触发；仅修改 Markdown 不会触发这些检查。

[查看现有 Pull Requests](https://github.com/tw93/Kaku/pulls)

---

Source: https://kaku.fun/zh/docs/contributing
Site index for LLMs: https://kaku.fun/llms.txt
