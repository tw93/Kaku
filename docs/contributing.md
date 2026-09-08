# Contributing

Local setup, build commands, PR workflow, and CI checks.

## Setup

Clone the repo, install the Rust toolchain, then install the local dev tools and the git hook.

```bash
git clone https://github.com/tw93/Kaku.git
cd Kaku

# Install Rust if it isn't already available (Homebrew keeps rustup keg-only)
brew install rustup
echo "export PATH=\"$(brew --prefix rustup)/bin:\$HOME/.cargo/bin:\$PATH\"" >> ~/.zprofile
exec zsh -l
rustup toolchain install 1.95.0

# Install required tools (cargo-nextest, cargo-watch, nightly rustfmt)
make install-tools

# Install pre-commit hook (format + test before each commit)
make install-hooks
```

After setup, `make app` builds a local debug app bundle at `dist/Kaku.app`. Use it for development and verification; normal users should install from the DMG or Homebrew.

## Development

Make targets cover formatting, type checks, tests, and local runs.

| Command | Purpose |
| --- | --- |
| `make fmt` | Auto-format code, requires nightly Rust |
| `make fmt-check` | Check formatting without modifying files |
| `make check` | Compile check, catch type and syntax errors |
| `make test` | Run unit tests |
| `make dev` | Fast local debug: build `kaku-gui` and run from `target/debug` |
| `make build` | Compile binaries (no app bundle) |
| `make app` | Build a debug app bundle to `dist/Kaku.app` for local testing |

Recommended workflow:

```bash
make fmt        # format first
make check      # verify it compiles
make test       # run tests
make dev        # fast local run without packaging
```

Override the log level for `make dev`:

```bash
RUST_LOG=debug make dev
```

## Build Release

Reproduce release artifacts locally. Official releases run through `scripts/release.sh`.

```bash
# Build app and DMG, release, universal binary
./scripts/build.sh
# Outputs: dist/Kaku.app and dist/Kaku.dmg

# Build for current architecture only, faster for local testing
./scripts/build.sh --native-arch

# Build app bundle only, skip DMG creation
./scripts/build.sh --native-arch --app-only

# Build and open the app automatically
./scripts/build.sh --native-arch --open
```

## Pull Requests

1. Fork the repo and branch from `main`.
2. Make your changes.
3. Run `make fmt && make check && make test` locally.
4. Commit and push.
5. Open a PR targeting `main`.

CI checks formatting, compilation, and tests for code changes. Universal builds run separately for build-pipeline changes, on a schedule, or on manual dispatch. Markdown-only changes do not trigger these checks.

[Browse open Pull Requests](https://github.com/tw93/Kaku/pulls)

---

Source: https://kaku.fun/docs/contributing
Site index for LLMs: https://kaku.fun/llms.txt
