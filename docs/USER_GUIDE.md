# PicManager user guide

PicManager keeps its catalog and original media on your Mac. The Rust service owns the
catalog and background work; the same Web photo interface is available in a browser or
inside the Mac app.

## Recommended Mac workflow

1. Install and open `PicManager.app`.
2. Choose a PicManager library folder. This is not the Photos.app System Photo Library.
3. Grant Full Photos access. PicManager reads the System Photo Library selected by
   Photos.app and never switches or moves it.
4. Open the library in the embedded window or choose system-browser mode in Settings.
   The Mac app starts its bundled service automatically; do not run `picmanager serve`
   separately. While the service prepares the catalog, the window shows a bounded
   **Preparing library** state instead of loading the Web interface prematurely.
5. Choose **Refresh Apple Photos Inventory** to compare metadata. This operation does
   not download or replace originals.
6. Use the Apple source view to inspect synchronized, pending, excluded, missing, and
   failed items. Failed tasks can be retried from the task center or menu bar.

The first inventory preserves PhotoKit local identifiers separately from original phone
filenames. UUID-like provider identities are never used as user-facing filenames.

Large libraries perform only safety-critical recovery before the local service becomes
available. Media-presence and derived-cache checks continue as a low-priority
**Library reconciliation** task visible in the task center.

## Browsing places and albums

The Places tab uses OpenStreetMap tiles when the Mac is online. Drag to move the map,
use the zoom buttons or trackpad/wheel to reveal finer photo clusters, and click a
numbered cluster to browse every represented photo. **全屏** gives the map the entire
app window; press Escape or **收起** to leave that mode. Tile requests
describe only the visible map area—PicManager does not send photos to OpenStreetMap.
Without a network connection, cached tiles may remain visible and the local place tree
and photo results continue to work.

The place tree is sorted by photo count and initially folded at country level. Expand a
country or state, or choose its “view all” action, to load matching photos in the right
pane. The navigation tree and results scroll independently.

“统一已有地名”需要访问 Nominatim。PicManager.app 会在启动本地服务时继承 macOS
中启用的手动 HTTP、HTTPS 和 SOCKS 代理；修改代理后请退出并重新打开应用。任务
进度按唯一 GPS 坐标统计，因此总数通常明显小于待修正的照片数。网络不可用时任务
会在连续三次失败后停止并允许重试，不会继续逐张照片超时。

如果正在从旧版本替换应用，并且旧地名任务仍在运行：先在任务页点“取消”，再退出
旧应用并替换 `.app`。即使旧界面暂时仍显示 running，取消标记也已经持久化；新版
服务启动后会把过期 lease 恢复为 cancelled。确认代理可用后，再从地点页重新启动
地名统一。现有照片和旧地名不会因取消而丢失。

The Albums tab follows the same split-pane pattern. Collections, months, locations,
cameras and other smart albums have separate collapsible sections on the left; choosing
one keeps the navigation visible while its photos open on the right. Entries with more
photos appear first.

## Existing files and catalogs

Upgrades keep photo IDs, metadata, relationships, source identities, and media paths.
Run the migration rehearsal before changing a valuable catalog; see
[MIGRATION_AND_RECOVERY.md](MIGRATION_AND_RECOVERY.md). Never point tests at a personal
library.

## RAW policy

PicManager prefers the JPEG/HEIC photo resource for RAW+JPEG pairs and excludes RAW-only
assets from automatic export. Existing RAW files are not deleted. Any future cleanup
must be a separate, explicit dry-run workflow after a verified backup.

## Web/CLI workflow

The standalone service remains supported:

```bash
picmanager import --copy /path/to/photos
picmanager serve
```

Open `http://127.0.0.1:8080`. Use `--copy` when source files must remain in place. See
`MANUAL.md` or `MANUAL.zh.md` for the complete CLI reference.
