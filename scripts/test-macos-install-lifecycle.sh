#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SOURCE_APP="${1:?usage: test-macos-install-lifecycle.sh /path/to/PicManager.app}"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/picmanager-install.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

APPLICATIONS="$TEST_ROOT/Applications"
SUPPORT="$TEST_ROOT/Application Support/PicManager"
LIBRARY="$TEST_ROOT/Pictures/PicManager"
INSTALLED_APP="$APPLICATIONS/PicManager.app"
mkdir -p "$APPLICATIONS"

# Clean installation must not create or bundle user state.
/usr/bin/ditto "$SOURCE_APP" "$INSTALLED_APP"
"$SCRIPT_DIR/test-macos-bundle.sh" "$INSTALLED_APP"
test ! -e "$SUPPORT/config.json"
test ! -e "$LIBRARY/picmanager.db"
/usr/libexec/PlistBuddy -c 'Print :NSPhotoLibraryUsageDescription' \
    "$INSTALLED_APP/Contents/Info.plist" >/dev/null

# Represent state produced after the interactive first-run assistant.
mkdir -p "$SUPPORT" "$LIBRARY/2026-08-15"
printf '%s\n' '{"library_path":"<test-library>","api":"v1"}' > "$SUPPORT/config.json"
printf '%s\n' 'catalog-preservation-marker' > "$LIBRARY/picmanager.db"
printf '%s\n' 'media-preservation-marker' > "$LIBRARY/2026-08-15/IMG_0001.JPG"
BEFORE="$(find "$SUPPORT" "$LIBRARY" -type f -print0 | sort -z | xargs -0 shasum -a 256)"

# Upgrade replaces only the app bundle.
mv "$INSTALLED_APP" "$APPLICATIONS/PicManager.previous.app"
/usr/bin/ditto "$SOURCE_APP" "$INSTALLED_APP"
"$SCRIPT_DIR/test-macos-bundle.sh" "$INSTALLED_APP"
AFTER_UPGRADE="$(find "$SUPPORT" "$LIBRARY" -type f -print0 | sort -z | xargs -0 shasum -a 256)"
test "$BEFORE" = "$AFTER_UPGRADE"

# Uninstall removes the app while preserving catalog, media, and configuration.
mkdir -p "$TEST_ROOT/Trash"
mv "$INSTALLED_APP" "$TEST_ROOT/Trash/PicManager.app"
test -f "$SUPPORT/config.json"
test -f "$LIBRARY/picmanager.db"
test -f "$LIBRARY/2026-08-15/IMG_0001.JPG"
AFTER_UNINSTALL="$(find "$SUPPORT" "$LIBRARY" -type f -print0 | sort -z | xargs -0 shasum -a 256)"
test "$BEFORE" = "$AFTER_UNINSTALL"

echo "Validated clean install, upgrade, and preservation-safe uninstall"
