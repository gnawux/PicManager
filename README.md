# PicManager

A family photo management tool built in Rust. Automatically organizes photos, detects duplicates, groups them into albums by time, camera and location, detects faces locally, and provides both a Web UI and a CLI.

中文文档：[README.zh.md](README.zh.md)

## Features

| Feature | Status |
|---------|--------|
| Import photos from a directory | ✓ |
| EXIF metadata extraction (time, camera, GPS) | ✓ |
| Exact duplicate detection (SHA-256) | ✓ |
| Perceptual duplicate detection (two-layer: Gradient pHash + DCT pHash) | ✓ |
| Dedup confirmation workflow (keep / soft-delete) | ✓ |
| Import state tracking (skip already-imported) | ✓ |
| Format detection (JPEG / PNG / GIF / WebP / HEIC / ARW) | ✓ |
| Auto album grouping by month, camera, and GPS location | ✓ |
| Manual album merge | ✓ |
| Web UI (photo grid, album nav, import panel, dedup modal) | ✓ |
| REST API | ✓ |
| Config file (`~/Library/Application Support/picmanager/config.toml`) | ✓ |
| Local face detection on import (ultraface-slim-320 ONNX) | ✓ |
| Face embedding extraction (ArcFace MobileNetV1, 512-D L2-normalised) | ✓ |
| Batch face re-analysis (all library or specific photos) | ✓ |
| ONNX model download via CLI (`models fetch`) | ✓ |
| People view with DBSCAN auto-clustering (ArcFace embeddings) | ✓ |
| Hierarchical person tree (parent_id, unlimited depth) | ✓ |
| Animal detection on import (YOLOv8-nano ONNX, 10 COCO species) | ✓ |
| Animal species browser with bounding-box overlay | ✓ |
| Geographic hierarchy view (country → state → city drill-down) | ✓ |
| Map view with GPS markers (Leaflet.js + markercluster) | ✓ |
| Photo time/timezone editing (DB-only, no EXIF write-back) | ✓ |
| Fill missing metadata button (face re-analysis + geo re-coding for uncovered photos) | ✓ |
| CLI `fill-missing` command with per-minute progress and final summary | ✓ |
| Person status management (active / ignored / not-a-person) | ✓ |
| Inline person name editing with blur-to-save | ✓ |
| "…" context menu per person card (ignore / mark not-a-person) | ✓ |
| Multi-select people + batch bar (name+merge into tree, batch ignore/not-person) | ✓ |
| Client-side undo stack for all people edits | ✓ |
| Person detail tree editing (set/change parent, sub-person panel) | ✓ |
| Duplicate name detection with face thumbnails on rename | ✓ |

## Requirements

