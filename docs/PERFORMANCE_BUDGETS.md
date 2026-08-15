# Performance budgets

These budgets protect the Phase 0–3 browsing experience from regressions as a
personal library grows. Run them with `cargo test --test timeline_performance`
and, after `npm run build` in `web/`, with `npm run perf`.

| Area | Budget | Enforcement |
| --- | --- | --- |
| Catalog query | A warm first-page query over 100,000 photos completes in under 1 second | Rust integration test |
| API page | Initial and subsequent cursor pages contain 120 photos by default | Rust and TypeScript tests |
| Browser grouping | Grouping 100,000 timeline items completes in under 1 second | Vitest performance fixture |
| JavaScript | `frontend/assets/app.js` is at most 150 KiB uncompressed | Build budget script |
| CSS | `frontend/assets/app.css` is at most 50 KiB uncompressed | Build budget script |

The timeline groups records by date, so structural groups grow with the number
of represented dates rather than the number of photos. Date groups use viewport
virtualization: offscreen groups keep only their measured placeholder and remove
their photo image DOM. Cursor pagination prevents the initial request and render
from loading the complete catalog.

Timing budgets intentionally include enough headroom for CI and development
machines. A failure should be investigated with a release build and a warm
database before changing a threshold.
