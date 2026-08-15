# PicManager operations

## Service contracts

Mac and operational clients use `/api/v1/service`, `/api/v1/health`,
`/api/v1/diagnostics`, `/api/v1/metrics`, and `/api/v1/tasks`. Unversioned photo routes
remain available for the Web UI. The v1 service response identifies API compatibility,
capabilities, service version, and the one-way library fingerprint.

## Durable work

Imports, thumbnails, analysis, geocoding, deduplication, derived-media maintenance, and
Apple synchronization use persisted jobs. Application jobs use negative public task IDs;
historical sync jobs use positive IDs. Leases recover after interruption, retries retain
attempt history, and cancellation is explicit.

## Health and recovery

SQLite uses WAL, a busy timeout, and a bounded pool. Startup recovers expired leases and
filesystem intents, then reconciles derived state. Check:

```bash
curl http://127.0.0.1:8080/api/v1/health
curl http://127.0.0.1:8080/api/v1/diagnostics
curl http://127.0.0.1:8080/api/v1/metrics
```

## Backups

Use the `picmanager backup` commands to create and verify atomic SQLite snapshots. Keep
backup files outside app replacement workflows. Restoring a catalog never authorizes
deleting or rewriting original media.

## Logs and ownership

The Mac app stores bounded rotating service logs after removing user paths, library media
paths, and common credentials. `.picmanager-app.lock` and
`.picmanager-service.lock` are advisory lock files; lock ownership is held by the open
process handle, so stale file contents do not imply a stale active lock.
