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

### PhotoKit exports must not inherit the UI actor

The command-line exporter and the Mac shell use the same `PHAssetResourceManager`
write API, but the shell's synchronization coordinator is `@MainActor`. Running the
PhotoKit asset lookup directly from that coordinator left a claimed cloud resource in
`downloading` without even creating its staging directory, while the WebKit page kept
refreshing normally. Run PhotoKit export work in a detached utility task, preserve the
durable lease heartbeat, and surface a retryable failure rather than treating a
repainting web page as evidence of progress.

### Bundled helpers need retained process ownership and explicit signing

An async wrapper that creates `Process` only inside a continuation can lose reliable
termination delivery after a short-lived helper exits. Keep the process strongly owned
until `waitUntilExit()` returns from a detached utility task, then translate its exit
status into the durable retry state. Sign the bundled PhotoBridge helper explicitly with
its Photos entitlement and verify it in the bundle release gate; sealing a linker-signed
resource inside the outer app is not an adequate nested-code contract.

### Snake-case conversion does not preserve Swift acronym spelling

`JSONDecoder.KeyDecodingStrategy.convertFromSnakeCase` maps `item_id` to `itemId`,
not `itemID` (and likewise `external_id` to `externalId`). A service can therefore
commit a lease successfully while the native client fails to decode the response and
never starts the work. Give acronym-bearing wire properties explicit coding keys and
test them against literal server JSON, including side-effecting claim responses.

### Debug cross-component state transitions before subsystem internals

The Apple export incident looked like a PhotoKit or iCloud stall because the source row
said `downloading`. In fact, the Rust service had committed the claim and the Swift
client had failed to decode its response, so no helper or media I/O had started. The
following evidence order found the fault quickly and should be the default incident
workflow:

1. Hash the installed and newly built executables, including managed services and
   helpers, so stale binaries are excluded first.
2. Inspect durable source and item state separately. A lease proves only that the server
   mutated its database; it does not prove that the client decoded the response or began
   I/O.
3. Check the expected process, staging package and lease heartbeat. Their simultaneous
   absence places the break before export without speculating about network speed.
4. Sample the live process to distinguish a blocked thread from an async path that
   already failed or disappeared.
5. Test the first unproven boundary with literal producer data. For a side-effecting API,
   verify both the server mutation and consumer decoding before investigating downstream
   frameworks.

Prefer timestamped durable facts and falsifiable boundary checks over adding broad logs
or changing several layers at once. Keep hypotheses separate: package permissions,
PhotoKit access, helper lifecycle and wire decoding require different evidence.

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

### Credential sheets and bundled runtimes are cross-process contracts

The Garmin sign-in regression had two independent symptoms: an unconstrained `NSAlert`
accessory could collapse its account/password controls, while bundled Python created
`__pycache__` beneath `Contents/Resources` and invalidated the outer code seal.

- Use a sized native sheet for credentials and return an explicit `saved`, `cancelled`,
  or `error` event object to WebKit. The Web UI must consume the outcome rather than
  inferring success from the click that opened the form.
- Keychain records are keyed by both service and provider account. Update an exact
  record in place; migrate a former service-only lookup by copying it to the requested
  account without broad deletion.
- A credential save is not applied until the owned service has stopped and restarted
  with the new environment. Surface a saved-but-restart-failed result explicitly.
- Run bundled Python with `-B` and `PYTHONDONTWRITEBYTECODE=1`. The release gate must
  execute an offline helper invocation after app signing and then run strict signature
  verification; a successful pre-launch seal alone is not sufficient.
- Provider errors cross Python, Rust, and Web as stable error codes. Log only redacted
  exception class and HTTP status, never provider response text, account identifiers,
  passwords, token bytes, or URLs containing credentials.

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

### Fullscreen surfaces need an explicit overlay hierarchy

`position: fixed` does not imply that the newest overlay is on top. The fullscreen map
and photo viewer are siblings with independent `z-index` values, so a viewer can open
successfully while remaining hidden behind the map. Keep their layer priorities explicit
and test the complete fullscreen-map-to-photo interaction, including the relative layers.

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

### A committed database path must outlive staging cleanup

The native Apple exporter originally committed paths that still pointed into
`.sync-staging/apple`, then deleted the package after the server returned success. The transaction,
job state and source count all looked correct while thumbnails and full media immediately became
unreadable. Component tests missed this because their temporary package remained alive until the
test ended.

- Define ownership and lifetime for every path crossing a process boundary: transient input,
  durable media or regenerable cache.
- Move a verified package atomically into its durable library location before publishing any path
  in SQLite. A database commit must never be the only thing making transient bytes appear durable.
- Test the caller's complete lifecycle, including its documented post-success cleanup, and verify
  both the compatibility photo path and selected rendition path remain readable afterward.
- Make persistence idempotent. A database rollback after an atomic filesystem move may leave an
  orphan package; retries must recognize and safely reuse the same content identity.
- Recovery should inspect only records owned by the affected workflow, avoid a whole-library walk,
  and requeue missing originals without deleting or overwriting unrelated media.

### Provider ingestion must finish the same post-import contract

The Apple rendition path initially created a readable photo row but skipped the legacy importer's
EXIF, GPS, camera-album and thumbnail work. A full-screen file response therefore succeeded while
the timeline stayed blank and the metadata drawer remained sparse. Every provider-specific commit
must schedule durable post-processing from the persisted original, not assume that creating the
compatibility photo row completes ingestion. Recovery must target provider-owned variants and remain
safe to repeat after an interrupted app session.

Thumbnail idempotency keys identify one photo revision and size, but a failed job must not permanently
poison that key after its underlying media path is repaired. Re-enqueue terminal failures explicitly,
record terminal render state, and propagate catalog-state write failures instead of reporting a job as
successful while `derived_media_state` remains pending.

### Provider source state and item state form one transition

Cancelling a sync job while leaving its sources marked `queued` creates work that the UI counts but no
worker can claim. Update item and source states in the same transaction, and reconcile historical
terminal items at startup. A cancelled unsynchronized source returns to `discovered`; a source that
already owns a committed asset remains `ready`.

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
