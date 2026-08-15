# Asset Catalog and Migration Contract

Status: accepted for the Phase 0-3 modernization work.

This document is the compatibility contract for evolving the existing `photos` table
into a multi-source catalog. Database migrations and services must preserve these
invariants unless a later decision record explicitly replaces them.

## Identity boundaries

PicManager uses three independent identities:

1. **Asset identity** represents one logical photographic moment in PicManager.
2. **Source identity** represents an item in an external provider or import source.
3. **Variant identity** represents one concrete byte or generated representation.

An Apple Photos local identifier is a source identity, not an asset ID or filename. A
SHA-256 value identifies bytes, not necessarily a photographic moment. A pHash is only
similarity evidence and is never a unique identity.

During the compatibility period, every imported `photos` row has exactly one `assets`
row and `assets.photo_id` remains unique. Existing APIs may continue to address the
photo ID while newer APIs use the asset ID.

## Provider names

Provider values are stable lowercase identifiers:

| Provider | Meaning |
| --- | --- |
| `legacy_local` | A photo that existed before the source catalog was introduced |
| `local_import` | A file imported directly from a user-selected directory |
| `apple_photos` | A PhotoKit `PHAsset` |
| `google_photos` | A Google Picker or API record |
| `google_takeout` | A Takeout media/JSON record |

Provider-specific IDs are opaque strings. They must not be parsed except by the owning
adapter. The `(provider, external_id)` pair is unique when an external ID is present.

## Source lifecycle

`asset_sources.sync_status` uses the following states:

```text
discovered -> queued -> downloading -> downloaded -> importing -> ready
     |          |           |              |            |
     +----------+-----------+--------------+------------+-> failed
     |
     +-> excluded
     +-> missing
```

- `discovered`: present in provider inventory; no transfer has been scheduled.
- `queued`: a durable sync item exists.
- `downloading`: the source adapter holds an active lease.
- `downloaded`: bytes are in managed staging and have passed basic validation.
- `importing`: PicManager is committing catalog/file changes.
- `ready`: required variants and links are usable.
- `failed`: retryable or terminal failure details are recorded.
- `excluded`: a visible policy decision, for example an unselected burst or RAW policy.
- `missing`: previously known source item was absent during full reconciliation.

Transitions are validated in service code. Recovery may move expired `downloading` and
`importing` items back to `queued`; it must increment attempts and retain diagnostics.

## Variant roles and quality

Variant roles are stable lowercase identifiers:

| Role | Meaning |
| --- | --- |
| `original` | Immutable provider or camera original |
| `current` | Provider's current edited appearance |
| `imported` | Existing PicManager file when provenance is not yet known |
| `preview` | Generated screen-sized rendition |
| `thumbnail` | Generated grid rendition |
| `raw_companion` | Optional RAW paired with a finished image |

Variants include content hash, byte size, pixel dimensions, media type and provenance
when known. Only one preferred variant exists per asset and role. Generated variants
carry a generation key so stale derivatives can be invalidated deterministically.

Canonical-master selection follows this order unless a user override exists:

1. byte-preserved original from a trusted source;
2. existing imported full-resolution file;
3. provider current rendition;
4. recompressed remote copy;
5. generated preview.

Lower-quality variants may contribute metadata but cannot silently replace a higher-
quality master.

## Matching evidence

Links use an explicit method and confidence:

| Method | Default confidence | Automatic merge |
| --- | ---: | --- |
| provider external ID | 1.00 | yes |
| exact SHA-256 | 1.00 | yes, subject to collision-safe constraints |
| legacy sanitized Apple ID filename | 0.99 | yes |
| structured metadata agreement | calculated | only above reviewed threshold |
| perceptual similarity | calculated | no, supporting evidence only |
| user confirmation | 1.00 | yes |

An automatic link never deletes either source or variant. Conflicting high-confidence
evidence creates a review item and diagnostic event.

## Migration invariants

Every migration and backfill must preserve:

- all existing `photos.id` values;
- all foreign-key relationships to photos, faces, people, albums and activities;
- photo paths and bytes unless a separate, explicit file operation is requested;
- active/deleted import status and `photo_stats.active_count` consistency;
- manual timestamps, timezone offsets, rotations and flips;
- face embeddings, person assignments and cover faces;
- album, collection and dedup decisions;
- geocache and activity metadata.

Backfills are idempotent. Running a completed backfill again must not create duplicate
assets, sources, variants or audit rows. A failed run remains inspectable and may resume
from a recorded checkpoint.

## Transaction and filesystem boundary

SQLite cannot atomically commit a filesystem rename. Operations that change both use a
recoverable intent protocol:

1. persist the intended operation and source state;
2. perform the filesystem operation using explicit paths;
3. validate the resulting bytes or file metadata;
4. commit the catalog mutation and mark the intent complete;
5. reconcile incomplete intents at startup.

No cleanup process treats an uncommitted intent as permission to delete data.

## PhotoKit checkpoint contract

PhotoKit enumeration and durable work creation are separate from transfer:

1. fetch changes since the saved token;
2. upsert every discovered source item and its sync item in a transaction;
3. commit the new token only after all discovered work is durable;
4. process queued work independently;
5. retain failed work across future tokens and application restarts.

A periodic full inventory reconciliation is required because persistent history may
expire and provider state may change outside the incremental stream.

## Google Photos boundary

Google sources use the same catalog but different discovery mechanisms. Picker sessions
contain explicitly selected items. Takeout imports contain an offline inventory and
sidecar metadata. Neither is modeled as guaranteed continuous full-account sync.

Recompressed Google files remain distinct variants. Metadata matching may link them to
an Apple or local asset, but exact and perceptual evidence are retained for review.

## RAW policy contract

RAW+finished pairs prefer the finished JPEG/HEIC as the default master. The RAW is a
visible `raw_companion` or policy-excluded source item. RAW-only assets are visible and
may be rendered to a finished copy, but are not silently discarded.

Deletion is outside synchronization. A future cleanup workflow must verify pairing,
readability and backup, then quarantine before requiring explicit deletion approval.

## Compatibility and rollout

The rollout uses dual reads before changing API contracts:

1. migrations create new tables without changing existing handlers;
2. legacy backfill establishes one-to-one asset/photo links;
3. new repositories dual-write source and variant data for new imports;
4. read APIs adopt asset data with compatibility fallbacks;
5. verification proves coverage before old columns can be considered redundant.

At no point may the application require a completed Apple or Google link merely to show
an existing local photo.
