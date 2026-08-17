# Native Rust Garmin release gate

Validated on 2026-08-17 on macOS arm64 without a real Garmin account or personal media.

- Baseline: annotated tag `v1.0.1-garmin-python` at commit `670c32b`.
- Rollback App: `dist/PicManager-Python-Garmin.app`, 107 MB; strict deep signature verification passed.
- Native App: `dist/PicManager.app`, 46 MB; bundle, installation lifecycle and strict deep
  ad-hoc signature verification passed.
- Rust: 549 passed, 1 intentionally ignored, 0 failed.
- Web: 16 files / 57 tests passed; Svelte reported 0 errors and warnings; production and
  performance budgets passed.
- Swift: 103 passed, 0 failed.
- Offline Garmin coverage: credential login, MFA cookie session, DI client fallback, token
  persistence/restore/refresh, literal activity pagination, ORIGINAL ZIP/FIT validation,
  interrupted download resume, catalog import and post-import checkpoint acknowledgement.
- Bundle inspection proved that Python, `garminconnect`, `curl_cffi`, `garmin_sync.py` and
  Python dynamic-library links are absent from the native App.

The native App is about 61 MB (57%) smaller than the preserved Python rollback App.
Production Developer ID signing and notarization remain external release-operator steps.
