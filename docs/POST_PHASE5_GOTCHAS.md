# Post-Phase-5 stabilization gotchas

This document records failures found after Phase 5 and the constraints future
refactors must preserve. Read it before changing startup, macOS presentation,
geographic browsing, embedded assets, or background work.

## macOS service and application lifecycle

### A matching version is not necessarily the matching binary

The Mac shell previously reused a managed Rust service solely because its semantic
version matched the bundled service. A rebuilt app could therefore continue running
stale code. Managed-service refresh must compare binary content as well as version.
Tests must cover a same-version binary replacement.

### Readiness and reconciliation are different phases

Deep catalog/filesystem reconciliation can take long enough to exceed native startup
timeouts. The service must bind its listener and expose the selected-library contract
before scheduling expensive reconciliation as durable background work. Do not put a
full-library scan back on the readiness path or increase timeouts to hide it.

### Quit can interrupt work

Closing the app stops its owned service; it does not keep an invisible process running
until every operation finishes. Long operations must use durable jobs, checkpoints,
retry-safe handlers, cancellation states, and startup lease recovery. Never depend on
an in-memory task surviving app exit.

### A menu-bar app still needs normal window semantics

`LSUIElement` accessory applications do not appear in Dock or Command-Tab. PicManager
switches to regular activation while the Library window is visible and returns to
accessory activation after it closes. Preserve this adaptive activation instead of
making the whole app permanently regular or permanently hidden.

### Bundles must contain, refresh, and sign every executable

Web changes require a production frontend build and a new embedded Rust binary. The
app bundle must then be rebuilt and signed, including the nested service. Validate the
bundle and clean-install/upgrade/uninstall lifecycle; copying an old `PicManager.app`
does not exercise new code.

## Browser and API scalability

### Payload bounds do not automatically bound DOM work

The original Places map returned about 5.75 MB and created one button per GPS photo;
56,000 photos blocked both entry and teardown for seconds. Map APIs must aggregate on
the server for the requested viewport/zoom, and UI tests must assert a bounded marker
count even when the represented photo total is much larger. A test that only proves a
zoomed viewport is smaller can still miss every photo: initialize the map around real
cluster data and assert the focused viewport retains the expected cluster.

### Navigation teardown is part of the performance budget

A view can be slow not only while loading but also while destroying a large component
tree. Measure entering and leaving heavy tabs. Keep the Places view proportional to
visible clusters and hierarchy nodes, not total library size.

### Responsive measurement must not reset pagination

Replacing a scrollable photo grid with a loading placeholder can remove its scrollbar.
That width change may trigger a responsive `ResizeObserver`, recalculate the page size
and issue a new page-one request that supersedes the user's next-page request. Preserve
the current bounded grid while paging, disable repeated navigation and reserve a stable
scrollbar gutter. Tests should hold a later-page response in flight and assert that the
current page remains mounted until the requested replacement arrives.

### Product vocabulary is not automatically stored vocabulary

The UI calls date-derived albums “month” albums, but existing databases and the Rust
API store their kind as `time`. A refactor that grouped only `month` passed synthetic
component tests while sending real date albums to the fallback section. Before changing
an enum-like field, inspect migrated data, SQL producers and API fixtures; accept legacy
values at compatibility boundaries and make tests use at least one production value.

### Display labels are not query identities

Converting SQL `NULL` to the label `Unknown` destroyed the information needed to query
those photos. API hierarchy nodes carry a separate `query_value`; `__null__` maps to
`IS NULL`. Future labels, translations, aliases, and user-facing formatting must never
replace stable query or provider identities.

### Localization rules need versions and repair paths

`Accept-Language` alone is not a migration strategy. Geographic cache rows record the
name-policy revision. New lookups prefer Simplified Chinese, other Chinese variants,
English, then the provider default. Existing rows are repaired only by an explicit,
durable, repeatable task. Failed provider calls preserve old names and leave them
eligible for retry.

### macOS system proxy settings are not process proxy variables

A service launched by the Mac app does not automatically receive the manual proxies
configured in System Settings. During the first name-normalization run, direct
Nominatim requests timed out after ten seconds while the configured system HTTPS proxy
answered in under one second. The Mac shell must translate enabled HTTP, HTTPS and
SOCKS settings into standard child-process variables while preserving explicit
overrides and localhost exclusions. Proxy settings are captured when the service
starts; restart the app after changing them.

### Remote maintenance must count durable work units, not consumers

The first normalization job treated 56,340 photos as work even though they referenced
far fewer coordinates. It scanned in photo order, made poor use of the 1 km cache and
restarted from zero after a failed attempt. Geographic maintenance now groups by
canonical coordinate, sorts spatially, updates all matching photos transactionally and
uses successful cache-policy revisions as restart checkpoints. Apply the same pattern
whenever many records consume one remotely derived value.

### Progress telemetry must not be able to kill the work

Writing unchanged progress four times per second increased SQLite contention until a
progress update failed and restarted a multi-hour job. Publish only changed progress at
a bounded interval, retry transient lock errors, and separately retry worker lease
contention. Provider failures need a circuit breaker, and cancellation must be checked
between bounded remote calls. Never report a timed-out lookup as successfully updated.

### Expired job leases require continuous recovery

Startup-only lease recovery has a race: a replacement service can start seconds before
the previous process's lease expires, miss it during startup recovery, and leave the job
marked as running forever. Worker runtimes must continue scanning for expired leases at
a bounded interval. Tests must cover a lease that is valid at startup and expires only
after workers are already polling.

