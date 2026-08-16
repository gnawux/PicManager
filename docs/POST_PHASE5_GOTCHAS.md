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
count even when the represented photo total is much larger.

### Navigation teardown is part of the performance budget

A view can be slow not only while loading but also while destroying a large component
tree. Measure entering and leaving heavy tabs. Keep the Places view proportional to
visible clusters and hierarchy nodes, not total library size.

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

