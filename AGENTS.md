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

## Current sequencing

- Complete architectural Phase 4 and macOS product Phase 5 before spending time on
  minor UI defects. Record discovered defects for the post-refactor stabilization pass.
- Phase 4 must establish shared service/repository boundaries, durable background work,
  database concurrency hardening, backup/reconciliation and consistent API lifecycle
  rules before Phase 5 depends on those interfaces.
- Phase 5 must reuse the Rust service and embedded web application. Do not create a
  second photo-management frontend in SwiftUI.
- After Phase 4/5 implementation, reorganize and update architecture, operation,
  packaging, upgrade and release documentation, then run the full release gates.
