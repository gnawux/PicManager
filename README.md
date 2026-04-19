# PicManager

A family photo management tool built in Rust. Automatically organizes photos, detects duplicates, groups them into albums by time and camera, and provides both a Web UI and a CLI.

中文文档：[README.zh.md](README.zh.md)

## Features

| Feature | Status |
|---------|--------|
| Import photos from a directory | ✓ |
| EXIF metadata extraction (time, camera, GPS) | ✓ |
| Exact duplicate detection (SHA-256) | ✓ |
| Perceptual duplicate detection (dHash) | ✓ |
| Dedup confirmation workflow (keep / soft-delete) | ✓ |
| Import state tracking (skip already-imported) | ✓ |
| Format detection (JPEG / PNG / GIF / WebP / HEIC / ARW) | ✓ |
| Auto album grouping by month, camera, and GPS location | ✓ |
| Manual album merge | ✓ |
| Web UI (photo grid, album nav, import panel, dedup modal) | ✓ |
| REST API | ✓ |
| Config file (`~/Library/Application Support/picmanager/config.toml`) | ✓ |

## Requirements

- Rust 1.95+
- macOS (primary platform; other platforms planned)
- [libheif](https://github.com/strukturag/libheif) — for HEIC / Apple Live Photo support

```bash
brew install libheif
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

Scans all imported photos for visual similarity (perceptual hash, Hamming distance ≤ 10), then presents each duplicate group interactively. Enter the photo IDs to keep; the rest are soft-deleted (marked `deleted` in the database — no files are removed from disk).

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
| GET | `/api/photos/:id/thumb` | 300 px JPEG thumbnail |
| POST | `/api/import` | Trigger a background import |
| GET | `/api/import/status` | Poll import progress |
| GET | `/api/dedup` | List pending duplicate groups |
| POST | `/api/dedup/:group_id/resolve` | Confirm which photos to keep |
| GET | `/api/albums` | List all albums with photo counts |
| GET | `/api/albums/:id/photos` | Paginated photos in an album |
| POST | `/api/albums/merge` | Merge one album into another |

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
cargo nextest run            # run all 76 tests
cargo clippy -- -D warnings  # lint
cargo watch -x build         # rebuild on file changes
```

## Project layout

```
src/
  main.rs        CLI entry point (import / dedup / serve / config)
  config.rs      Config struct with TOML file loading
  error.rs       Unified AppError type
  importer/      Directory scanner, SHA-256, import pipeline
  metadata/      Format detection (magic bytes), EXIF/GPS extraction
  dedup/         Perceptual hash, candidate scan, resolve workflow
  album/         Auto-grouping by month, camera & GPS location; manual merge
  storage/       SQLite connection pool, migrations
  web/           Axum server, REST handlers, static file serving
frontend/        Static HTML + CSS + JS (no build step)
migrations/      SQLx migration files (0001 schema, 0002 geocache)
tests/           Integration tests
docs/            Architecture design and development plan
```
