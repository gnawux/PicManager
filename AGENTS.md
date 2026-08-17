# PicManager agent instructions

These repository-wide instructions apply to every automated coding session.

## Product and data safety

- Preserve existing photos, photo IDs, metadata, relationships, source identities and
  files. Prefer additive migrations and compatibility layers.
- Never delete, rewrite or move original media, RAW files or a personal library unless
  the user explicitly requests that exact operation after a dry-run and backup.
- External provider IDs are identities, never filenames. Lower-quality or recompressed
  variants never replace a higher-quality master.
- Use isolated temporary libraries for tests and browser checks. Never point automated
  verification at the default or personal PicManager library.
- Preserve unrelated worktree changes. Do not use destructive Git commands.

## Milestone protocol

- Break Phase 4/5 and later work into small, independently testable milestones recorded
  in `docs/MODERNIZATION_PLAN.md`.
- Keep the repository buildable at the end of every milestone.
- Run targeted tests while implementing and the complete affected suites before each
  milestone commit. Review `git diff --check` and migration/data-safety implications.
- Commit every completed milestone separately with `git commit -s`.
- Use a one-line English Conventional Commit subject followed by a detailed English
  body that explains behavior, safety constraints and tests performed.
- Do not combine unrelated milestones in one commit, and do not mark a milestone
  complete while required tests or acceptance criteria remain.
- Do not run `cargo fmt --all`; legacy files contain unrelated formatting. Format only
  files changed by the milestone when safe.

## Cross-component contracts and naming

- Treat every Rust/API, TypeScript/Web and Swift/macOS boundary as one shared contract,
  not as three independently named implementations. The producing component owns the
  wire schema; consumers must follow it explicitly.
- Use `snake_case` on JSON and database wires (`item_id`, `external_id`), idiomatic
  names inside each language, and an explicit boundary mapping when language casing
  differs. For Swift acronyms, prefer wire DTO properties such as `itemId` and map them
  to domain properties such as `itemID`, or declare tested coding keys. Never assume an
  automatic snake-case converter preserves `ID`, `URL`, `UUID` or similar acronyms.
- Keep one vocabulary for each concept across components. A provider identity, source
  ID, asset ID, photo ID, filename, path, display label and task ID are distinct types
  and names; do not rename or reuse them according to local preference.
- Any contract change must update the producer, every consumer, literal wire fixtures
  and contract documentation in the same milestone. Do not defer consumer verification
  until manual cross-component testing.
- Test clients against literal serialized responses from the real producer, including
  nulls, legacy enum values, acronym-bearing keys and extra fields. Generated/synthetic
  fixtures that merely mirror the consumer model are insufficient.
- For side-effecting APIs such as claim, lease, commit, retry and cancel, tests must
  prove the complete boundary: the server performs the mutation, the client decodes the
  response, and the next operation starts. A successful server mutation alone is not
  acceptance evidence.
- Do not use `try?`, empty catches or best-effort logging on critical cross-component
  paths. Persist and surface errors so a claimed lease cannot look like active I/O when
  the client actually failed to decode or dispatch it.
- Before integrating components, run a minimal end-to-end contract smoke test for each
  new path. For Apple export this is `claim -> decode -> helper launch -> package ->
  commit`, using an isolated test library or controlled fake provider.

## Git integration

- Prefer fast-forward integration from development branches into `main` so the primary
  branch keeps the milestone commit history without an additional merge commit.
- Before integration, rebase or otherwise update the development branch onto the
  current `main` when this can be done safely, then use `git merge --ff-only`.
- Create a merge commit only when preserving non-linear history has a concrete benefit
  or fast-forward integration is unsafe or impractical; do not add merge commits merely
  to record that work originated on a development branch.
- Never rewrite shared or published history solely to make a fast-forward possible.

## Current sequencing

- Phase 4 and Phase 5 are implemented. Preserve their service contracts, durable-job,
  recovery, ownership, Mac shell and data-migration guarantees during stabilization.
- Phase 5 must reuse the Rust service and embedded web application. Do not create a
  second photo-management frontend in SwiftUI.
- Use `docs/README.md` as the current documentation index. Treat `PLAN.md` and
  `DESIGN.md` as historical records when they conflict with current documents.
- Run the full release gates before merging release work to `main`.
- Read `docs/POST_PHASE5_GOTCHAS.md` before refactoring startup, macOS presentation,
  embedded assets, background jobs, geographic data or high-volume browser views.
