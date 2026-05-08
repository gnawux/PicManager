# Garmin 数据同步方案调研

## 背景

PicManager 2.0 需要导入 Garmin 运动数据（FIT/GPX 文件）。本文档调研从 Garmin 获取数据的可行方案，评估各方案的可靠性和实现成本。

---

## 方案一：手动下载（MVP，当前实现）

用户从 Garmin Connect 网页或 App 手动下载 FIT/GPX 文件，放入指定目录，再运行：

```bash
picmanager activities import ~/Downloads/garmin/
```

**优点：**
- 零开发成本，立即可用
- 无需处理认证、网络、API 变更等问题
- 用户完全掌控导入时机

**缺点：**
- 需要用户手动操作，不自动化

**适用场景：** 偶尔整理一批历史数据；初期 MVP 阶段。

---

## 方案二：Garmin 设备 USB 直连（推荐短期实现）

将 Garmin 手表通过 USB 连接 Mac 后，设备以存储模式挂载。FIT 文件直接存放在设备文件系统中：

```
/Volumes/GARMIN/GARMIN/Activity/
  2026-05-01-10-30-00.fit
  2026-05-03-07-15-22.fit
  ...
```

可增加 CLI 命令：

```bash
picmanager activities sync-usb                  # 自动检测挂载的 Garmin 设备
picmanager activities sync-usb --device GARMIN  # 指定卷名
```

实现思路：
1. 扫描 `/Volumes/` 下名称包含 `GARMIN` 的卷（也可让用户配置路径）
2. 读取 `{volume}/GARMIN/Activity/` 下所有 `.fit` 文件
3. 按 SHA-256 去重后批量导入

**优点：**
- 完全离线，无需账号认证
- 不依赖第三方 API，不受 Garmin 政策影响
- 文件是设备原始 FIT，数据最完整
- 实现简单（就是目录扫描）

**缺点：**
- 需要物理连接设备，不适合自动后台同步
- 部分新款 Garmin 表型号挂载路径可能不同（需测试）

**结论：值得作为 2.0 短期目标实现。**

---

## 方案三：Garmin 官方 Connect Developer API

Garmin 官方提供 [Connect Developer Program](https://developer.garmin.com/gc-developer-program/overview/)，包含 Activity API、Health API 等，支持下载 FIT/GPX/TCX 文件。

**限制：**
- **面向企业/合作伙伴**，需要申请并通过审核（1–4 周）
- 审核通过后才能创建 OAuth 2.0 应用并获得 Client ID/Secret
- 协议要求不得用于纯个人/家庭用途的私有部署工具（条款存在模糊地带）
- 部分商业用途需要支付授权费用

**结论：个人家庭工具不适合走官方 API 通道，放弃此方案。**

---

## 方案四：非官方 API（python-garminconnect）

[cyberjunky/python-garminconnect](https://github.com/cyberjunky/python-garminconnect) 是最流行的非官方 Garmin Connect 客户端（2.2k stars，71 次发布），通过模拟 Garmin 官方 App 的 SSO 登录流程访问 Garmin Connect 内部 API。

**能做什么：**
- 用 Garmin 账号邮箱+密码（支持 MFA）登录，获取 OAuth token
- 列出所有活动、下载活动详情
- token 自动刷新，re-login 频率极低

**FIT 文件下载：**
该库主要暴露结构化 JSON 数据接口。Garmin Connect 网页端确实提供 FIT 文件下载，但通过非官方 API 下载原始 FIT 文件的接口不在主要文档中，需进一步验证可行性。

**稳定性风险：**
- Garmin 随时可以更改内部 API，导致库失效
- 历史上已多次因 Garmin 更改 SSO 流程而短暂失效
- 不受官方支持，无 SLA

**实现方式（如果要做）：**
写一个类似 PhotoBridge 的 Python 桥接工具 `GarminBridge`：
```bash
garmin-bridge sync --output ~/garmin-activities/
# 然后
picmanager activities import ~/garmin-activities/
```

**结论：可行但脆弱，作为可选增强，不作为主路径。如果有需求可以后续评估。**

---

## 方案五：通过 Strava 中转

如果用户已在 Garmin Connect 中开启 Strava 自动同步，可以通过 [Strava API](https://developers.strava.com/)（完全公开，免费申请）获取活动数据和 FIT 文件。

**优点：**
- Strava API 对个人开发者完全开放，OAuth2 流程标准
- 活动上传后即可通过 API 下载原始 FIT 文件

**缺点：**
- 需要用户同时使用 Strava（额外依赖）
- Garmin → Strava 同步有延迟（通常几分钟内）

**结论：对已用 Strava 的用户是个好选择，但不能作为通用方案。**

---

## 建议实现路线

| 阶段 | 方案 | 说明 |
|------|------|------|
| **2.0 MVP** | 手动导入 | `picmanager activities import <dir>`，零成本立即可用 |
| **2.0 短期** | USB 直连 | `picmanager activities sync-usb`，离线可靠，实现简单 |
| **2.x 可选** | 非官方 API 桥接 | 单独工具，用户自愿启用，按需维护 |
| **不做** | 官方 Garmin API | 企业通道，不适合个人工具 |

---

## 参考链接

- [Garmin Connect Developer Program](https://developer.garmin.com/gc-developer-program/overview/)
- [cyberjunky/python-garminconnect](https://github.com/cyberjunky/python-garminconnect)
- [bes-dev/garmy（自托管/AI 方向）](https://github.com/bes-dev/garmy)
- [Strava API Docs](https://developers.strava.com/)
