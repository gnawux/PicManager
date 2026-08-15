# Release process

## Assemble and test

```bash
scripts/build-macos-app.sh
scripts/test-macos-install-lifecycle.sh dist/PicManager.app
```

The bundle embeds the production Web assets in the Rust binary and places that service
in `Contents/Resources/picmanager`. Cargo and Info.plist versions must match.

## Sign and notarize

```bash
PICMANAGER_CODESIGN_IDENTITY='Developer ID Application: …' \
  scripts/sign-macos-app.sh dist/PicManager.app
PICMANAGER_NOTARY_PROFILE='picmanager-notary' \
  scripts/notarize-macos-app.sh dist/PicManager.app
```

Credentials are never inferred. Ad-hoc signing (`PICMANAGER_CODESIGN_IDENTITY=-`) is for
local verification only.

## Update metadata

Archive the stapled app, then generate metadata locally with an explicit future URL:

```bash
PICMANAGER_UPDATE_URL='https://example.invalid/PicManager-1.0.1.zip' \
  scripts/generate-update-metadata.sh artifact.zip update-manifest.json
scripts/test-update-metadata.sh update-manifest.json artifact.zip
```

The generator does not upload or publish. Complete `release/INSTALL_TEST_MATRIX.md` and
the gates in `RELEASE_VERIFICATION.md` before any external distribution.
