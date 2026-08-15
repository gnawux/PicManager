# macOS installation test matrix

Run `scripts/test-macos-install-lifecycle.sh dist/PicManager.app` for the automated
filesystem-preservation test. Before distributing a signed and notarized build, also
complete these interactions on a clean macOS user account:

1. Copy `PicManager.app` to Applications and confirm no catalog or configuration is
   created before launch.
2. Launch the app, choose a temporary PicManager library, grant Full Photos access, and
   confirm the app explicitly leaves the System Photo Library in place.
3. Refresh Apple Photos inventory and verify synchronized, pending, excluded, and failed
   counts against the Web source view.
4. Quit, replace only `PicManager.app` with the upgrade candidate, relaunch, and confirm
   the same library, photo IDs, metadata, tasks, and files remain available.
5. Move the app to Trash. Confirm the selected photo library and
   `~/Library/Application Support/PicManager` remain untouched. Remove those separately
   only when the user explicitly requests complete data deletion.

Do not approve a release if migration needs destructive schema changes, the older
version is below the update manifest rollback floor, or backup verification fails.
