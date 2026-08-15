# Legacy catalog migration rehearsal

The automated rehearsal creates a schema-0018 catalog with representative HEIC,
JPEG and PNG records, metadata, album membership, a face link and an activity. It
closes and copies that database, then upgrades only the copy.

Run it before a release:

```sh
cargo test --test migration_rehearsal
```

The test enforces these migration invariants:

- schema upgrades and local-source backfill do not change photo IDs, paths,
  hashes, timestamps, camera/GPS fields, display transforms or dimensions;
- album, face and activity relationships survive unchanged;
- dry-run creates no catalog or audit rows;
- actual backfill creates one asset, local source, imported variant and accepted
  link per legacy photo;
- a second backfill creates no duplicate records;
- catalog verification succeeds with file checks enabled;
- completed migration runs contain audit summaries;
- the original pre-upgrade database remains at schema 0018.

For a personal library, first make a filesystem-level copy of both the database
and media root while PicManager is stopped. Set `PICMANAGER_LIBRARY_PATH` to the
copy and run the operator workflow documented in the upgrade guide. Never use the
only copy of a library for a migration rehearsal.
