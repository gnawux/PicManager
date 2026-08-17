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
swift build --package-path photobridge -c release --product PhotoBridge

rm -rf "$APP_PATH"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "$INFO_PLIST" "$CONTENTS/Info.plist"
cp "$REPO_ROOT/photobridge/.build/release/PicManagerMac" "$MACOS_DIR/PicManagerMac"
cp "$REPO_ROOT/photobridge/.build/release/PhotoBridge" "$RESOURCES_DIR/photobridge"
cp "$REPO_ROOT/target/release/picmanager" "$RESOURCES_DIR/picmanager"
cp "$REPO_ROOT/scripts/garmin_sync.py" "$RESOURCES_DIR/garmin_sync.py"
PYTHON_RUNTIME="${PICMANAGER_PYTHON_RUNTIME:-$(dirname "$(dirname "$(uv python find 3.12)")")}"
# Python bytecode is derived runtime state, not sealed application content. Exclude it
# while assembling the bundle and require the helper to keep any later cache external.
rsync -aL --delete --exclude '__pycache__/' --exclude '*.pyc' --exclude '*.pyo' "$PYTHON_RUNTIME/" "$RESOURCES_DIR/python/"
# PEP 517 build subprocesses do not reliably inherit Python's -B flag. Point their
# cache at the build root so source installs cannot write bytecode into signed app
# contents; the runtime itself uses the same external-cache contract at launch.
PYTHON_BYTECODE_CACHE="${PICMANAGER_PYTHON_BYTECODE_CACHE:-$BUILD_ROOT/python-bytecode-cache}"
mkdir -p "$PYTHON_BYTECODE_CACHE"
install_garmin_dependencies() {
    local attempt
    for attempt in 1 2 3; do
        if PYTHONDONTWRITEBYTECODE=1 PYTHONPYCACHEPREFIX="$PYTHON_BYTECODE_CACHE" \
            "$RESOURCES_DIR/python/bin/python3" -B -m pip install --break-system-packages \
            --disable-pip-version-check --no-cache-dir --no-compile --only-binary=:all: \
            --require-hashes -r "$REPO_ROOT/scripts/requirements-garmin.txt"; then
            return 0
        fi
        if [[ "$attempt" -lt 3 ]]; then
            echo "Garmin wheel installation failed (attempt $attempt/3); retrying PyPI." >&2
        fi
    done
    echo "Unable to install the hash-locked Garmin wheels after 3 attempts." >&2
    return 1
}
install_garmin_dependencies
# Some isolated PEP 517 bootstrap subprocesses still create standard-library
# bytecode before they inherit the cache policy. This tree is the newly assembled
# app runtime (never user data), so remove only those derived cache files before
# invoking the bundle and signature gates.
find "$RESOURCES_DIR/python" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete
find "$RESOURCES_DIR/python" -type d -name '__pycache__' -empty -delete
chmod 755 "$MACOS_DIR/PicManagerMac" "$RESOURCES_DIR/picmanager" "$RESOURCES_DIR/photobridge"
printf 'APPL????' > "$CONTENTS/PkgInfo"

"$SCRIPT_DIR/test-macos-bundle.sh" "$APP_PATH"
echo "$APP_PATH"
