#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ARTIFACT="${1:?usage: generate-update-metadata.sh artifact.zip [output.json]}"
OUTPUT="${2:-$REPO_ROOT/dist/update-manifest.json}"
UPDATE_URL="${PICMANAGER_UPDATE_URL:?Set PICMANAGER_UPDATE_URL to the future artifact URL}"
CHANNEL="${PICMANAGER_UPDATE_CHANNEL:-stable}"
INFO_PLIST="$REPO_ROOT/photobridge/Sources/PicManagerMac/Info.plist"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$INFO_PLIST")"
ROLLBACK_VERSION="${PICMANAGER_MINIMUM_ROLLBACK_VERSION:-$VERSION}"
SHA256="$(shasum -a 256 "$ARTIFACT" | awk '{print $1}')"
SIZE_BYTES="$(stat -f '%z' "$ARTIFACT")"
GENERATED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

mkdir -p "$(dirname "$OUTPUT")"
node -e '
const fs = require("fs");
const [output, channel, version, url, sha256, size, generatedAt, rollback] = process.argv.slice(1);
const manifest = {
  schema_version: 1,
  channel,
  version,
  generated_at: generatedAt,
  artifact: { url, sha256, size_bytes: Number(size) },
  compatibility: {
    service_api: "v1",
    catalog_schema: 26,
    minimum_rollback_version: rollback,
    backup_required_before_rollback: true
  }
};
fs.writeFileSync(output, JSON.stringify(manifest, null, 2) + "\n", { flag: "wx" });
' "$OUTPUT" "$CHANNEL" "$VERSION" "$UPDATE_URL" "$SHA256" "$SIZE_BYTES" "$GENERATED_AT" "$ROLLBACK_VERSION"

echo "$OUTPUT"
