# PicManager user guide

PicManager keeps its catalog and original media on your Mac. The Rust service owns the
catalog and background work; the same Web photo interface is available in a browser or
inside the Mac app.

## Recommended Mac workflow

1. Install and open `PicManager.app`.
2. Choose a PicManager library folder. This is not the Photos.app System Photo Library.
3. Grant Full Photos access. PicManager reads the System Photo Library selected by
   Photos.app and never switches or moves it.
4. Open the library in the embedded window or choose system-browser mode in Settings.
5. Choose **Refresh Apple Photos Inventory** to compare metadata. This operation does
   not download or replace originals.
6. Use the Apple source view to inspect synchronized, pending, excluded, missing, and
   failed items. Failed tasks can be retried from the task center or menu bar.

The first inventory preserves PhotoKit local identifiers separately from original phone
filenames. UUID-like provider identities are never used as user-facing filenames.

## Existing files and catalogs

Upgrades keep photo IDs, metadata, relationships, source identities, and media paths.
Run the migration rehearsal before changing a valuable catalog; see
[MIGRATION_AND_RECOVERY.md](MIGRATION_AND_RECOVERY.md). Never point tests at a personal
library.

## RAW policy

PicManager prefers the JPEG/HEIC photo resource for RAW+JPEG pairs and excludes RAW-only
assets from automatic export. Existing RAW files are not deleted. Any future cleanup
must be a separate, explicit dry-run workflow after a verified backup.

## Web/CLI workflow

The standalone service remains supported:

```bash
picmanager import --copy /path/to/photos
picmanager serve
```

Open `http://127.0.0.1:8080`. Use `--copy` when source files must remain in place. See
`MANUAL.md` or `MANUAL.zh.md` for the complete CLI reference.
