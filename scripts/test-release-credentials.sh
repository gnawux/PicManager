#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if env -u PICMANAGER_CODESIGN_IDENTITY "$SCRIPT_DIR/sign-macos-app.sh" /nonexistent >/dev/null 2>&1; then
    echo "Signing script accepted missing credentials" >&2
    exit 1
fi
if env -u PICMANAGER_NOTARY_PROFILE "$SCRIPT_DIR/notarize-macos-app.sh" /nonexistent >/dev/null 2>&1; then
    echo "Notarization script accepted missing credentials" >&2
    exit 1
fi

echo "Release scripts require explicit external credentials"
