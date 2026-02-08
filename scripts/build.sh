#!/usr/bin/env bash
set -euo pipefail

if [[ "${OSTYPE:-}" != darwin* ]]; then
	echo "This script is macOS-only." >&2
	exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

APP_NAME="Kaku"
TARGET_DIR="${TARGET_DIR:-target}"
PROFILE="${PROFILE:-release}"
OUT_DIR="${OUT_DIR:-dist}"
OPEN_APP="${OPEN_APP:-0}"

if [[ "${1:-}" == "--open" ]]; then
	OPEN_APP=1
fi

APP_BUNDLE_SRC="assets/macos/Kaku.app"
APP_BUNDLE_OUT="$OUT_DIR/$APP_NAME.app"

echo "[1/5] Building binaries ($PROFILE)..."
if [[ "$PROFILE" == "release" ]]; then
	cargo build --release -p kaku-gui -p kaku
	BIN_DIR="$TARGET_DIR/release"
else
	cargo build -p kaku-gui -p kaku
	BIN_DIR="$TARGET_DIR/debug"
fi

echo "[2/5] Preparing app bundle..."
rm -rf "$APP_BUNDLE_OUT"
mkdir -p "$OUT_DIR"
cp -R "$APP_BUNDLE_SRC" "$APP_BUNDLE_OUT"

mkdir -p "$APP_BUNDLE_OUT/Contents/MacOS"
mkdir -p "$APP_BUNDLE_OUT/Contents/Resources"

echo "[3/5] Copying resources and binaries..."
cp -R assets/shell-integration/* "$APP_BUNDLE_OUT/Contents/Resources/"
cp -R assets/shell-completion "$APP_BUNDLE_OUT/Contents/Resources/"
cp -R assets/fonts "$APP_BUNDLE_OUT/Contents/Resources/"

tic -xe kaku -o "$APP_BUNDLE_OUT/Contents/Resources/terminfo" termwiz/data/kaku.terminfo

for bin in kaku kaku-gui; do
	cp "$BIN_DIR/$bin" "$APP_BUNDLE_OUT/Contents/MacOS/$bin"
	chmod +x "$APP_BUNDLE_OUT/Contents/MacOS/$bin"
done

# Clean up xattrs to prevent icon caching issues or quarantine
xattr -cr "$APP_BUNDLE_OUT"

echo "[4/5] Signing app bundle..."
codesign --force --deep --sign - "$APP_BUNDLE_OUT"

touch "$APP_BUNDLE_OUT/Contents/Resources/terminal.icns"
touch "$APP_BUNDLE_OUT/Contents/Info.plist"
touch "$APP_BUNDLE_OUT"

echo "[5/5] Done: $APP_BUNDLE_OUT"
if [[ "$OPEN_APP" == "1" ]]; then
	echo "Opening app..."
	open "$APP_BUNDLE_OUT"
fi
