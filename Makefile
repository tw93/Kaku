.PHONY: all fmt fmt-check build app dev check test install-hooks install-tools

all: build

RUST_LOG ?= info

test:
	cargo nextest run
	cargo nextest run -p wezterm-escape-parser # no_std by default

check:
	cargo check
	cargo check -p wezterm-escape-parser
	cargo check -p wezterm-cell
	cargo check -p wezterm-surface
	cargo check -p wezterm-ssh

app:
	PROFILE=debug ./scripts/build.sh --app-only

dev:
	cargo build $(BUILD_OPTS) -p kaku-gui
	RUST_LOG=$(RUST_LOG) ./target/debug/kaku-gui

build:
	cargo build $(BUILD_OPTS) -p kaku -p kaku-gui -p wezterm-mux-server-impl

fmt:
	cargo +nightly fmt -p kaku -p kaku-gui -p mux -p wezterm-term -p termwiz -p config -p wezterm-font

fmt-check:
	cargo +nightly fmt -p kaku -p kaku-gui -p mux -p wezterm-term -p termwiz -p config -p wezterm-font -- --check
	@echo "Format check passed."

install-hooks:
	cp scripts/pre-commit .git/hooks/pre-commit
	chmod +x .git/hooks/pre-commit
	@echo "Pre-commit hook installed."

install-tools:
	cargo install cargo-nextest --locked
	rustup toolchain install nightly --component rustfmt
	@echo "Tools installed."
