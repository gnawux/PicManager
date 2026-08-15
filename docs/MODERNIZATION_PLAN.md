# PicManager Modernization Plan

## 1. Product direction

PicManager will evolve from a feature-rich local engineering tool into a reliable,
daily-use personal photo product. The long-term shape is a web-first photo management
core with a thin macOS companion:

```text
macOS companion / menu bar agent
  - PhotoKit permission and inventory
  - automatic Apple Photos synchronization
  - local service lifecycle and diagnostics
  - native notifications and first-run setup
                         |
                         v
Rust PicManager core and HTTP API
  - SQLite catalog and filesystem library
  - durable background jobs
  - source reconciliation and metadata
  - thumbnails, search, faces, animals, geography and deduplication
                         |
                         v
Modern web frontend
  - local browser
  - macOS WebView shell
  - future authenticated access from other devices
```

The browser interface remains the primary UI so that local desktop use and future
multi-device access share one implementation. A native macOS application is a thin
system-integration layer, not a second photo-management frontend.

## 2. Non-negotiable principles

1. Existing photos, photo IDs and user metadata must survive the modernization.
2. Schema changes are additive until a verified migration makes old structures obsolete.
3. Migration and cleanup operations provide dry-run, audit and resumability.
4. No source file or RAW file is deleted automatically.
5. External identifiers identify source records; they are not user-facing filenames.
6. Import and synchronization failures are visible, durable and retryable.
7. Perceptual similarity alone must never authorize destructive merging.
8. The highest-quality known copy is the canonical master; lower-quality copies may
   contribute metadata without replacing it.
9. Every implementation milestone ends with proportionate automated tests and a
   signed-off commit.

## 3. Target domain model

The current `photos` row combines logical identity, file storage and import state. The
new model separates them while retaining compatibility with existing IDs and APIs.

```text
Asset (one logical photo)
  |- Source item: Apple Photos PHAsset
  |- Source item: Google Photos or Takeout record
  |- Source item: legacy PicManager/local import
  |
  |- Variant: original/master HEIC or JPEG
  |- Variant: Apple Photos current edited rendition
  |- Variant: Google recompressed copy
  |- Variant: generated preview or thumbnail
  |
  `- PicManager metadata
       |- faces and people
       |- albums and collections
       |- geography
       |- activity associations
       `- user corrections
```

The initial migration keeps `photos.id` stable. New source and variant tables link to
the current row, allowing handlers to move gradually to the new model.

Proposed foundational entities:

- `assets`: stable logical identity and canonical photo link.
- `asset_sources`: provider-specific identity, original filename and source metadata.
- `asset_variants`: stored representations, quality role, dimensions, size and hash.
- `sync_jobs`: durable operation-level status and progress.
- `sync_items`: per-source-item state, attempts and diagnostic details.
- `asset_links`: candidate/exact links between legacy photos and source items.
- `migration_runs`: resumable migration and verification audit records.

## 4. Complete six-phase roadmap

### Phase 0 - Contract, safety baseline and migration design

Define what PicManager preserves and displays before changing ingestion.

Deliverables:

- Add the multi-source asset/source/variant schema with constraints and indexes.
- Preserve all existing photo IDs and relationships.
- Define source providers, variant roles, lifecycle states and quality ranking.
- Implement migration run records and dry-run/reporting foundations.
- Build a representative media fixture matrix: normal/rotated/mirrored HEIC, edited
  photo, screenshot, duplicate filename, Live Photo, RAW+JPEG and RAW-only.
- Record baseline database counts and integrity checks before migration.
- Define backup, restore and reconciliation acceptance tests.

Exit criteria:

- Existing databases migrate without moving media files.
- Old APIs and all pre-existing tests continue to pass.
- A migration report can describe what would be backfilled without writing changes.

### Phase 1 - Reliable Apple Photos inventory, migration and synchronization

Replace the staging-directory workflow with a durable source inventory and retryable
sync pipeline.

Deliverables:

- Enumerate lightweight PhotoKit inventory into `asset_sources`.
- Preserve `PHAsset.localIdentifier` as source identity and
  `PHAssetResource.originalFilename` as the user-visible filename.
- Match existing UUID-named PicManager files back to Apple assets using the legacy
  identifier transformation.
- Add secondary matching by SHA-256, filename/time/dimensions and perceptual evidence.
- Store confidence and require confirmation for ambiguous matches.
- Persist discovered changes before advancing the PhotoKit change token.
- Retry failed downloads/imports after restart with bounded exponential backoff.
- Add periodic full reconciliation to detect missed or orphaned assets.
- Add source inventory and sync-status APIs.
- Add an Apple Photos UI with filters for all, synced, missing, queued, failed and
  excluded assets, including retry actions.
- Remove manual staging-directory management from the normal user workflow.

Exit criteria:

