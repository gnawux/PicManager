#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_ROOT="${PICMANAGER_BUILD_DIR:-$REPO_ROOT/dist}"
APP_PATH="$BUILD_ROOT/PicManager.app"
CONTENTS="$APP_PATH/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCES_DIR="$CONTENTS/Resources"
INFO_PLIST="$REPO_ROOT/photobridge/Sources/PicManagerMac/Info.plist"

CARGO_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
APP_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INFO_PLIST")"
if [[ "$CARGO_VERSION" != "$APP_VERSION" ]]; then
    echo "Version mismatch: Cargo=$CARGO_VERSION app=$APP_VERSION" >&2
    exit 1
fi

cd "$REPO_ROOT/web"
npm run build

cd "$REPO_ROOT"
cargo build --release --bin picmanager
swift build --package-path photobridge -c release --product PicManagerMac

rm -rf "$APP_PATH"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "$INFO_PLIST" "$CONTENTS/Info.plist"
cp "$REPO_ROOT/photobridge/.build/release/PicManagerMac" "$MACOS_DIR/PicManagerMac"
cp "$REPO_ROOT/target/release/picmanager" "$RESOURCES_DIR/picmanager"
cp "$REPO_ROOT/scripts/garmin_sync.py" "$RESOURCES_DIR/garmin_sync.py"
python3 -m venv --copies "$RESOURCES_DIR/python"
"$RESOURCES_DIR/python/bin/pip" install --disable-pip-version-check --no-cache-dir -r "$REPO_ROOT/scripts/requirements-garmin.txt"
chmod 755 "$MACOS_DIR/PicManagerMac" "$RESOURCES_DIR/picmanager"
printf 'APPL????' > "$CONTENTS/PkgInfo"

"$SCRIPT_DIR/test-macos-bundle.sh" "$APP_PATH"
echo "$APP_PATH"
