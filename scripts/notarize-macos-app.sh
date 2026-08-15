#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_PATH="${1:-$REPO_ROOT/dist/PicManager.app}"
ARCHIVE_PATH="${2:-$REPO_ROOT/dist/PicManager-notarization.zip}"
NOTARY_PROFILE="${PICMANAGER_NOTARY_PROFILE:?Set PICMANAGER_NOTARY_PROFILE to an xcrun notarytool keychain profile}"
STAGING_ARCHIVE="$ARCHIVE_PATH.$$.partial"
trap 'rm -f "$STAGING_ARCHIVE"' EXIT

codesign --verify --deep --strict --verbose=2 "$APP_PATH"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$STAGING_ARCHIVE"
mv -f "$STAGING_ARCHIVE" "$ARCHIVE_PATH"
xcrun notarytool submit "$ARCHIVE_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$APP_PATH"
xcrun stapler validate "$APP_PATH"
