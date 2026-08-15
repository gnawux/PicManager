# Migration and recovery

## Existing-library guarantee

Phase 4/5 uses additive migrations. Existing photos, IDs, metadata, relationships,
source records, variants, and files remain in place. New source inventory and rendition
records are backfilled idempotently. Lower-quality external copies never replace a
higher-quality master.

## Upgrade procedure

1. Stop writers and create a verified catalog backup.
2. Copy or snapshot the library and run the rehearsal in `MIGRATION_REHEARSAL.md`.
3. Review integrity, source-link, variant, and missing-file reports.
4. Upgrade only the executable/app bundle; do not move the library.
5. Start the service and inspect v1 health/diagnostics before resuming imports.
6. Refresh Apple Photos inventory and review pending/conflict/failed items.

## Rollback

Use only versions at or above `minimum_rollback_version` in the release update manifest.
Retain the pre-upgrade backup. Because migrations are additive, an approved older build
may ignore new tables, but this is not a substitute for the declared rollback floor or a
verified restore rehearsal.

## Missing media or interrupted work

Do not manually delete catalog rows. Run diagnostics and reconciliation first. Durable
jobs and filesystem intents are restart-safe; retry failed items after correcting disk,
permission, or iCloud availability. Restore the database only from a verified backup and
keep the current database for forensic comparison.
