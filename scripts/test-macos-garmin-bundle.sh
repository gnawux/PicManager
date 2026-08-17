#!/bin/bash
set -euo pipefail

APP_PATH="${1:?usage: test-macos-garmin-bundle.sh /path/to/PicManager.app}"
CONTENTS="$APP_PATH/Contents"
SERVICE="$CONTENTS/Resources/picmanager"

test -x "$SERVICE"
test ! -e "$CONTENTS/Resources/python"
test ! -e "$CONTENTS/Resources/garmin_sync.py"

if find "$CONTENTS" \( -type d -name 'garminconnect' -o -type f -name 'requirements-garmin.txt' \) -print -quit | grep -q .; then
    echo "Application bundle contains the retired Python Garmin client" >&2
    exit 1
fi

if otool -L "$SERVICE" | grep -qi python; then
    echo "Native service unexpectedly links a Python runtime" >&2
    exit 1
fi
