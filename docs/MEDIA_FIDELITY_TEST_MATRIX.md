# Media fidelity regression matrix

This matrix is the release gate for display-orientation and Apple Photos representation
changes. Automated fixtures are synthetic or checked-in samples and never write to the
user's Photos library.

| Case | Expected policy | Automated evidence |
| --- | --- | --- |
| EXIF 1–8, including mirrored 2/4/5/7 | Apply source orientation exactly once | Exact marker-pixel signatures in `tests/media_fidelity.rs` |
| Legacy HEIC/IROT fallback | Read orientation from the original HEIC; never from converted output | Real HEIC repeatability and byte-preservation test on macOS |
| Photos-adjusted asset | Immutable original is master; current rendition is display | Rendition package selection and generation-revision tests |
| Screenshot/PNG | Preserve name, dimensions and source identity | Apple inventory policy fixture |
| Duplicate filename | Keep independent source identities | Two `IMG_0001.JPG` fixture records remain distinct |
| Live Photo | Keep still image visible and record paired video metadata | Multi-resource inventory assertion |
| RAW + JPEG | Queue the finished JPEG and retain RAW companion metadata | Multi-resource inventory assertion |
| RAW-only | Keep visible as policy-excluded; never silently drop or delete | Exclusion and no-export-work assertion |
| User rotation/flip | Create a new render revision and never reuse stale derived output | Revisioned thumbnail and face invalidation tests |

## Manual Photos comparison

Before a release that changes PhotoKit export code, use a copied test library containing
the same categories. Export with `photobridge export-asset`, open the PicManager current
rendition beside Photos, and compare rotation, mirroring, crop and common adjustments at
fit-to-window and 100% zoom. Verify the original SHA-256 before and after the run, then
repeat the import and confirm that neither the display revision nor derived output changes.

Color is compared in the same macOS display profile. A visible mismatch, original hash
change, second-import revision increment, missing RAW-only row, or collapsed duplicate
filename fails the gate.
