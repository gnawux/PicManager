#!/bin/bash
set -euo pipefail

MANIFEST="${1:?usage: test-update-metadata.sh update-manifest.json artifact.zip}"
ARTIFACT="${2:?usage: test-update-metadata.sh update-manifest.json artifact.zip}"
node -e '
const fs = require("fs");
const crypto = require("crypto");
const [manifestPath, artifactPath] = process.argv.slice(1);
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const artifact = fs.readFileSync(artifactPath);
const digest = crypto.createHash("sha256").update(artifact).digest("hex");
if (manifest.schema_version !== 1 || manifest.compatibility.service_api !== "v1") process.exit(1);
if (manifest.compatibility.backup_required_before_rollback !== true) process.exit(1);
if (manifest.artifact.sha256 !== digest || manifest.artifact.size_bytes !== artifact.length) process.exit(1);
' "$MANIFEST" "$ARTIFACT"

echo "Validated $MANIFEST"