Lease heartbeats are writes and can encounter the same transient SQLite contention as
progress updates and job acquisition. Retry heartbeat writes within a bounded window;
otherwise one short lock can abandon healthy work and consume an entire retry attempt.

Heartbeat retries cannot compensate for a handler that owns SQLite's single writer lock
for an entire full-library scan. Resolve expensive read scopes before beginning a write
transaction, then apply derived-state changes in small atomic batches. This preserves
per-photo reconciliation while leaving regular lock gaps for leases, progress and other
interactive writes.

Periodic crash recovery must also distinguish a dead prior-process lease from a locally
active handler whose heartbeat is temporarily blocked. Track active worker owners in the
runtime, exclude only those owners from that process's recovery pass, and continue the
handler on transient database errors. A replacement process starts with an empty active
set, so genuinely abandoned leases remain recoverable after restart.

Do not await heartbeat retries inside the same `select` branch that polls a handler. If
the handler is suspended while it owns the writer transaction that blocked the heartbeat,
the retry loop creates a self-deadlock. Run heartbeat renewal in an independent task so
the handler can finish and release its transaction while renewal waits.

The same rule applies inside a handler that selects between core work and progress
publication. Geographic execution must continue in an independently owned task while
progress writes wait, and that task must be aborted if the owning durable handler is
dropped. Optional telemetry must never pause the operation that can release its lock.

## Data and derived-state safety

### External IDs, filenames, labels, and paths are different concepts

Apple provider identifiers are stable identities, not filenames. Preserve original
filenames separately and never derive user-visible filenames from UUID-like IDs.
Likewise, geographic labels are presentation data and must not become database keys.

### Current appearance is not always the original bytes

Apple Photos edits, orientation, crops, and format variants must be represented
explicitly. Never replace a higher-quality original with a recompressed provider copy.
RAW+JPEG policy decisions remain separate from import fidelity and require an explicit
user-approved cleanup workflow.

### Derived relationships must be reconciled transactionally

When a normalized place name changes, replace that photo's derived location-album
membership in one transaction and delete only empty derived albums. Do not bulk-delete
photos, user collections, source identities, or media to clean derived state.

### Migrations must be additive and network-free

Schema migration may mark existing data as needing maintenance, but startup migration
must not call external providers or erase usable cached values. Provide status, an
explicit action, durable progress, safe retry, and copied-library rehearsal.

## Development workflow traps

- Run frontend commands from `web/`; the repository root has no `package.json`.
- Do not run `rustfmt` on a module root such as `src/web/mod.rs`; it can recursively
  reformat unrelated legacy modules. Format only intentional files and inspect the
  diff immediately.
- Preserve generated `frontend/assets` after `npm run build`; Rust embeds those files.
- Use isolated temporary libraries for browser and migration tests. Synthetic photo
  paths can trigger thumbnail jobs, so expect and contain those failures in test data.
- Keep signed, independently tested milestones and fast-forward them into `main`.
- Before declaring a performance fix, compare API size/latency, rendered node count,
  and both entry and exit interaction latency on a large synthetic library.

## 2026-08-16 performance incident summary

Several independent costs looked like one "background task uses all CPU" failure. Diagnose
the running job table, process CPU and slow-query log separately before blaming the visible task.

- Large photo sets must use replacement pagination, not cumulative "load more" DOM growth.
  Derive page size from actual grid columns times complete rows so bounded pages also render
  without partial final rows.
- Optional telemetry must run independently from transaction-owning work. Heartbeat or progress
  retries inside the same `select` branch can suspend the future that must release SQLite's
  writer lock, creating a self-deadlock that resembles CPU or database overload.
- Frequent health endpoints must be constant-time. The native shell polls every five seconds;
  running `PRAGMA quick_check` there repeatedly scanned a roughly 600 MB catalog and kept the
  service busy even when no durable task was active. Integrity and filesystem checks belong to
  explicit deep diagnostics.
- Correlated presentation subqueries multiply with catalog size. Computing a location parent
  independently for every album produced observed 39- and 123-second `/api/albums` queries.
  A grouped location pass plus set-based album statistics reduced the same query to 0.40 seconds
  on a copied existing catalog.
- Composite index order must match traversal direction. `photo_albums(photo_id, album_id)`
  protects uniqueness and photo-first lookup but does not efficiently serve album-first browsing;
  keep the additive `(album_id, photo_id)` index as well.
- Remote geocoding throughput and interactive database throughput are separate concerns. Provider
  HTTP 429 responses should trip the circuit breaker and stop safely; they must not trigger busy
  retries that consume local CPU or disguise a rate limit as database contention.

## Refactor review checklist

Before merging related work, answer all of the following:

1. Can app exit interrupt this operation, and is recovery durable?
2. Does startup expose readiness before deep maintenance begins?
3. Are labels separate from stable identities and null semantics?
4. Are API results and DOM nodes bounded for a 100,000-photo library?
5. Does migration preserve usable old data when offline?
6. Are derived relationships updated transactionally and narrowly?
7. Was the embedded frontend, Rust service, app bundle, and nested signature rebuilt?
8. Were tests run only against isolated or copied libraries?
9. Do enum-like test fixtures include the values stored by existing catalogs?
10. Does a Mac child process receive required system proxy settings explicitly?
11. Are remote work units deduplicated, checkpointed, cancellable and failure-aware?
12. Can optional progress reporting fail without restarting completed work?
