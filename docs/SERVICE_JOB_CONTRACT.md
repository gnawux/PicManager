# Service and durable-job contract

Status: accepted baseline for Phase 4 and the Phase 5 macOS client.

## Layer boundaries

PicManager has one application core with three adapters:

```text
CLI              Axum HTTP API              macOS local client
 |                    |                            |
 +----------- typed application services --------+
                      |
             durable job coordinator
                      |
        repositories + filesystem gateway
                      |
             SQLite + managed media
```

Adapters validate transport input and translate service responses. They do not contain
catalog SQL, filesystem mutation sequences, retry policy or background execution logic.
Services accept typed commands and an explicit request context. Repositories own SQL and
transaction boundaries. Filesystem gateways own path validation, atomic placement and
reconciliation. Long operations are represented by durable jobs before work begins.

## Compatibility rules

- Existing CLI commands and `/api` routes remain available while their implementation
  moves behind services.
- Existing response fields remain stable. New fields are additive until a versioned API
  explicitly replaces a contract.
- `photos.id`, existing job IDs and provider source identities remain stable.
- Active-library reads consistently exclude `import_status = 'deleted'` unless a caller
  explicitly requests an administrative lifecycle view.
- CLI commands may wait for a queued job and print its final summary; Web and macOS
  callers receive the durable job ID immediately and observe progress separately.
- A service error has a stable code, safe user message, retryability classification and
  optional structured details. Internal causes remain in structured logs.

## Request context

Every service command carries a context with:

- caller kind: CLI, local Web, macOS app or internal worker;
- request/correlation ID;
- library identity;
- local authorization scope;
- optional idempotency key.

Phase 4 keeps the current local-trusted policy, but services must not infer authority
from HTTP handler location. Phase 6 can add authenticated principals without moving
business logic again.

## Generic job model

A job is one durable application operation. It contains:

- stable kind and schema version;
- JSON payload validated by the registered worker;
- lifecycle status and priority;
- progress totals and latest human-readable stage;
- cancellation request timestamp;
- bounded attempt count and next eligible time;
- lease owner and expiry for crash recovery;
- structured error code, message and diagnostic details;
- correlation and idempotency keys;
- creation, start, finish and update timestamps.

Lifecycle:

```text
queued -> running -> succeeded
   |         |  \-> retry_wait -> queued
   |         |  \-> failed
   |         \----> cancelled
   \--------------> cancelled
```

Only a lease owner may mutate running progress. Lease expiry returns interrupted work to
`queued` or `retry_wait` according to its policy. Cancellation is cooperative between
safe checkpoints; it never interrupts a database/filesystem commit halfway through.
Retries preserve previous attempts and diagnostics.

## Job kinds and idempotency

Initial registered kinds are `import`, `thumbnail`, `face_analysis`, `animal_analysis`,
`geocode`, `dedup_scan`, `derived_maintenance` and Apple synchronization operations. Payloads
include a version. Unknown kinds or versions fail visibly rather than being discarded.

Idempotency is kind-specific:

- import: source path plus explicit import operation ID;
- thumbnail/derived media: photo/variant ID plus generation key;
- analysis: photo ID plus model and display revision;
- geocode: normalized coordinate plus provider revision;
- dedup: scan scope plus catalog revision;
- provider sync: provider identity plus checkpoint or package identity.

## Worker and shutdown contract

- Concurrency is bounded globally and optionally per resource class.
- Database-heavy, network-heavy and model-heavy work can have separate limits.
- Workers renew leases while making progress and check cancellation between safe units.
- Shutdown stops leasing new work, requests cooperative cancellation only when required,
  waits for bounded graceful completion and leaves recoverable leases in the database.
- Startup first recovers expired leases, reconciles incomplete filesystem intents and
  then accepts new work.

## Observability contract

The task API and macOS status client receive the same service DTOs. Every job exposes
status, progress, attempts, timestamps and a safe error. Structured logs carry job ID,
request ID, kind and library identity. Diagnostic export redacts paths and external IDs
by default and never includes media bytes.

## Versioned local-service API

The macOS shell and future local clients use `/api/v1`. `GET /api/v1/service` advertises
the service build, minimum compatible API and capability names. Health, deep diagnostics,
task listing, task detail, retry and cancellation are available under `/api/v1`; the
unversioned routes remain compatibility aliases for the current Web UI. Version 1 is
local-trusted only and does not imply remote authentication.
