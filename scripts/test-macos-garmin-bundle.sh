#!/bin/bash
set -euo pipefail

APP_PATH="${1:?usage: test-macos-garmin-bundle.sh /path/to/PicManager.app}"
PYTHON="$APP_PATH/Contents/Resources/python/bin/python3"
HELPER="$APP_PATH/Contents/Resources/garmin_sync.py"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/picmanager-garmin-bundle.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

# This is deliberately offline: no password means the helper must report its durable
# not-configured state without constructing a Garmin connection. -B is the same guard
# used by the Rust service and proves package imports cannot create sealed pycache files.
RESULT="$(PYTHONDONTWRITEBYTECODE=1 "$PYTHON" -B "$HELPER" auth \
  --output "$TEST_ROOT/downloads" \
  --state "$TEST_ROOT/journal.json" \
  --tokens "$TEST_ROOT/tokens" \
  --email "fixture@example.invalid")"
test "$RESULT" = '{"message": "Garmin credentials are not available to the local service", "status": "not_configured"}'

if find "$APP_PATH/Contents" \( -type d -name '__pycache__' -o -type f \( -name '*.pyc' -o -name '*.pyo' \) \) -print -quit | grep -q .; then
    echo "Application bundle contains Python bytecode; it must stay external to the code seal" >&2
    exit 1
fi
