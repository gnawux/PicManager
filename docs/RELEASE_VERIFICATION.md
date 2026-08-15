# Phase 4/5 release verification

Run this checklist from a clean worktree with no process pointed at a personal library.
Use a temporary or copied library for browser checks.

## Automated gates

```sh
cargo test --quiet
cargo test --quiet --test migration_rehearsal
cargo test --quiet --test timeline_performance
(cd photobridge && swift run PhotoBridgeTestRunner)
(cd web && npm run check && npm test && npm run build && npm run perf)
scripts/build-macos-app.sh
PICMANAGER_CODESIGN_IDENTITY=- scripts/sign-macos-app.sh dist/PicManager.app
scripts/test-macos-install-lifecycle.sh dist/PicManager.app
git diff --check
```

Expected release gates:

- all Rust unit and integration tests pass (the intentionally ignored test remains
  documented by the test runner);
- the copied schema-0018 migration rehearsal preserves all legacy records and verifies
  healthy after an idempotent backfill;
- the 100,000-photo warm timeline query and frontend grouping each stay under one
  second;
- Svelte reports zero errors and warnings and all Vitest files pass;
- uncompressed production JavaScript is at most 150 KiB and CSS at most 50 KiB;
- PhotoBridge's 87-test executable suite passes. The package intentionally uses the
  `PhotoBridgeTestRunner` target rather than an XCTest target, so `swift test` alone
  reports that no tests were found.

## Isolated browser smoke test

Start the release server against an explicit temporary library path and non-default
port. Never use the default personal library for this check.

- Open the photo timeline and confirm cursor loading, density control and selection.
- Open the viewer; verify keyboard navigation, metadata drawer and original/current
  controls. Escape must close it and restore focus to the originating photo.
- Confirm albums/collections, people, places and activities load their empty or seeded
  states without console errors.
- Confirm Apple Photos exposes unsynced/failed/excluded/missing filters and original
  filenames, and retry actions reach the task center.
- Confirm duplicate resolution requires two steps and states that original files are
  not immediately deleted.
- Confirm `/legacy/` remains available.
- Test a narrow viewport, keyboard-only navigation, visible focus, the skip link and
  reduced-motion behavior.

Record the exact commit, platform, test totals, bundle sizes and any expected external
telemetry warnings in the release notes.

The latest recorded result is [PHASE45_RELEASE_GATE.md](PHASE45_RELEASE_GATE.md).
Production Developer ID signing, notarization and interactive PhotoKit checks remain
explicit release-operator gates and must not be represented by the local ad-hoc check.
