# PicManager

A family photo management tool. Import, de-duplicate, organise by time / location / camera, detect faces and animals, and browse everything through a Web UI — all running locally, no cloud required.

中文文档：[README.zh.md](README.zh.md)

---

## Executables

| Binary | Language | Purpose |
|--------|----------|---------|
| `picmanager` | Rust | Main tool — import, dedup, serve Web UI, CLI |
| `photobridge` | Swift | iCloud Photos companion — export from Photos.app and feed into picmanager |

---

## Core features

- **Import** — scan a directory, move photos into a dated library (`library/YYYY-MM-DD/`), skip duplicates by SHA-256
- **Smart date inference** — EXIF → file mtime → filename pattern → `unknown/`
- **Two-layer dedup** — Gradient pHash (fast filter) + DCT pHash (precision verify) + Union-Find grouping
- **Auto albums** — by month, by camera model, by GPS city (reverse-geocoded via OSM Nominatim)
- **Face detection & clustering** — ultraface-slim-320 + ArcFace 512-D embeddings + DBSCAN people grouping; all local, no API key
- **Animal detection** — YOLOv8-nano, 10 COCO species, runs on import
- **Web UI** — photo grid, album sidebar, people manager, map view, dedup review, curated collections
- **PhotoBridge** — incremental iCloud sync via `PHPersistentChangeToken`; auto-sets file timestamps from `PHAsset.creationDate`; auto-corrects HEIC EXIF orientation to match Photos.app display (requires `exiftool`)

---

## Requirements

- **picmanager**: Rust 1.95+, macOS (primary); `brew install libheif` for HEIC support
- **photobridge**: macOS 13+, Xcode command-line tools; `brew install exiftool` for HEIC orientation correction

---

## Build

```bash
# picmanager
cargo build --release          # → target/release/picmanager

# photobridge (optional iCloud companion)
cd photobridge
swift build -c release
codesign --force --sign - \
  --entitlements Sources/PhotoBridge/PhotoBridge.entitlements \
  .build/release/photobridge
```

---

## Quick start

```bash
# 1. Import a folder of photos (moves files into the library)
picmanager import ~/Downloads/photos/

# 2. Start the Web UI
picmanager serve               # → http://127.0.0.1:8080

# 3. Download AI models (face + animal detection)
picmanager models fetch

# 4. Import from iCloud (PhotoBridge)
photobridge setup              # first-time guide
photobridge export             # full export + auto-import if picmanager is in PATH
photobridge sync               # incremental sync on subsequent runs
# Fix orientation for already-exported HEIC files
photobridge fix-orientations --dir ~/staging/ --dry-run  # report mismatches
photobridge fix-orientations --dir ~/staging/            # apply fixes
```

See **[docs/MANUAL.md](docs/MANUAL.md)** for full CLI reference, REST API, configuration, and PhotoBridge options.
