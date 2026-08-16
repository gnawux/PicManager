# PicManager Modernization Plan

Implementation status: Phase 0–5 and M18 stabilization are complete and merged into
`main`. The additive catalog, reliable Apple Photos inventory, rendition model, durable
services, modern Web application, and macOS product are implemented. Phase 6 is
intentionally deferred.

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

Status: complete.

- M8.1: accessibility, responsive and failure-state pass.
- M8.2: 100,000-item performance fixture and budgets.
- M8.3: migration rehearsal against a copied real-world database.
- M8.4: documentation, upgrade guide and full release verification.

## 8. Phase 4-5 implementation milestones

Phase 4/5 work begins from the Phase 0–3 release candidate merged into `main`. Minor
visual and interaction defects are recorded but intentionally deferred until the
architectural work and macOS integration are complete.

### M9 - Phase 4/5 planning and integration baseline

- M9.1: merge Phase 0–3 into `main`, record repository agent rules and expand this plan.
- M9.2: capture service-boundary and job-system contracts with compatibility tests.

### M10 - Shared application services

- M10.1: introduce application context, repository traits and typed service errors.
- M10.2: route import and catalog mutations through services shared by CLI and Web.
- M10.3a: route deduplication scans, review and resolution through services.
- M10.3b: route photo metadata and display-transform mutations through services.
- M10.3c: route curated collection mutations through services and transactions.
- M10.4: add authorization-ready request context without enabling remote access yet.

### M11 - Unified durable background work

- M11.1: add additive generic job, attempt, lease and diagnostic persistence.
- M11.2: implement a worker registry, bounded concurrency and graceful shutdown.
- M11.3a: move CLI and Web imports onto durable jobs with persisted summaries.
- M11.3b: move thumbnail generation onto revision-keyed durable jobs.
- M11.4: move face/animal analysis and geocoding onto durable jobs.
- M11.5: move deduplication and derived-media maintenance onto durable jobs.
- M11.6: expose consistent queue, progress, cancellation, retry and failure APIs.

### M12 - Database and filesystem reliability

- M12.1: enable SQLite WAL, busy timeout, explicit pool sizing and connection tests.
- M12.2: review transaction scopes and add contention/restart integration tests.
- M12.3: implement atomic SQLite backups with retention and restore verification.
- M12.4: reconcile catalog variants, media files, staging intents and derived caches.
- M12.5: add startup recovery and structured health/diagnostic reports.

### M13 - Phase 4 API and operational hardening

Status: complete. Release-gate evidence is recorded in `PHASE4_RELEASE_GATE.md`.

- M13.1: apply consistent active/deleted/source lifecycle filtering across APIs.
- M13.2: version service-facing health, jobs and diagnostics contracts for macOS use.
- M13.3: add worker metrics, structured logs and end-to-end interruption tests.
- M13.4: run the Phase 4 compatibility, performance and migration release gate.

### M14 - macOS application foundation

Status: complete.

- M14.1: add a SwiftUI menu-bar application target and shared configuration model.
- M14.2: implement first-run library selection and PhotoKit authorization assistant.
- M14.3: display source inventory, synchronization and background-task health.

### M15 - Rust service lifecycle integration

Status: complete.

- M15.1: locate or install the bundled Rust service and validate compatible versions.
- M15.2: start, monitor and gracefully stop the local service with crash recovery.
- M15.3: add health polling, native failure notifications and safe log capture.
- M15.4: prevent two app instances or workers from owning the same library.

### M16 - Native shell and daily operation

Status: complete.

- M16.1: add embedded WKWebView and system-browser launch modes.
- M16.2: add menu-bar task progress, retry shortcuts and synchronization controls.
- M16.3: add launch-at-login management using current macOS service APIs.
- M16.4: add diagnostic export with configuration redaction and no media inclusion.

### M17 - Distribution preparation

Status: complete. External signing credentials and publication remain release-operator actions.

- M17.1: define app bundle resources, entitlements and release build assembly.
- M17.2: add signing/notarization scripts that require explicit external credentials.
- M17.3: define update metadata and rollback compatibility without auto-publishing.
- M17.4: test clean installation, first run, upgrade and uninstall preservation.

### M18 - Phase 4/5 stabilization and documentation

