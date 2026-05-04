# PicManager — User Manual

Full reference for `picmanager` CLI, `photobridge` CLI, REST API, and configuration.

---

## Table of contents

1. [picmanager CLI](#picmanager-cli)
2. [Configuration](#configuration)
3. [REST API](#rest-api)
4. [PhotoBridge](#photobridge)
5. [Data storage](#data-storage)
6. [Development](#development)

---

## picmanager CLI

### `import` — import photos into the library

```bash
picmanager import <dir>
picmanager import --copy <dir>          # copy instead of move (keep source files)
picmanager import --batch-size 200 <dir>
picmanager import --log import.ndjson <dir>
picmanager import --dry-run <dir>       # count files, do not import
```

**Date inference chain (in order):**

| Priority | Source | Notes |
|----------|--------|-------|
| 1 | EXIF (DateTimeOriginal → DateTimeDigitized → GPS DateStamp → DateTime) | Most reliable |
| 2 | File mtime | Set by PhotoBridge from `PHAsset.creationDate` |
| 3 | Filename pattern | Unix timestamp (10/13 digits), YYYYMMDD_HHMMSS, YYYY-MM-DD |
| 4 | None | Placed in `library/unknown/` |

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--copy` | — | Copy files; do not move them |
| `--batch-size <n>` | (all) | Stop after importing N files |
| `--log <path>` | — | Write NDJSON import log (one line per file) |
| `--dry-run` | — | Scan and report counts without modifying anything |

**NDJSON log format** (`--log`):

```json
{"path":"/staging/file.heic","status":"imported","sha256":"abc...","error":null,"ts":"2026-01-01T00:00:00Z"}
{"path":"/staging/dup.jpg","status":"skipped","sha256":"abc...","error":null,"ts":"..."}
{"path":"/staging/bad.jpg","status":"failed","sha256":null,"error":"unsupported format","ts":"..."}
```

---

### `dedup` — find visual duplicates

```bash
picmanager dedup            # incremental scan (new photos only)
picmanager dedup --full     # full rescan of entire library
```

Uses a two-layer algorithm:

1. **Layer 1** — Gradient pHash, Hamming ≤ 10 (≤ 8 for photos taken > 60 s apart)
2. **Layer 2** — DCT pHash verification, Hamming ≤ 8 — eliminates false positives

Duplicate groups are stored in the database as *pending*. Review and confirm in the Web UI (🔍 button) or via the REST API. No files are deleted automatically — soft-delete only.

---

### `serve` — start the Web server

```bash
picmanager serve
picmanager serve --host 0.0.0.0 --port 9090
```

Opens at `http://127.0.0.1:8080` by default. Includes photo grid, album sidebar, people manager, map view, dedup review, and curated collections.

---

### `faces` — face analysis

```bash
picmanager faces analyze                      # re-analyse entire library
picmanager faces analyze --photo-ids 1,2,3    # specific photos
picmanager faces analyze --rotated-only       # photos with non-zero rotation/flip
```

---

### `fill-missing` — backfill metadata

```bash
picmanager fill-missing            # both faces and geo-coding
picmanager fill-missing --faces    # only photos never analysed for faces
picmanager fill-missing --geo      # only photos with GPS but no cached city name
```

Progress is printed every minute:

```
开始补全缺失元数据…
  待补充人脸分析：75 张
  待补充地理编码：23 张

[00:01:00] 人脸：12/75 (16%) ｜ 地理：3/23 (13%)
[00:03:45] 人脸：75/75 (100%) ｜ 地理：20/23 (87%)

补全完成（耗时 3 分 45 秒）
```

---

### `models` — manage ONNX model files

```bash
picmanager models fetch     # download face_detector.onnx (~1 MB),
                            # arcface_mobilenetv1.onnx (~10 MB),
                            # yolov8n.onnx (~6 MB)
picmanager models bundle    # copy models to ./models/ and embed in next build
```

Models are stored at `~/Library/Application Support/picmanager/models/`. Face and animal detection run automatically on every imported photo once models are present.

---

### `config` — show active configuration

```bash
picmanager config
```

---

## Configuration

Create `~/Library/Application Support/picmanager/config.toml`:

```toml
library_path = "/Volumes/NAS/Photos/PicManager"
host         = "0.0.0.0"
port         = 9090
thumb_size   = 400
```

CLI flags take precedence over the config file, which takes precedence over built-in defaults.

---

## REST API

### Photos

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/photos` | Paginated photo list (`?page=`, `?per_page=`, `?album_id=`, `?in_collection=`) |
| GET | `/api/photos/gps-points` | Coordinates of all geotagged photos |
| POST | `/api/photos/batch-update` | Batch-update time / timezone / rotation |
| GET | `/api/photos/:id` | Single photo detail (EXIF, GPS, faces, animals) |
| PATCH | `/api/photos/:id` | Edit taken_at / timezone_offset / rotation / flip |
| GET | `/api/photos/:id/thumb` | 300 px JPEG thumbnail |
| GET | `/api/photos/:id/file` | Original file bytes (Content-Type inferred from format) |
| GET | `/api/photos/:id/faces` | Face regions detected in a photo |
| GET | `/api/photos/:id/animals` | Animal detections in a photo |

### Import

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/import` | Trigger background import (`{"dir": "..."}`) |
| GET | `/api/import/status` | Poll import progress |

### Dedup

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/dedup` | List pending duplicate groups (with filename, width, height, date) |
| POST | `/api/dedup/:group_id/resolve` | Confirm which photos to keep (`{"keep": [id, ...]}`) |

### Albums

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/albums` | All albums with photo counts and `latest_photo_at` |
| GET | `/api/albums/:id/photos` | Paginated photos in an album |
| POST | `/api/albums/merge` | Merge source album into target (`{"source": 2, "target": 1}`) |

### Collections (curated)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/collections` | List all curated collections |
| POST | `/api/collections` | Create collection (`{"name": "..."}`) |
| PATCH | `/api/collections/:id` | Rename collection |
| DELETE | `/api/collections/:id` | Delete collection |
| GET | `/api/collections/:id/photos` | Photos in a collection |
| POST | `/api/collections/:id/photos` | Add photos (`{"photo_ids": [...]}`) |
| DELETE | `/api/collections/:id/photos` | Remove photos (`{"photo_ids": [...]}`) |

### Faces & People

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/faces/analyze` | Trigger face re-analysis (body: `{}` all, `{"photo_ids": [...]}`, `{"missing_only": true}`) |
| GET | `/api/faces/jobs/:id` | Poll face job progress |
| GET | `/api/faces/:id/thumb` | Cropped face thumbnail |
| GET | `/api/people` | List people (`?status=active\|ignored\|not_a_person\|all`, `?name_exact=`) |
| GET | `/api/people/tree` | Nested person tree with `cover_face_id` |
| POST | `/api/people/cluster` | Trigger full DBSCAN re-clustering |
| POST | `/api/people/cluster/incremental` | Non-destructive incremental clustering |
| POST | `/api/people/merge` | Merge two people (`{"source_id": x, "target_id": y}`) |
| PATCH | `/api/people/:id` | Update person name and/or status |
| POST | `/api/people/batch-update` | Batch-update status on multiple people |
| GET | `/api/people/:id` | Photos belonging to a person (recursive subtree) |
| POST | `/api/people/:id/reparent` | Change person's parent in tree |
| GET | `/api/people/:id/merge-suggestions` | Top merge candidates by centroid cosine distance |
| GET | `/api/people/:id/outlier-faces` | Faces farthest from centroid (possible misassignments) |
| POST | `/api/people/:id/eject-face` | Remove a face from this person |
| GET | `/api/people/:id/centroid-faces` | Faces used for centroid + distance distribution stats |

### Geography

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/geo/hierarchy` | Nested country → state → city with photo counts |
| POST | `/api/geo/regeocode` | Trigger background reverse-geocoding for un-geocoded photos |
| GET | `/api/geo/regeocode/status` | Poll geocoding task status |

### Animals

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/animals/species` | Species list with photo counts |
| GET | `/api/animals/:species/photos` | Photos containing a species |
| GET | `/api/photos/:id/animals` | Animal detections in a photo |

---

## PhotoBridge

PhotoBridge is a companion Swift CLI that exports photos from your iCloud / Photos.app library into a staging directory. If `picmanager` is found in `PATH`, it calls `picmanager import` automatically after each export.

### Prerequisites

- macOS 13+ (incremental sync via PHPersistentChangeToken requires macOS 16+)
- Xcode command-line tools (`xcode-select --install`)

### Build

```bash
cd photobridge
swift build -c release
codesign --force --sign - \
  --entitlements Sources/PhotoBridge/PhotoBridge.entitlements \
  .build/release/photobridge
```

> **Photos permission:** macOS TCC attributes the consent dialog to the terminal app (iTerm2, Terminal.app…), not to the binary. The dialog reads "iTerm2 wants to access your Photos". After granting, no restart needed — re-run the command immediately.
>
> If the dialog does not appear (previously denied), reset the terminal app's permission:
> ```bash
> tccutil reset Photos com.googlecode.iterm2   # iTerm2
> tccutil reset Photos com.apple.Terminal      # Terminal.app
> tccutil reset Photos com.microsoft.VSCode    # VS Code
> ```
>
> The `codesign` step is required after every rebuild; without it `requestAuthorization` returns `denied` without showing a dialog.

### First-time setup

```bash
photobridge setup                          # print step-by-step guide
photobridge setup --install-launchd        # write launchd plist for automatic sync
photobridge setup --install-launchd --interval-hours 3
# activate:
launchctl load ~/Library/LaunchAgents/com.picmanager.photobridge-sync.plist
```

### Commands

**`export`** — full export of entire Photos library:

```bash
photobridge export --dry-run                    # count only
photobridge export                              # export + auto-import (if picmanager in PATH)
photobridge export --output /Volumes/NAS/staging \
                   --picmanager /usr/local/bin/picmanager
```

**`sync`** — incremental export (new assets since last sync):

```bash
photobridge sync
photobridge sync --dry-run
photobridge status                              # last sync date and total count
```

**`fix-timestamps`** — repair mtime/ctime on already-exported files:

```bash
photobridge fix-timestamps /path/to/staging/
photobridge fix-timestamps --dry-run /path/to/staging/
```

Use this once on files exported before auto-timestamp was added.

**Options shared by `export` and `sync`:**

| Option | Default | Description |
|--------|---------|-------------|
| `--output <dir>` | `~/Library/Application Support/PhotoBridge/staging` | Staging directory |
| `--picmanager <path>` | (auto-detect from PATH) | Path to picmanager executable |
| `--batch-size <n>` | 200 | Photos per picmanager import batch |
| `--max-concurrent <n>` | 4 | Concurrent iCloud downloads |
| `--dry-run` | — | Count only, do not export |

### How timestamps work

When PhotoBridge writes a file, it calls `FileManager.setAttributes([.modificationDate:, .creationDate:])` with `PHAsset.creationDate`. picmanager then reads the file mtime as a fallback when EXIF has no date — so screenshots and WhatsApp images without EXIF land in the correct dated directory instead of `library/unknown/`.

---

## Data storage

```
~/Pictures/PicManager/
  picmanager.db            SQLite database
  YYYY-MM-DD/              Photos organised by date
  unknown/                 Photos where no date could be inferred
  .thumbs/                 Thumbnail cache (auto-generated, safe to delete)
```

Original photo files are **never modified**. The database stores metadata and status only. Soft-deleted photos (from dedup) are marked `import_status='deleted'` in the database; no files are removed from disk.

---

## Development

```bash
cargo nextest run                   # run all tests (~315 passing)
cargo clippy                        # lint
python3 tests/make_fixtures.py      # regenerate test fixtures

# PhotoBridge tests
cd photobridge
.build/debug/PhotoBridgeTestRunner  # 53 tests
```
