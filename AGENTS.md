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
