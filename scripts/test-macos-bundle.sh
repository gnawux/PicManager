#!/bin/bash
set -euo pipefail

APP_PATH="${1:?usage: test-macos-bundle.sh /path/to/PicManager.app}"
CONTENTS="$APP_PATH/Contents"
PLIST="$CONTENTS/Info.plist"
APP_EXECUTABLE="$CONTENTS/MacOS/PicManagerMac"
SERVICE_EXECUTABLE="$CONTENTS/Resources/picmanager"
GARMIN_PYTHON="$CONTENTS/Resources/python/bin/python3"
GARMIN_HELPER="$CONTENTS/Resources/garmin_sync.py"

test -d "$APP_PATH"
plutil -lint "$PLIST" >/dev/null
test -x "$APP_EXECUTABLE"
test -x "$SERVICE_EXECUTABLE"
test -x "$GARMIN_PYTHON"
test -f "$GARMIN_HELPER"
"$GARMIN_PYTHON" -c 'import garminconnect, garth'
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")" = "io.picmanager.mac"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$PLIST")" = "PicManagerMac"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$PLIST")" = "APPL"
"$SERVICE_EXECUTABLE" --version | grep -Eq '^picmanager [0-9]+\.[0-9]+\.[0-9]+'

if find "$CONTENTS" -name '*.db' -o -name '*.db-wal' -o -name '*.db-shm' | grep -q .; then
    echo "Application bundle must not contain a user catalog" >&2
    exit 1
fi

echo "Validated $APP_PATH"