- Killing either process during export/import does not lose or duplicate an asset.
- Advancing a change token cannot strand a failed item.
- Existing library files and metadata are linked without destructive rewrites.
- The UI can identify every Apple Photos item that is not ready in PicManager.

### Phase 2 - Apple Photos fidelity and representation policy

Represent both the camera original and the current Photos appearance where necessary.

Deliverables:

- Save the original resource as an immutable master variant.
- Detect adjusted assets and produce a current rendition that includes Photos edits.
- Default browsing to the current rendition while allowing original inspection/export.
- Centralize EXIF orientation, HEIF IROT, mirroring and user transforms so display
  orientation is applied exactly once.
- Replace silent exiftool fallbacks with explicit capabilities and visible errors.
- Track rendition provenance, dimensions, encoding, color metadata and generation time.
- Define behavior for Live Photos, RAW+JPEG and RAW-only assets.
- Invalidate derived thumbnails and face embeddings when the display variant changes.

Exit criteria:

- Fixture results visually agree with Photos for rotation, mirroring and common edits.
- Original files remain byte-preserved.
- Reprocessing is idempotent and does not accumulate orientation transforms.

### Phase 3 - Modern high-density web experience

Replace the monolithic frontend with a typed, modular application built as static
assets and embedded by the Rust binary.

Deliverables:

- Adopt TypeScript and Vite with a component framework (Svelte is preferred unless a
  spike demonstrates a material disadvantage).
- Add design tokens and reusable layout, dialog, menu and selection components.
- Build a date-grouped, justified high-density photo timeline.
- Add virtual scrolling, cursor pagination, responsive image sizing and prefetching.
- Add density controls, sticky date headers, keyboard navigation and Shift selection.
- Build an immersive viewer with metadata drawer and original/current switching.
- Redesign albums, people, locations, activities and task progress around browsing
  instead of administration.
- Add the Apple Photos inventory/sync view and a unified task center.
- Keep existing capabilities available throughout incremental migration.
- Meet accessibility, responsive-layout and error-state requirements.

Exit criteria:

- Warm local first view is usable within one second on the reference library.
- A 100,000-item synthetic catalog scrolls without DOM growth proportional to catalog
  size.
- Core photo, album, people, location, activity and sync workflows have UI tests.

### Phase 4 - Service boundaries and unified background work

Consolidate business logic that currently lives in import functions and HTTP handlers.

Deliverables:

- Introduce service and repository boundaries shared by Web, CLI and macOS integration.
- Move import, thumbnails, AI, geocoding and deduplication onto the durable job system.
- Add cancellation, retry policy, concurrency limits and structured diagnostics.
- Enable SQLite WAL, busy timeout, explicit pool sizing and reviewed transaction scopes.
- Add filesystem/database reconciliation and automatic database backups.
- Apply consistent lifecycle filtering and authorization-ready API boundaries.

### Phase 5 - macOS product integration

Package the system as a normal Mac application without duplicating the frontend.

Deliverables:

- SwiftUI menu bar application and first-run assistant.
- PhotoKit authorization, library selection and sync status.
- Rust service lifecycle, health checks and native notifications.
- Embedded WKWebView or system-browser launch option.
- Launch-at-login support, logs and diagnostic export.
- Prepare signing, notarization and update distribution.

### Phase 6 - Secure multi-device and external-source access

Open the stable local product to trusted devices and additional providers.

Deliverables:

- Authentication, sessions, CSRF protection and authorization policy.
- HTTPS or a documented trusted private-network proxy such as Tailscale.
- Optional read-only clients and bandwidth-aware rendition delivery.
- Google Photos Picker ingestion for explicitly selected content.
- Google Takeout media/JSON inventory import and metadata comparison.
- Cross-source quality comparison without replacing masters with recompressed copies.

Google Photos constraint: since March 31, 2025, the Library API can only list and
retrieve app-created content. Full-library integration therefore uses user-selected
Picker sessions or offline Takeout imports rather than promising continuous account-wide
enumeration.

## 5. Existing-library migration strategy

Migration is an in-place catalog upgrade, not a media rewrite.

### Backfill order

1. Create an `asset` for each existing imported photo while retaining `photos.id`.
2. Create a `legacy_local` source and original variant from the current path/hash.
3. Enumerate Apple Photos inventory without downloading full resources.
4. Match the legacy UUID-style basename against the sanitized PhotoKit identifier.
5. For unmatched items, compare exact SHA-256 where available.
6. Use filename, timestamp, dimensions, camera and GPS as structured evidence.
7. Use pHash only as additional evidence for transformed or recompressed copies.
8. Record ambiguous candidates for review rather than merging automatically.
9. Verify counts, foreign keys, active-photo totals and file existence.

### Preserved metadata

