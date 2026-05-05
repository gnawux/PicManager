import ArgumentParser
import Foundation
import PhotoBridgeLib
import Photos

@available(macOS 13, *)
struct ExportCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "export",
        abstract: "Full export of all photos from Photos library"
    )

    @Option(name: .long, help: "Staging directory for exported files")
    var output: String?

    @Option(name: .long, help: "Photos per batch (default: 200)")
    var batchSize: Int = 200

    @Option(name: .long, help: "Max concurrent iCloud downloads (default: 4)")
    var maxConcurrent: Int = 4

    @Option(name: .long, help: "Path to picmanager executable")
    var picmanager: String?

    @Flag(name: .long, help: "Count assets only, do not export")
    var dryRun: Bool = false

    func run() async throws {
        // Authorization
        let authResult = try await requestPhotoLibraryAccess()
        if authResult == .limited {
            fputs("⚠️  Photos access is limited. Grant full access for complete export.\n", stderr)
        }

        let stagingDir = resolveStagingDir()
        if !dryRun {
            try FileManager.default.createDirectory(at: stagingDir, withIntermediateDirectories: true)
        }

        // Snapshot the current change token before enumeration so that a
        // subsequent `photobridge sync` starts from this point in time.
        let tokenSnapshot = IncrementalEnumerator().serializeToken(
            PHPhotoLibrary.shared().currentChangeToken
        )

        print("Enumerating Photos library…")
        let enumerator = LibraryEnumerator()
        let pairs = enumerator.enumerate { processed, total in
            if processed % 500 == 0 || processed == total {
                print("  \(processed)/\(total) assets scanned", terminator: "\r")
                fflush(stdout)
            }
        }
        print()  // newline after progress

        if dryRun {
            print("Dry run: \(pairs.count) assets would be exported.")
            return
        }

        // Disk space pre-check
        let assets = pairs.map { $0.0 }
        let libraryPath: String? = nil  // PHPhotoLibrary has no public API for library path
        if let warning = checkDiskSpace(stagingDir: stagingDir, assets: assets, photosLibraryPath: libraryPath) {
            if warning.isSystemVolume {
                fputs("""
                ⚠️  Photos Library is on the system volume. The Photos framework will temporarily
                    download iCloud photos to the system volume cache during export.
                    Consider moving your Photos Library to an external drive first.
                    Run: photobridge setup\n
                """, stderr)
            }
            if Double(warning.estimatedBytes) > Double(warning.freeBytes) * 0.8 {
                fputs("⚠️  Estimated download size (\(formatBytes(warning.estimatedBytes))) may exceed 80% of free space (\(formatBytes(warning.freeBytes))). Proceeding anyway.\n", stderr)
            }
        }

        print("Exporting \(pairs.count) assets to \(stagingDir.path)…")
        var exported = 0
        var exportFailed = 0

        // Process in batches of maxConcurrent
        var idx = 0
        while idx < pairs.count {
            let batchEnd = min(idx + maxConcurrent, pairs.count)
            let batch = Array(pairs[idx..<batchEnd])
            let batchSucceeded = await withTaskGroup(of: Bool.self) { group in
                for (asset, resource) in batch {
                    group.addTask {
                        let destURL = exportDestinationURL(
                            stagingDir: stagingDir,
                            localIdentifier: asset.localIdentifier,
                            uti: resource.uniformTypeIdentifier
                        )
                        do {
                            try await writeAssetResourceOrientationFixed(resource, for: asset, to: destURL)
                            if let date = asset.creationDate ?? asset.modificationDate {
                                try? applyTimestamp(to: destURL, date: date)
                            }
                            return true
                        } catch {
                            fputs("  ✗ \(asset.localIdentifier): \(error)\n", stderr)
                            return false
                        }
                    }
                }
                var count = 0
                for await success in group { if success { count += 1 } }
                return count
            }
            exported += batchSucceeded
            exportFailed += batch.count - batchSucceeded
            idx = batchEnd
            print("  \(exported)/\(pairs.count) exported", terminator: "\r")
            fflush(stdout)
        }
        print()

        // Save sync state so `photobridge sync` only fetches changes since this export.
        var state = SyncState(
            lastSyncToken: tokenSnapshot,
            lastSyncDate: Date(),
            exportedCount: exported
        )
        try state.save(to: IncrementalEnumerator.defaultStateURL)

        // Optionally call picmanager import
        let runner = resolveRunner()
        if let runner {
            print("Importing into PicManager…")
            do {
                let result = try await runner.importBatch(stagingDir: stagingDir, batchSize: batchSize)
                for url in result.succeededPaths + result.skippedPaths {
                    try? FileManager.default.removeItem(at: url)
                }
                let imported = result.succeededPaths.count
                let skipped  = result.skippedPaths.count
                let failed   = result.failedPaths.count + exportFailed
                print("Done. \(exported) exported — \(imported) imported, \(skipped) skipped (dup), \(failed) failed.")
                if !result.failedPaths.isEmpty || exportFailed > 0 {
                    print("Failed files kept in staging for retry: \(stagingDir.path)")
                }
            } catch {
                fputs("picmanager import failed: \(error)\n", stderr)
                print("Staging files kept at: \(stagingDir.path)")
                print("Next: run picmanager import --batch-size \(batchSize) '\(stagingDir.path)'")
            }
        } else {
            print("Done. \(exported) exported, \(exportFailed) failed.")
            print("Next: run picmanager import --batch-size \(batchSize) '\(stagingDir.path)'")
        }
    }

    private func resolveStagingDir() -> URL {
        if let output {
            return URL(fileURLWithPath: output)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return support.appendingPathComponent("PhotoBridge/staging")
    }

    private func resolveRunner() -> PicManagerRunner? {
        if let path = picmanager {
            return PicManagerRunner(executableURL: URL(fileURLWithPath: path))
        }
        if let url = PicManagerRunner.findInPath() {
            return PicManagerRunner(executableURL: url)
        }
        return nil
    }
}
