# Phase 0–3 upgrade guide

This guide upgrades an existing PicManager library to the additive multi-source
catalog and modern web interface. Schema migration and local-source backfill do not
move or rewrite media files, but a complete backup is still required.

## 1. Back up and rehearse

1. Stop every PicManager process and ensure no importer or PhotoBridge task is active.
2. Copy the complete library directory, including `picmanager.db`, `.thumbs` and all
   media. Keep the backup on a different volume when practical.
3. Build the release and run the automated copied-database rehearsal:

```sh
cargo build --release
cargo test --test migration_rehearsal
```

4. Point PicManager at a second copy of the library and inspect it. The environment
   override relocates the database, thumbnails and media root together:

```sh
PICMANAGER_LIBRARY_PATH=/absolute/path/to/PicManager-rehearsal \
  target/release/picmanager migrate inspect --json
PICMANAGER_LIBRARY_PATH=/absolute/path/to/PicManager-rehearsal \
  target/release/picmanager migrate backfill-local --dry-run --json
```

Do not use the only copy of a library for rehearsal. `PICMANAGER_HOST` and
`PICMANAGER_PORT` can isolate a test server, for example `127.0.0.1:18080`.

## 2. Upgrade the catalog copy

Starting any current PicManager command applies pending additive SQL migrations. Save
the inspection JSON before and after the backfill, then run:

```sh
PICMANAGER_LIBRARY_PATH=/absolute/path/to/PicManager-rehearsal \
  target/release/picmanager migrate backfill-local
PICMANAGER_LIBRARY_PATH=/absolute/path/to/PicManager-rehearsal \
  target/release/picmanager migrate verify --json
PICMANAGER_LIBRARY_PATH=/absolute/path/to/PicManager-rehearsal \
  target/release/picmanager migrate report --json
```

Verification must report a healthy catalog, zero photos without assets or legacy
sources, zero orphan sources, zero assets without primary variants, and no foreign-key
or SQLite integrity errors. Investigate missing media files before proceeding.

Once the copied rehearsal is healthy, repeat the same inspect, dry-run, backfill,
verify and report sequence against the real library while the backup remains intact.

## 3. Use the modern interface

Run `picmanager serve` and open `http://127.0.0.1:8080/`. The default interface offers
the high-density photo timeline, immersive original/current viewer, albums and
collections, people, places, activities, Apple Photos reconciliation, and the task and
duplicate-review center. The preserved interface is available at `/legacy/` if a
workflow has not yet moved to the modern application.

An asset may have multiple representations:

- `original` is immutable provider/camera content;
- `current` is the provider's edited appearance and is the normal display choice;
- `imported` is an existing PicManager file whose exact provenance is not yet known;
- generated previews and thumbnails can be rebuilt and never replace a master.

A recompressed Google Photos copy may contribute metadata or matching evidence, but it
cannot replace a higher-quality original. Google account-wide enumeration is not
promised: future integration uses Picker selections or offline Takeout metadata because
the Google Photos Library API only exposes app-created content for new integrations.

## 4. Reconcile Apple Photos

Create and ingest a metadata-only inventory before requesting image bytes:

```sh
photobridge inventory --output /tmp/apple-inventory.ndjson
picmanager apple inventory /tmp/apple-inventory.ndjson --dry-run --json
picmanager apple inventory /tmp/apple-inventory.ndjson
```

Open the Apple Photos workspace to review ready, unsynced, queued, failed, excluded and
missing source items. Original filenames are displayed; PhotoKit local identifiers stay
internal source identities and are never treated as filenames. Retry failed sources in
the workspace or task center, and explicitly review ambiguous legacy matches.

For incremental changes, obtain the committed checkpoint shown by PicManager, pass it
to `photobridge changes --checkpoint ... --output ...`, and ingest the complete output
with `picmanager apple changes ...`. If PhotoKit history expires, run a full inventory
again. A checkpoint advances only after discovered work is durable.

## 5. RAW and cleanup policy

For RAW+JPEG/HEIC pairs, PicManager prefers the finished JPEG/HEIC and keeps RAW visible
as a companion or excluded source. RAW-only items remain visible. PicManager does not
automatically delete RAW, duplicates, originals or provider files. Any future space
reclamation must first verify pairing, readability and backup, then quarantine files
for review before explicit deletion.

## Rollback

The reliable rollback is to stop PicManager and restore the complete pre-upgrade
library backup. Do not mix a restored database with media from a different snapshot.
Although Phase 0–3 migrations are additive and do not move media, an older binary may
reject a database containing migration versions it does not know. Binary-only rollback
is therefore not guaranteed. Keep migration reports and the failed upgraded copy for
diagnosis instead of deleting tables or manually editing migration history.