- faces, embeddings and person assignments;
- album and collection membership;
- geocoding and GPS metadata;
- activity associations;
- manual timestamps, orientation and flips;
- deduplication decisions and import state.

### Migration commands

The intended operator workflow is:

```text
picmanager migrate inspect
picmanager migrate backfill-local --dry-run
picmanager migrate backfill-local
picmanager migrate link-apple --dry-run
picmanager migrate link-apple
picmanager migrate verify
picmanager migrate report
```

## 6. RAW retention policy

The default product policy favors finished JPEG/HEIC media for non-professional use:

| Source case | Default behavior |
| --- | --- |
| RAW + JPEG/HEIC | Import/display JPEG or HEIC; record RAW as an optional companion |
| RAW-only | Report explicitly; optionally render a high-quality finished copy |
| Existing RAW | Produce a reclaimable-space report; never delete automatically |
| Ambiguous pairing | Retain and require review |

Cleanup is a separate, reversible workflow:

1. verify pairing and capture time;
2. verify the finished copy is imported and readable;
3. verify backup policy;
4. move RAW to a quarantine/review location;
5. retain it for a configurable grace period;
6. delete only after explicit confirmation.

PhotoBridge currently skips RAW alternates and RAW-only assets. Modernized inventory
must expose these as policy-excluded or review-required rather than silently omitting
them.

## 7. Phase 0-3 implementation milestones

Each milestone must leave the repository buildable, run proportionate tests and end in
one signed-off commit with an English subject and detailed English body.

### M0 - Planning and safety baseline

- M0.1: commit this roadmap and development rules.
- M0.2: add schema/domain decision records and migration invariants.
- M0.3: add baseline integrity/reporting tests for existing databases.

### M1 - Multi-source catalog foundation

- M1.1: additive asset/source/variant migrations, constraints and indexes.
- M1.2: Rust domain types and repository operations.
- M1.3: legacy-local backfill with dry-run and audit records.
- M1.4: verification/report commands and compatibility tests.

### M2 - Durable sync jobs

- M2.1: sync job/item schema and lifecycle state machine.
- M2.2: retry, lease/recovery and token-checkpoint semantics.
- M2.3: task APIs and structured error reporting.
- M2.4: restart/interruption integration tests.

### M3 - Apple Photos inventory and reconciliation

- M3.1: PhotoBridge inventory DTOs and original-filename extraction.
- M3.2: inventory ingestion and exact legacy-ID linking.
- M3.3: evidence-based secondary matching and review candidates.
- M3.4: incremental discovery-before-token checkpointing.
- M3.5: full reconciliation and missing/failed/excluded status APIs.
- M3.6: Apple Photos inventory UI and retry controls.

### M4 - Representation fidelity

- M4.1: original/current variant policy and adjusted-asset detection.
- M4.2: current rendition export and provenance.
- M4.3: centralized orientation/transform pipeline.
- M4.4: derived-artifact invalidation and reprocessing.
- M4.5: fixture matrix and visual/metadata regression tests.

### M5 - Frontend platform

- M5.1: TypeScript/Vite/Svelte scaffold embedded by Rust.
- M5.2: API client, routing, state and design-system primitives.
- M5.3: coexistence bridge for workflows not yet migrated.

### M6 - Modern photo browsing

- M6.1: cursor-paginated timeline API and responsive rendition contract.
- M6.2: virtualized justified date timeline and density control.
- M6.3: selection, keyboard navigation and batch action shell.
- M6.4: immersive viewer, metadata drawer and variant switching.

### M7 - Feature-view migration

- M7.1: albums and collections.
- M7.2: people and face workflows.
- M7.3: locations and map.
- M7.4: activities.
- M7.5: deduplication and unified task center.

### M8 - Phase 0-3 hardening and release candidate

- M8.1: accessibility, responsive and failure-state pass.
- M8.2: 100,000-item performance fixture and budgets.
- M8.3: migration rehearsal against a copied real-world database.
- M8.4: documentation, upgrade guide and full release verification.

## 8. Testing and commit protocol

For every milestone:

1. inspect the worktree and preserve unrelated user changes;
2. add unit tests for domain behavior;
3. add migration/integration tests for persistence behavior;
4. add Swift tests for PhotoKit-independent logic;
5. add frontend component/end-to-end tests for user workflows;
6. run targeted tests during development;
7. run the milestone's complete affected test suites before committing;
8. review the diff and migration safety;
9. commit with `git commit -s`.

Commit messages use a one-line English subject followed by a detailed English body:

```text
feat(sync): persist Apple Photos inventory

Store PhotoKit asset identities and original filenames in the source catalog.
Add reconciliation states and retry-safe inventory ingestion.
Cover exact legacy identifier matching with migration integration tests.
```

Phase completion additionally requires the full Rust and Swift suites, frontend build and
tests, migration verification and a documented manual smoke test.
