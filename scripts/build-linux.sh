#!/usr/bin/env bash
set -euo pipefail

if [[ "${OSTYPE:-}" == darwin* ]]; then
	echo "This script is Linux-only. Use build.sh for macOS." >&2
	exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP_NAME="Kaku"
PROFILE="${PROFILE:-release}"
OUT_DIR="${OUT_DIR:-dist}"
ARCH="$(uname -m)"
VERSION=$(grep '^version =' kaku/Cargo.toml | head -n 1 | cut -d '"' -f2)

# Determine cargo flags and binary directory
if [[ "$PROFILE" == "release" ]]; then
	CARGO_FLAGS="--release"
	BIN_DIR="target/release"
elif [[ "$PROFILE" == "release-opt" ]]; then
	CARGO_FLAGS="--profile release-opt"
	BIN_DIR="target/release-opt"
else
	CARGO_FLAGS=""
	BIN_DIR="target/debug"
fi

echo "[1/4] Building binaries ($PROFILE)..."
cargo build $CARGO_FLAGS -p kaku -p kaku-gui

echo "[2/4] Preparing package directory..."
PKG_NAME="$APP_NAME-linux-$ARCH"
PKG_DIR="$OUT_DIR/$PKG_NAME"
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/bin"
mkdir -p "$PKG_DIR/share/terminfo"
mkdir -p "$PKG_DIR/share/shell-integration"
mkdir -p "$PKG_DIR/share/shell-completion"
mkdir -p "$PKG_DIR/share/fonts"

echo "[3/4] Copying files..."
for bin in kaku kaku-gui; do
	if [[ -f "$BIN_DIR/$bin" ]]; then
		cp "$BIN_DIR/$bin" "$PKG_DIR/bin/"
		chmod +x "$PKG_DIR/bin/$bin"
	fi
done

# Shell integration & completion
if [[ -d "assets/shell-integration" ]]; then
	cp -R assets/shell-integration/* "$PKG_DIR/share/shell-integration/"
fi
if [[ -d "assets/shell-completion" ]]; then
	cp -R assets/shell-completion/* "$PKG_DIR/share/shell-completion/"
fi

# Fonts
if [[ -d "assets/fonts" ]]; then
	cp -R assets/fonts/* "$PKG_DIR/share/fonts/"
fi

# Compile terminfo
if command -v tic &>/dev/null && [[ -f "termwiz/data/kaku.terminfo" ]]; then
	tic -xe kaku -o "$PKG_DIR/share/terminfo" termwiz/data/kaku.terminfo || echo "Warning: terminfo compilation failed"
fi

echo "[4/4] Creating archive..."
mkdir -p "$OUT_DIR"
tar -czf "$OUT_DIR/$PKG_NAME.tar.gz" -C "$OUT_DIR" "$PKG_NAME"
rm -rf "$PKG_DIR"

echo "Archive created: $OUT_DIR/$PKG_NAME.tar.gz"
echo "Version: ${VERSION:-unknown}"
