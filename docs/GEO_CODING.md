# 地理编码（Reverse Geocoding）设计文档

## 概述

PicManager 使用 OSM Nominatim 对照片的 GPS 坐标进行反向地理编码，将坐标转换为城市/州省/国家信息，用于在地点标签页展示照片层级和地图打点。

核心实现位于 `src/album/location.rs`。

---

## 数据流

### 导入时（Import）

```
import_dir_inner / import_dir_batch
  └─ 全部照片导入完成后，调用一次：
     group_by_location_scoped(pool, &newly_imported_ids, ...)
       └─ 按 (gps_lat, gps_lon) 排序，逐张调用 cached_or_fetch
            └─ 成功 → ensure_location_album（创建/更新 location 相册）
```

### 手动修复时（Fill Geo）

```
POST /api/geo/regeocode
  └─ tokio::spawn group_by_location(pool)
       └─ 处理全库所有有 GPS 的照片，逻辑同 cached_or_fetch
```

---

## 三级缓存策略（cached_or_fetch）

```
L1  session_cache (in-memory HashMap)
    ↓ miss
L2  geocache 表精确匹配（lat_key = PRINTF('%.4f', lat)）
    ↓ miss 或 全NULL（瞬时失败标记）或 stale（city 有值但 state 为 NULL）
L3  邻近查找（±0.01°，≈1km）
    仅返回 city/state/country 至少一项不为 NULL 的记录
    命中后写回精确 key，更新 session_cache
    ↓ miss
    Nominatim API 调用（1 req/s 限速）
    结果写入 geocache（失败时写全 NULL 作为瞬时失败标记）
    更新 session_cache
```

### 坐标精度

`coord_key()` 保留 4 位小数（`GEO_COORD_PRECISION = 4`），对应约 11 m 精度，作为 geocache 的 `lat_key` / `lon_key`。

### Stale Entry 检测

geocache 中 `city IS NOT NULL AND state IS NULL AND country IS NOT NULL` 的记录被视为旧格式（migration 之前写入的记录缺少 state 字段），会跳过直接返回，重新走 L3/Nominatim。

**注意**：台湾、韩国等 Nominatim 本身不返回 state 的地区，每次导入都会触发一次邻近查找或 Nominatim 请求；因为邻近查找通常能命中，不会产生实际 API 调用。

---

## 处理顺序与邻近缓存的关系

`group_by_location_scoped` 查询照片时按 `(gps_lat, gps_lon)` 排序，确保地理位置相近的照片连续处理。这样做的好处：

- 若照片 A（坐标 39.9001）的 Nominatim 调用成功，地理相邻的照片 B（坐标 39.9003）能通过 L3 邻近查找直接命中 A 的缓存记录，无需额外网络请求。
- 若照片 A 的 Nominatim 调用失败（全 NULL），照片 B 仍需独立尝试；但 B 成功后，下次 fill geo 处理 A 时，A 的邻近查找能命中 B 的记录，快速补全。

若不按地理坐标排序，按 ID（插入）顺序处理，同一地点的照片分散在整个队列中，邻近缓存的有效性大幅降低。

---

## 全 NULL 记录（瞬时失败标记）

当 Nominatim 调用失败（超时、网络错误、该坐标无地名数据）时，向 geocache 写入全 NULL 记录：

```sql
INSERT OR REPLACE INTO geocache (lat_key, lon_key, city, state, county, country)
VALUES (?, ?, NULL, NULL, NULL, NULL)
```

**目的**：标记"已尝试过该坐标"，避免下次同一 session 重复调用 Nominatim。

**影响**：
- `count_missing_geo()` 将该照片计入"待补全"，显示在 📍 图标徽章中。
- 地点层级查询（`get_geo_hierarchy`）INNER JOIN geocache，全 NULL 记录会让照片以 `Unknown > Unknown > Unknown` 出现在层级中。
- 点击 📍 修复地理位置后，这些记录会被重新处理；如附近有已成功的记录，邻近查找会快速填入，无需网络请求。

---

## Nominatim 特殊处理

### 城市字段回退链

Nominatim 返回的 `address` 对象按以下顺序查找城市：

```
city → town → village → county（区/县级名称）
```

若前三者均无，则用 `county` 作为城市名（常见于直辖市 zoom=10 返回区级名称的场景）。

### 中国直辖市

北京、上海、天津、重庆在 Nominatim 中没有 `state` 字段，通过 `ISO3166-2-lvl4` 字段（如 `CN-BJ`）识别并映射到省级名称：

```rust
fn cn_municipality_state(iso: &str) -> Option<&'static str> {
    match iso {
        "CN-BJ" => Some("北京市"),
        "CN-SH" => Some("上海市"),
        "CN-TJ" => Some("天津市"),
        "CN-CQ" => Some("重庆市"),
        _ => None,
    }
}
```

### 请求参数

```
zoom=10            返回城市级别（不是街道）
accept-language=zh,en  优先中文名称，回退英文
```

---

## 地点层级查询

`GET /api/geo/hierarchy` 通过 INNER JOIN 将照片与 geocache 关联：

```sql
SELECT gc.country, gc.state, gc.city, COUNT(DISTINCT ph.id) AS cnt
FROM photos ph
JOIN geocache gc
  ON PRINTF('%.4f', ph.gps_lat) = gc.lat_key
 AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
WHERE ph.import_status = 'imported' AND ph.gps_lat IS NOT NULL
GROUP BY gc.country, gc.state, gc.city
```

- geocache 字段为 NULL 时，前端展示为 `Unknown`。
- 没有 geocache 记录的照片（从未走过 geocoding）不出现在层级中。

`GET /api/geo/photos` 支持 `country` / `state` / `city` 过滤参数：
- 普通字符串：`gc.field = ?`
- 特殊值 `__null__`：`gc.field IS NULL`（用于筛选 Unknown 层级下的照片）

---

## 邻近地理编码缓存（Proximity Geocache）

邻近查找设计见 `docs/ARCHITECTURE.md` 的"邻近地理编码缓存"节，核心参数：

| 参数 | 值 | 说明 |
|------|-----|------|
| `PROXIMITY_DEG` | 0.01° | 约 1 km，搜索半径 |
| `GEO_COORD_PRECISION` | 4 位小数 | 约 11 m 精度 |

---

## 常见问题

### Q：为什么导入后还有一批 Unknown 照片？

导入时，地理编码按地理坐标顺序处理，但仍有少量照片可能遇到：

1. **Nominatim 瞬时失败**（超时/网络抖动），且没有邻近成功记录可借用。
2. **Nominatim 无数据**（极偏远区域无地名），任何时候都返回 null。

对于情况 1，点击 📍 修复后，由于其他照片的 geocache 已在导入时写入，邻近查找会快速填入，通常无需 Nominatim 网络请求，速度很快。

### Q：修复地理位置为什么这么快？

fill geo（`group_by_location`）重新处理全库照片时，之前导入成功的照片已在 geocache 中留下记录。失败照片通过 L3 邻近查找直接命中这些记录，不产生 Nominatim API 调用，因此整体很快。

### Q：台湾/韩国照片的州/省显示 Unknown？

这是正常现象。Nominatim 对台湾、韩国部分地区不返回 `state` 字段，geocache 中该字段为 NULL，地点层级展示为 `Unknown`。点击 `Unknown` 州/省下的城市可正常显示照片。
