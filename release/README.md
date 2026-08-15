# Release metadata

`update-manifest.schema.json` defines the offline metadata contract for a future
updater. Generate a manifest only after assembling, signing, notarizing, and archiving
the app. The generator writes a local file and never uploads the archive or publishes a
feed.

Rollback is supported only to `minimum_rollback_version` in the manifest. Keep the
catalog backup created before an upgrade; media files and the catalog are never removed
by app replacement or rollback. A release that introduces a non-additive migration must
raise the rollback floor and document a restore procedure before publishing metadata.
