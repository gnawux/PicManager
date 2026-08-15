# Phase 4/5 release gate

Validated on 2026-08-15 from commit `3fcb327` on macOS arm64 using isolated temporary
libraries. No personal catalog or media was used.

## Automated results

- Rust all-target suite: 497 passed, 1 intentionally ignored, 0 failed.
  - Library unit tests: 370 passed, 1 ignored.
  - Web API integration: 114 passed.
  - Database reliability, media fidelity, migration rehearsal, service compatibility,
    sync restart, and timeline performance: 13 passed.
- Swift: 87 passed, 0 failed; `PhotoBridge` and `PicManagerMac` targets built.
- Frontend: 14 files / 32 tests passed; Svelte check reported 0 errors and 0 warnings.
- Performance: 100,000-photo Rust timeline passed; frontend grouping passed.
- Production budgets: JavaScript 112.7 KiB / 150 KiB; CSS 32.3 KiB / 50 KiB.
- Release bundle: production Web, Rust, and Swift assembly passed; arm64 app bundle
  identity, executable layout, embedded service, and absence of user databases passed.
- Distribution preparation: strict ad-hoc signature verification passed; missing
  production signing/notary credentials were correctly rejected.
- Installation lifecycle: clean install, app-only upgrade, and preservation-safe
  uninstall passed with byte-identical configuration, catalog, and media markers.

## Browser results

The in-app browser exercised an isolated service on a non-default port:

- timeline empty state, density and selection controls loaded;
- Apple Photos displayed synchronized, unsynchronized, failed, excluded, and missing
  filters plus review state;
- task center and two-step duplicate review loaded;
- `/legacy/` remained available;
- console contained no warnings or errors;
- 390×844 responsive layout was visually inspected.

The first narrow run exposed the mobile navigation inside the filtered header containing
block. Commit `3fcb327` moved it to the viewport and added an opaque bottom surface; the
frontend gates and browser layout were rerun after the fix. Viewer metadata, original
rendition switching, focus trap, arrow navigation, and Escape close are covered by the
passing component workflow test.

## External release actions

The local release gate is complete. Public distribution still requires a real Developer
ID signature, Apple notarization/stapling, and the manual PhotoKit/first-run matrix in
`release/INSTALL_TEST_MATRIX.md`. No artifact or update feed was published.
