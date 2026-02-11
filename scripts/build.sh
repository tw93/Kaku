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
# Optional: set BUILD_TARGET for cross-compilation (e.g. x86_64-apple-darwin)
BUILD_TARGET="${BUILD_TARGET:-}"

if [[ "${1:-}" == "--open" ]]; then
	OPEN_APP=1
fi

APP_BUNDLE_SRC="assets/macos/Kaku.app"
APP_BUNDLE_OUT="$OUT_DIR/$APP_NAME.app"

# Determine cargo target flag and binary output directory
CARGO_TARGET_FLAG=""
if [[ -n "$BUILD_TARGET" ]]; then
	CARGO_TARGET_FLAG="--target $BUILD_TARGET"
fi

echo "[1/6] Building binaries ($PROFILE${BUILD_TARGET:+, target=$BUILD_TARGET})..."
if [[ "$PROFILE" == "release" ]]; then
	cargo build --release $CARGO_TARGET_FLAG -p kaku-gui -p kaku
	PROFILE_DIR="release"
elif [[ "$PROFILE" == "release-opt" ]]; then
	cargo build --profile release-opt $CARGO_TARGET_FLAG -p kaku-gui -p kaku
	PROFILE_DIR="release-opt"
else
	cargo build $CARGO_TARGET_FLAG -p kaku-gui -p kaku
	PROFILE_DIR="debug"
fi

if [[ -n "$BUILD_TARGET" ]]; then
	BIN_DIR="$TARGET_DIR/$BUILD_TARGET/$PROFILE_DIR"
else
	BIN_DIR="$TARGET_DIR/$PROFILE_DIR"
fi

echo "[2/6] Preparing app bundle..."
rm -rf "$APP_BUNDLE_OUT"
mkdir -p "$OUT_DIR"
cp -R "$APP_BUNDLE_SRC" "$APP_BUNDLE_OUT"

