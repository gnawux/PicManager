# PicManager for macOS

## Product boundary

The app is a SwiftUI menu-bar shell around the Rust service and existing Web UI. It does
not contain a second photo-management frontend. The app manages first run, PhotoKit
permission, service lifecycle, notifications, inventory refresh, diagnostics, and
presentation mode.

## First run and daily use

- Select a dedicated PicManager library folder and grant Full Photos access.
- The app acquires a per-library app lock and the Rust service acquires a separate
  service lock. A second owner is refused rather than allowed to corrupt the catalog.
- The service contract includes a one-way library fingerprint; a port serving another
  library is rejected.
- Embedded mode uses WKWebView. External links open in the system browser. Browser mode
  opens the same loopback URL directly.
- Health is polled every five seconds. Repeated failures trigger one native notification
  until recovery.

## Settings and data

Changing library, host, or port stops the owned service, releases the old lock, saves the
new configuration, and starts a matching service. Launch at login uses
`SMAppService.mainApp`; macOS may require approval in System Settings.

Configuration and redacted logs live under `~/Library/Application Support/PicManager`.
The selected photo library is separate and is not removed when the app is uninstalled.

## Diagnostics

**Export Diagnostics** creates a ZIP containing only allowlisted configuration fields,
health/task counts, and sanitized logs. It excludes bookmarks, executable/library paths,
database files, media files, and media filenames.

## Development run

```bash
swift run --package-path photobridge PicManagerMac
```

For an installable bundle use `scripts/build-macos-app.sh`; do not distribute an ad-hoc
signed build.
