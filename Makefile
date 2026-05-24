.PHONY: all fmt fmt-check build app dev check test install-tools install-hooks test-webgpu-fallback release dmg nightly

all: build

RUST_LOG ?= info

test:
	cargo nextest run --locked -E 'not test(shapecache::test::ligatures_jetbrains)'
	cargo nextest run --locked -p wezterm-escape-parser # no_std by default

check:
	cargo check --locked
	cargo check --locked -p wezterm-escape-parser
	cargo check --locked -p wezterm-cell
	cargo check --locked -p wezterm-surface
	cargo check --locked -p wezterm-ssh
	./scripts/check_prompts.sh

app:
	PROFILE=debug ./scripts/build.sh --app-only 2>&1 | grep -v "ranlib: warning:.*has no symbols"; exit $${PIPESTATUS[0]}

dev:
	@if ! command -v cargo-watch >/dev/null 2>&1; then \
		echo "Installing cargo-watch..."; \
		cargo install cargo-watch --locked; \
	fi
	RUST_LOG=$(RUST_LOG) cargo watch \
		--skip-local-deps \
		-w Cargo.toml \
		-w kaku-gui \
		-w window \
		-w term \
		-w mux \
		-w config \
		-w kaku \
		-w lua-api-crates \
		-i "dist/**" \
		-i "deps/**" \
		-x "run $(BUILD_OPTS) -p kaku-gui --"

build:
	cargo build --locked $(BUILD_OPTS) -p kaku -p kaku-gui -p wezterm-mux-server-impl

fmt:
	cargo +nightly fmt -p kaku -p kaku-gui -p mux -p wezterm-term -p termwiz -p config -p wezterm-font

fmt-check:
	cargo +nightly fmt -p kaku -p kaku-gui -p mux -p wezterm-term -p termwiz -p config -p wezterm-font -- --check
	@echo "Format check passed."

install-tools:
	@if ! command -v cargo >/dev/null 2>&1 || ! command -v rustup >/dev/null 2>&1; then \
		echo "Rust toolchain not found. Install rustup first, then re-run make install-tools."; \
		echo "See CONTRIBUTING.md for the bootstrap steps."; \
		exit 1; \
	fi
	cargo install cargo-nextest --locked
	cargo install cargo-watch --locked
	rustup toolchain install nightly --profile minimal --component rustfmt
	@echo "Tools installed."

install-hooks:
	@hooks_dir="$$(git rev-parse --git-path hooks 2>/dev/null)" && \
	if [ -z "$$hooks_dir" ]; then \
		echo "install-hooks must be run from a git checkout."; \
		exit 1; \
	fi && \
	mkdir -p "$$hooks_dir" && \
	printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail' 'exec make fmt-check test' > "$$hooks_dir/pre-commit" && \
	chmod +x "$$hooks_dir/pre-commit" && \
	echo "Installed pre-commit hook at $$hooks_dir/pre-commit"

test-webgpu-fallback:
	./scripts/test_webgpu_fallback.sh --strict

dmg:
	./scripts/build.sh && ./scripts/notarize.sh

nightly:
	./scripts/nightly.sh

release:
	bash ./scripts/release.sh