# Move libraries from root to Frameworks (macOS requirement)
if ls "$APP_BUNDLE_OUT"/*.dylib 1>/dev/null 2>&1; then
	mkdir -p "$APP_BUNDLE_OUT/Contents/Frameworks"
	mv "$APP_BUNDLE_OUT"/*.dylib "$APP_BUNDLE_OUT/Contents/Frameworks/"
fi

mkdir -p "$APP_BUNDLE_OUT/Contents/MacOS"
mkdir -p "$APP_BUNDLE_OUT/Contents/Resources"

echo "[2.5/6] Syncing version from Cargo.toml..."
# Extract version from kaku/Cargo.toml (assuming it's the source of truth)
VERSION=$(grep '^version =' kaku/Cargo.toml | head -n 1 | cut -d '"' -f2)
if [[ -n "$VERSION" ]]; then
	echo "Stamping version $VERSION into Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP_BUNDLE_OUT/Contents/Info.plist"
	/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP_BUNDLE_OUT/Contents/Info.plist"
else
	echo "Warning: Could not detect version from kaku/Cargo.toml"
fi

echo "[3/6] Downloading vendor dependencies..."
./scripts/download_vendor.sh

echo "[4/6] Copying resources and binaries..."
cp -R assets/shell-integration/* "$APP_BUNDLE_OUT/Contents/Resources/"
cp -R assets/shell-completion "$APP_BUNDLE_OUT/Contents/Resources/"
cp -R assets/fonts "$APP_BUNDLE_OUT/Contents/Resources/"
mkdir -p "$APP_BUNDLE_OUT/Contents/Resources/vendor"
cp -R assets/vendor/* "$APP_BUNDLE_OUT/Contents/Resources/vendor/"
cp assets/shell-integration/first_run.sh "$APP_BUNDLE_OUT/Contents/Resources/"
chmod +x "$APP_BUNDLE_OUT/Contents/Resources/first_run.sh"

# Explicitly use the logo.icns from assets if available
if [[ -f "assets/logo.icns" ]]; then
	cp "assets/logo.icns" "$APP_BUNDLE_OUT/Contents/Resources/terminal.icns"
fi

tic -xe kaku -o "$APP_BUNDLE_OUT/Contents/Resources/terminfo" termwiz/data/kaku.terminfo

for bin in kaku kaku-gui; do
	cp "$BIN_DIR/$bin" "$APP_BUNDLE_OUT/Contents/MacOS/$bin"
	chmod +x "$APP_BUNDLE_OUT/Contents/MacOS/$bin"
done

# Clean up xattrs to prevent icon caching issues or quarantine
xattr -cr "$APP_BUNDLE_OUT"

echo "[5/6] Signing app bundle..."
codesign --force --deep --sign - "$APP_BUNDLE_OUT"

touch "$APP_BUNDLE_OUT/Contents/Resources/terminal.icns"
touch "$APP_BUNDLE_OUT/Contents/Info.plist"
touch "$APP_BUNDLE_OUT"

echo "[6/6] Creating DMG..."
DMG_NAME="$APP_NAME.dmg"
DMG_PATH="$OUT_DIR/$DMG_NAME"
DMG_BASE_PATH="$OUT_DIR/$APP_NAME"
TEMP_DMG_PATH="$OUT_DIR/${APP_NAME}-temp.dmg"
STAGING_DIR="$OUT_DIR/dmg_staging"
BACKGROUND_IMAGE_SOURCE="assets/macos/dmg/background.png"
BACKGROUND_IMAGE_NAME="background.png"

hdiutil_cmd() {
	LC_ALL=C LANG=en_US.UTF-8 hdiutil "$@"
}

cleanup_volumes() {
	local vol_pattern="/Volumes/$APP_NAME"
	local max_attempts=15
	local attempt=1

	while [ $attempt -le $max_attempts ]; do
		if hdiutil_cmd info | grep -q "$vol_pattern"; then
			echo "Detaching existing volumes (Attempt $attempt/$max_attempts)..."
			hdiutil_cmd info | grep "$vol_pattern" | awk '{print $1}' | while read -r dev; do
				echo "Force detaching $dev..."
				hdiutil_cmd detach "$dev" -force || true
			done
			sleep 1
		else
			if [ -d "$vol_pattern" ]; then
				echo "Removing stale mount point directory $vol_pattern..."
				rmdir "$vol_pattern" || true
			fi
			return 0
		fi
		attempt=$((attempt + 1))
	done
	echo "Warning: Failed to fully detach volumes after $max_attempts attempts."
}

configure_dmg_layout() {
	local disk_name="$1"
	local app_name="$2"
	local background_name="$3"

	osascript >/dev/null <<EOF
tell application "Finder"
	tell disk "${disk_name}"
		open
		set current view of container window to icon view
		set toolbar visible of container window to false
		set statusbar visible of container window to false
		set the bounds of container window to {100, 100, 780, 520}
		set viewOptions to the icon view options of container window
		set arrangement of viewOptions to not arranged
		set icon size of viewOptions to 120
		set text size of viewOptions to 14
		try
			set background picture of viewOptions to file ".background:${background_name}"
		end try
		set position of item "${app_name}.app" of container window to {190, 250}
		set position of item "Applications" of container window to {500, 250}
		close
		open
		update without registering applications
		delay 1
	end tell
end tell
EOF
}

cleanup_volumes

sync

rm -rf "$DMG_PATH" "$TEMP_DMG_PATH" "$STAGING_DIR" "$DMG_BASE_PATH.dmg"
mkdir -p "$STAGING_DIR"

cp -R "$APP_BUNDLE_OUT" "$STAGING_DIR/"
ln -s /Applications "$STAGING_DIR/Applications"

if [[ -f "$BACKGROUND_IMAGE_SOURCE" ]]; then
	mkdir -p "$STAGING_DIR/.background"
	cp "$BACKGROUND_IMAGE_SOURCE" "$STAGING_DIR/.background/$BACKGROUND_IMAGE_NAME"
else
	echo "Warning: DMG background image not found at $BACKGROUND_IMAGE_SOURCE; using default Finder background."
fi

mdutil -i off "$STAGING_DIR" >/dev/null 2>&1 || true

echo "Creating DMG..."
MAX_RETRIES=3
RETRY_COUNT=0

while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
	if ! hdiutil_cmd create -quiet -volname "$APP_NAME" \
		-srcfolder "$STAGING_DIR" \
		-ov -format UDRW \
		"$TEMP_DMG_PATH"; then
		echo "hdiutil create failed. Retrying in 2 seconds... ($((RETRY_COUNT + 1))/$MAX_RETRIES)"
		cleanup_volumes
		sleep 2
		RETRY_COUNT=$((RETRY_COUNT + 1))
		continue
	fi

	ATTACH_OUTPUT=$(hdiutil_cmd attach -readwrite -noverify -noautoopen "$TEMP_DMG_PATH" 2>/dev/null || true)
	DEVICE=$(echo "$ATTACH_OUTPUT" | awk '/\/dev\// {print $1; exit}')
	MOUNT_POINT=$(echo "$ATTACH_OUTPUT" | awk -F'\t' '/\/Volumes\// {print $NF; exit}')
	MOUNT_NAME=$(basename "$MOUNT_POINT")

	if [[ -z "$DEVICE" || -z "$MOUNT_POINT" ]]; then
		echo "Failed to attach temporary DMG. Retrying..."
		if [[ -n "${DEVICE:-}" ]]; then
			hdiutil_cmd detach "$DEVICE" -force >/dev/null 2>&1 || true
		fi
		cleanup_volumes
		sleep 2
		RETRY_COUNT=$((RETRY_COUNT + 1))
		continue
	fi

	if ! configure_dmg_layout "$MOUNT_NAME" "$APP_NAME" "$BACKGROUND_IMAGE_NAME"; then
		echo "Warning: Failed to configure Finder layout for DMG."
	fi

	sync
	hdiutil_cmd detach "$DEVICE" -force >/dev/null 2>&1 || true

	if hdiutil_cmd convert -quiet "$TEMP_DMG_PATH" \
		-format UDZO \
		-imagekey zlib-level=9 \
		-ov -o "$DMG_BASE_PATH"; then
		break
	else
		echo "hdiutil convert failed. Retrying in 2 seconds... ($((RETRY_COUNT + 1))/$MAX_RETRIES)"
		cleanup_volumes
		sleep 2
		RETRY_COUNT=$((RETRY_COUNT + 1))
	fi
done

if [ ! -f "$DMG_PATH" ]; then
	echo "Error: Failed to create DMG after retries."
	exit 1
fi

rm -rf "$STAGING_DIR" "$TEMP_DMG_PATH"

echo "DMG created: $DMG_PATH"

echo "Done: $APP_BUNDLE_OUT"
if [[ "$OPEN_APP" == "1" ]]; then
	echo "Opening app..."
	open "$APP_BUNDLE_OUT"
fi
