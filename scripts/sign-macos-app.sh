#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_PATH="${1:-$REPO_ROOT/dist/PicManager.app}"
IDENTITY="${PICMANAGER_CODESIGN_IDENTITY:?Set PICMANAGER_CODESIGN_IDENTITY to a Developer ID Application identity}"
ENTITLEMENTS="$REPO_ROOT/photobridge/Sources/PicManagerMac/PicManagerMac.entitlements"
PHOTO_HELPER_ENTITLEMENTS="$REPO_ROOT/photobridge/Sources/PhotoBridge/PhotoBridge.entitlements"
SERVICE="$APP_PATH/Contents/Resources/picmanager"
PHOTO_HELPER="$APP_PATH/Contents/Resources/photobridge"

"$SCRIPT_DIR/test-macos-bundle.sh" "$APP_PATH"
if [[ "$IDENTITY" == "-" ]]; then
    SIGN_ARGS=(--force --sign -)
else
    SIGN_ARGS=(--force --timestamp --options runtime --sign "$IDENTITY")
fi
codesign "${SIGN_ARGS[@]}" "$SERVICE"
codesign "${SIGN_ARGS[@]}" --entitlements "$PHOTO_HELPER_ENTITLEMENTS" "$PHOTO_HELPER"
codesign "${SIGN_ARGS[@]}" --entitlements "$ENTITLEMENTS" "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"
codesign --display --verbose=2 "$APP_PATH"
