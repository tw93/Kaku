# Contributing

How to build Kaku locally and send a pull request.

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

After setup, `make app` builds a debug app bundle at `dist/Kaku.app` for development and testing. For everyday use, install from the DMG or Homebrew.

## Development

Make targets cover formatting, type checks, tests, and local runs.

| Command | Purpose |
| --- | --- |
| `make fmt` | Format the code, needs nightly Rust |
| `make fmt-check` | Check formatting without changing files |
| `make check` | Run cargo check to catch type and syntax errors |
| `make test` | Run unit tests |
| `make dev` | Quick local debugging, builds `kaku-gui` and runs it from `target/debug` |
| `make build` | Build the binaries without an app bundle |
| `make app` | Build a debug app bundle at `dist/Kaku.app` for local testing |

A typical loop:

```bash
make fmt        # format first
make check      # verify it compiles
make test       # run tests
make dev        # fast local run without packaging
```

To change the log level for `make dev`:

```bash
RUST_LOG=debug make dev
```

## Build Release

These commands reproduce the release artifacts locally. Official releases go through `scripts/release.sh`.

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

For code changes, CI checks formatting, compilation, and tests. Universal builds run separately, on build-pipeline changes, on a schedule, or when dispatched by hand. Changes that only touch Markdown trigger neither.

[Browse open Pull Requests](https://github.com/tw93/Kaku/pulls)

---

Source: https://kaku.fun/docs/contributing
Site index for LLMs: https://kaku.fun/llms.txt