- M18.1: complete — bind native clients to the selected library and restart safely after configuration changes.
- M18.2: complete — reorganize architecture, operations, macOS, migration and user documentation.
- M18.3: complete — Rust, Swift, frontend, migration, performance, bundle, installation and browser gates passed locally; external signing and manual PhotoKit checks remain operator actions.
- M18.4: complete — merged the Phase 4/5 branch into `main` with signed-off merge commit `414e35d`.

### M19 - Geographic naming consistency

Status: complete.

- M19.1: version the geographic naming policy and prefer Simplified Chinese, other Chinese variants, English variants, then the provider default.
- M19.2: provide explicit, retryable background normalization for legacy caches without startup network work or destructive fallback.
- M19.3: reconcile derived location albums and expose normalization status and controls in the Places view.

### M20 - Post-Phase-5 browsing stabilization

Status: complete.

- M20.1: record startup, shutdown, packaging, high-volume UI, geographic NULL handling,
  localization and test-isolation lessons in `POST_PHASE5_GOTCHAS.md` and make them
  required reading for related refactors.
- M20.2: add bounded viewport geographic queries and paginated cluster-photo lookup.
- M20.3: replace the static geographic overview with a pannable OpenStreetMap tile map,
  zoom-dependent clusters, cluster photo browsing and a full-window mode.
- M20.4: sort the geographic hierarchy by photo count, fold it at country and state
  boundaries, and isolate navigation and result scrolling.
- M20.5: reorganize collections and smart albums into collapsible month, location,
  camera and fallback categories with persistent split-pane photo browsing.

The OpenStreetMap map is an optional network-backed presentation layer. Catalog GPS
coordinates and photo results remain local; tile requests contain ordinary map tile
coordinates for the visible viewport and never upload media.

### M21 - Geographic maintenance reliability

Status: complete.

- M21.1: propagate enabled macOS manual proxy settings to the bundled Rust service
  while preserving explicit process overrides and local bypasses.
- M21.2: normalize names by sorted unique coordinate, use cache revisions as restart
  checkpoints and transactionally reconcile every photo sharing that coordinate.
- M21.3: add provider circuit breaking, bounded cancellation, changed-only progress
  publication and transient SQLite retry for progress and worker leases.
- M21.4: cover system proxy mapping, 5,000-photo coordinate collapsing, provider
  failure, cancellation, contention and real isolated proxy-backed normalization.

### M22 - Paginated photo-set browsing

Status: complete.

- M22.1: complete — paginate geographic hierarchy results and protect rapid selection changes
  from stale responses.
- M22.2: complete — paginate smart albums, curated collections and person photo sets while
  preserving bounded initial rendering.
- M22.3: complete — bound activity-photo responses and provide the same incremental browsing
  behavior for unusually photo-heavy activities.
- M22.4: complete — rebuild the embedded frontend and Rust service, ad-hoc sign the
  macOS application, and verify bundle plus clean-install/upgrade/uninstall safety.

### M23 - Continuous expired-lease recovery

Status: complete.

- M23.1: complete — recover job leases that expire after service startup so interrupted durable
  work cannot remain indefinitely marked as running.
- M23.2: complete — verify the startup timing race, rebuild the macOS bundle and rehearse recovery
  against an isolated catalog before operating on a personal library.
- M23.3: complete — retry transient heartbeat-write contention so a healthy long-running
  job does not lose its lease because of one short SQLite lock.

### M24 - Album ordering and grid-aware pagination

Status: complete.

- M24.1: complete — add count, latest-photo and localized-name ordering for smart albums and
  curated collections.
- M24.2: complete — replace cumulative photo loading with bounded previous/next page navigation
  across albums, places, people and activities.
- M24.3: complete — derive each page size from the rendered column count and a fixed number of
  complete rows, recalculating safely when the result pane changes width.
- M24.4: complete — qualify location albums with a unique non-null parent region while preserving
  concise names for places such as Hong Kong whose parent region is absent.

### M25 - Geographic writer-lock hardening

Status: complete.

- M25.1: complete — resolve photos sharing a coordinate before opening a write transaction so
  full-library coordinate scans cannot starve durable-job heartbeats.
- M25.2: complete — reconcile derived location-album membership in bounded atomic batches,
  preserving per-photo consistency while regularly releasing SQLite's writer lock.
- M25.3: complete — verify duplicate-coordinate behavior in isolation and observe multiple lease
  renewals plus forward normalization progress against the existing library.

## 9. Testing and commit protocol

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
