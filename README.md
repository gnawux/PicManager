# PicManager

A family photo management tool. Import, de-duplicate, organise by time / location / camera, detect faces and animals, and browse everything through a Web UI — all running locally, no cloud required.

中文文档：[README.zh.md](README.zh.md)

---

## Executables

| Binary | Language | Purpose |
|--------|----------|---------|
| `picmanager` | Rust | Main tool — import, dedup, serve Web UI, CLI |
| `photobridge` | Swift | iCloud Photos companion — export from Photos.app and feed into picmanager |
| `PicManager.app` | Swift + Rust/Web | macOS menu-bar product with embedded or browser UI |

---

## Core features

- **Import** — scan a directory, move photos into a dated library (`library/YYYY-MM-DD/`), skip duplicates by SHA-256
- **Smart date inference** — EXIF → file mtime → filename pattern → `unknown/`
- **Two-layer dedup** — Gradient pHash (fast filter) + DCT pHash (precision verify) + Union-Find grouping
- **Auto albums** — by month, by camera model, by GPS city (reverse-geocoded via OSM Nominatim)
- **Face detection & clustering** — ultraface-slim-320 + ArcFace 512-D embeddings + DBSCAN people grouping; all local, no API key
- **Animal detection** — YOLOv8-nano, 10 COCO species, runs on import
- **Modern Web UI** — high-density justified timeline, cursor loading, immersive viewer, albums, people, map, activities, Apple inventory and unified task/dedup review
- **Reliable Apple Photos inventory** — preserves PhotoKit identity and original filenames, exposes unsynced/failed/excluded/missing items, and uses durable retryable work
- **Rendition fidelity** — immutable originals and separate current-edited appearances, with centralized HEIC orientation handling and revision-aware derived media
- **Safe catalog upgrades** — additive asset/source/variant records, dry-run and integrity reports, idempotent legacy backfill and migration audit history
- **PhotoBridge** — metadata-only inventory and incremental PhotoKit changes, plus explicit original/current rendition packages

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

For macOS daily use, build/install `PicManager.app`, choose a dedicated library folder,
grant Full Photos access, and refresh Apple Photos inventory from the menu bar. See the
**[Mac app guide](docs/MACOS_APP.md)**.

```bash
# 1. Import a folder of photos (moves files into the library)
picmanager import ~/Downloads/photos/

# 2. Start the modern Web UI
picmanager serve               # → http://127.0.0.1:8080

# 3. Download AI models (face + animal detection)
picmanager models fetch

# 4. Inventory Apple Photos without downloading the full library
photobridge inventory --output /tmp/apple-inventory.ndjson
picmanager apple inventory /tmp/apple-inventory.ndjson --dry-run --json
picmanager apple inventory /tmp/apple-inventory.ndjson
```

The modern interface is served at `/`; the previous interface remains available at
`/legacy/` during the transition. Before upgrading an existing library, follow the
**[upgrade guide](docs/UPGRADE_GUIDE.md)**. See **[docs/MANUAL.md](docs/MANUAL.md)**
for the full CLI reference, REST API, configuration, and PhotoBridge options.

Current documentation is indexed at **[docs/README.md](docs/README.md)**.