- Rust 1.95+
- macOS (primary platform; other platforms planned)
- [libheif](https://github.com/strukturag/libheif) — for HEIC / Apple Live Photo support
- [ONNX Runtime](https://github.com/microsoft/onnxruntime) — for face detection and embedding (optional; face features are silently skipped when not present)

```bash
brew install libheif
# Optional — for face detection:
brew install onnxruntime
picmanager models fetch   # downloads face_detector.onnx + arcface_mobilenetv1.onnx + yolov8n.onnx
```

## Build

```bash
cargo build --release
```

The binary is placed at `target/release/picmanager`.

## Usage

### Import photos

Scan a directory and import all supported photos into the library:

```bash
picmanager import ~/Pictures/exported-from-phone/
```

```
Importing from /Users/alice/Pictures/exported-from-phone/ ...
Done: 128 total, 120 imported, 8 skipped, 0 errors
```

- Source files are **never modified or deleted** — clean them up manually after verifying the import.
- Files with the same SHA-256 are skipped on re-import.
- After import, photos are automatically grouped into monthly and per-camera albums.

Supported formats: JPEG, PNG, GIF, WebP, HEIC (incl. Apple Live Photo), ARW (Sony RAW)

### Find and confirm duplicates

```bash
picmanager dedup
```

Scans all imported photos for visual similarity using a two-layer algorithm:
1. **Layer 1** — Gradient pHash, Hamming distance ≤ 10 (≤ 8 for photos taken more than 60 s apart)
2. **Layer 2** — DCT pHash verification, Hamming distance ≤ 8, eliminates false positives such as screenshots matched against natural photos

The rest are soft-deleted (marked `deleted` in the database — no files are removed from disk).

After running `dedup`, open the Web UI and click the 🔍 button in the **维护操作** row to review and confirm each duplicate group.

### Start the Web server

```bash
picmanager serve
```

Opens `http://127.0.0.1:8080` — a photo grid with album sidebar, import panel, and dedup modal.

### Show active configuration

```bash
picmanager config
```

Prints all settings and the config file path.

### Face detection and embedding

Download the ONNX model files once:

```bash
picmanager models fetch
```

This downloads `face_detector.onnx` (~1 MB), `arcface_mobilenetv1.onnx` (~10 MB), and `yolov8n.onnx` (~6 MB) to `~/Library/Application Support/picmanager/models/`. After that, face detection and animal detection run automatically on every imported photo.

**Embed models into the binary (optional)**

To build a fully self-contained binary that does not require model files on disk at runtime:

```bash
picmanager models fetch                   # download to config dir (once)
picmanager models bundle                  # copy to ./models/ in the project root
cargo build --release                     # rebuild — models are now compiled in
```

After the rebuild, the binary works without any files under `~/Library/Application Support/picmanager/models/`. If the binary was built without embedded models, it falls back to the on-disk path automatically.

To re-analyse the entire library (e.g. after downloading models for the first time):

```bash
picmanager faces analyze
```

To re-analyse specific photos:

```bash
picmanager faces analyze --photo-ids 1,2,3
```

Face data is stored locally in the SQLite database; no cloud service is used.

### Fill missing metadata

After downloading models, run one command to backfill both face analysis and reverse-geocoding for any photos that were imported before the models were available:

```bash
picmanager fill-missing            # fill both faces and geo
picmanager fill-missing --faces    # only photos never analysed for faces
picmanager fill-missing --geo      # only photos with GPS but no cached location
```

Progress is printed every minute; a summary is shown at the end:

```
开始补全缺失元数据…
  待补充人脸分析：75 张
  待补充地理编码：23 张

[00:01:00] 人脸：12/75 (16%) ｜ 地理：3/23 (13%)
[00:03:45] 人脸：75/75 (100%) ｜ 地理：20/23 (87%)

补全完成（耗时 3 分 45 秒）：
  人脸：分析了 75 张照片，库中共 203 个人脸记录
  地理：编码了 20 个新位置，3 张无城市信息（已跳过），共 23 张待处理
```

## PhotoBridge — iCloud Photos Import

PhotoBridge is a companion macOS CLI that exports photos from your iCloud / Photos library
into a staging directory, from which PicManager can import them.

### Prerequisites

- macOS 26 (Tahoe) or later
- If your Photos Library lives on the system volume, consider migrating it to an external drive
  first — iCloud downloads go to the same volume as the library, and a full-library export of a
  large iCloud library will fill the system volume.

### Build

```bash
cd photobridge
swift build -c release
# Sign the binary so macOS will show the Photos consent dialog:
codesign --force --sign - \
  --entitlements Sources/PhotoBridge/PhotoBridge.entitlements \
  .build/release/photobridge
# binary: photobridge/.build/release/photobridge
```

> **Photos permission note:** macOS TCC attributes the Photos permission to the terminal app
> that runs photobridge (iTerm2, Terminal.app, etc.), not to the binary itself. When you first
> run photobridge, your terminal app will show a "wants to access your Photos" consent dialog.
> After granting, no restart is needed — re-run the command immediately.
>
> The codesign step is required each time you rebuild. Without it, `requestAuthorization`
> returns `denied` immediately without showing a dialog.
>
> If the dialog does not appear (previously dismissed or denied), reset the terminal app's
> Photos permission and run again:
> ```bash
> tccutil reset Photos com.googlecode.iterm2  # iTerm2
> tccutil reset Photos com.apple.Terminal     # Terminal.app
> tccutil reset Photos com.microsoft.VSCode   # VS Code
> ```

### First-time setup

```bash
photobridge setup                      # print step-by-step setup guide
```

### Usage

**One-shot export** — exports your entire Photos library, then auto-imports into PicManager
if `picmanager` is found in `PATH` (or via `--picmanager`):

```bash
photobridge export --dry-run           # count assets without exporting
photobridge export                     # export + auto-import if picmanager in PATH
photobridge export --output /Volumes/NAS/staging --picmanager /usr/local/bin/picmanager

# If picmanager is not in PATH, import manually after export:
picmanager import /path/to/staging/
```

**Incremental sync** — exports only photos added or changed since the last sync:

```bash
photobridge sync                       # export new assets + auto-import
photobridge sync --dry-run             # show how many new assets would be exported
photobridge status                     # show last sync date and count
```

**Repair timestamps** on already-exported files (for files exported before auto-timestamp
was added):

```bash
photobridge fix-timestamps /path/to/staging/        # apply Photos creation dates to mtimes
photobridge fix-timestamps --dry-run /path/to/staging/
```

**Automatic sync via launchd:**

```bash
photobridge setup --install-launchd --interval-hours 6
# then activate:
launchctl load ~/Library/LaunchAgents/com.picmanager.photobridge-sync.plist
```

Options shared by `export` and `sync`:

| Option | Default | Description |
|--------|---------|-------------|
| `--output <dir>` | `~/Library/Application Support/PhotoBridge/staging` | Staging directory |
| `--picmanager <path>` | (auto-detect from PATH) | Path to picmanager executable |
| `--batch-size <n>` | 200 | Photos per PicManager import batch |
| `--max-concurrent <n>` | 4 | Max concurrent iCloud downloads |
| `--dry-run` | — | Count assets only, do not export |

## Configuration

Create `~/Library/Application Support/picmanager/config.toml` to override any default:

```toml
library_path = "/Volumes/NAS/Photos/PicManager"
host         = "0.0.0.0"
port         = 9090
thumb_size   = 400
```

Command-line flags (when added) take precedence over the config file, which takes precedence over built-in defaults.

## REST API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/photos` | Paginated photo list |
| GET | `/api/photos/gps-points` | GPS coordinates of all geotagged photos |
| POST | `/api/photos/batch-update` | Batch-update time / timezone on multiple photos |
| GET | `/api/photos/:id` | Single photo detail |
| PATCH | `/api/photos/:id` | Edit taken_at / timezone_offset (DB only) |
| GET | `/api/photos/:id/thumb` | 300 px JPEG thumbnail |
| POST | `/api/import` | Trigger a background import |
| GET | `/api/import/status` | Poll import progress |
| GET | `/api/dedup` | List pending duplicate groups |
| POST | `/api/dedup/:group_id/resolve` | Confirm which photos to keep |
| GET | `/api/albums` | List all albums with photo counts |
| GET | `/api/albums/:id/photos` | Paginated photos in an album |
| POST | `/api/albums/merge` | Merge one album into another |
| GET | `/api/photos/:id/faces` | Face regions detected in a photo |
| POST | `/api/faces/analyze` | Trigger face re-analysis (all, given photo IDs, or `missing_only`) |
| GET | `/api/faces/jobs/:id` | Poll face job progress |
| GET | `/api/faces/:id/thumb` | Cropped face thumbnail |
| GET | `/api/geo/hierarchy` | Nested country → state → city hierarchy with photo counts |
| POST | `/api/geo/regeocode` | Trigger background reverse-geocoding for photos with GPS but no cached location |
| GET | `/api/geo/regeocode/status` | Poll whether the geocoding background task is still running |
| GET | `/api/people` | List people (default: active only; `?status=all|ignored|not_a_person`, `?name_exact=`) |
| GET | `/api/people/tree` | Nested person tree |
| POST | `/api/people/cluster` | Trigger DBSCAN re-clustering |
| POST | `/api/people/merge` | Merge two person records |
| PATCH | `/api/people/:id` | Update person name and/or status |
| POST | `/api/people/batch-update` | Batch-update status on multiple people |
| GET | `/api/people/:id` | Photos belonging to a person |
| POST | `/api/people/:id/reparent` | Change a person's parent in the tree |
| GET | `/api/animals/species` | List detected animal species with photo counts |
| GET | `/api/animals/:species/photos` | Photos containing a given species |
| GET | `/api/photos/:id/animals` | Animal detections in a photo |

**Example — trigger import:**

```bash
curl -X POST http://localhost:8080/api/import \
  -H 'Content-Type: application/json' \
  -d '{"dir": "/path/to/photos"}'
```

**Example — confirm dedup group 3, keep photo 7:**

```bash
curl -X POST http://localhost:8080/api/dedup/3/resolve \
  -H 'Content-Type: application/json' \
  -d '{"keep": [7]}'
```

**Example — merge album 2 into album 1:**

```bash
curl -X POST http://localhost:8080/api/albums/merge \
  -H 'Content-Type: application/json' \
  -d '{"source": 2, "target": 1}'
```

## Data storage

Metadata is stored in SQLite at:

```
~/Pictures/PicManager/picmanager.db
```

Original photo files are **never modified**. The database stores only metadata and status.

## Development

```bash
cargo nextest run            # run all 202 tests (5 more need ONNX model files, marked #[ignore])
cargo clippy -- -D warnings  # lint
cargo watch -x build         # rebuild on file changes
```

## Project layout

```
src/
  main.rs        CLI entry point (import / dedup / faces / models / serve / config)
  config.rs      Config struct with TOML file loading
  error.rs       Unified AppError type (incl. ModelNotFound)
  importer/      Directory scanner, SHA-256, import pipeline
  metadata/      Format detection (magic bytes), EXIF/GPS extraction
  dedup/         Perceptual hash, candidate scan, resolve workflow
  album/         Auto-grouping by month, camera & GPS location; manual merge
  face/          Local face detection (ultraface), embedding (ArcFace), DBSCAN clustering, batch jobs
  animal/        Animal detection on import (YOLOv8-nano ONNX, 10 COCO species)
  storage/       SQLite connection pool, migrations
  web/           Axum server, REST handlers, static file serving
frontend/        Static HTML + CSS + JS (no build step)
migrations/      SQLx migration files (0001–0010)
tests/           Integration tests + real-camera sample images
docs/            Architecture design and development plan
```
