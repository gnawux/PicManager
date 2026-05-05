import ArgumentParser
import Foundation
import PhotoBridgeLib
import Photos

@available(macOS 13, *)
struct SyncCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "sync",
        abstract: "Incrementally export photos added since the last sync"
    )

    @Option(name: .long, help: "Staging directory for exported files")
    var output: String?

    @Option(name: .long, help: "Photos per batch (default: 200)")
    var batchSize: Int = 200

    @Option(name: .long, help: "Max concurrent iCloud downloads (default: 4)")
    var maxConcurrent: Int = 4

    @Option(name: .long, help: "Path to picmanager executable")
    var picmanager: String?

    @Flag(name: .long, help: "Count new assets only, do not export")
    var dryRun: Bool = false

    func run() async throws {
        let authResult = try await requestPhotoLibraryAccess()
        if authResult == .limited {
            fputs("⚠️  Photos access is limited. Grant full access for complete sync.\n", stderr)
        }

        let stateURL = IncrementalEnumerator.defaultStateURL
        var state = SyncState.load(from: stateURL)

        let stagingDir = resolveStagingDir()
        if !dryRun {
            try FileManager.default.createDirectory(at: stagingDir, withIntermediateDirectories: true)
        }

        if state.lastSyncToken == nil {
            print("No previous sync token — performing full enumeration…")
        } else {
            let dateStr = state.lastSyncDate.map { formatDate($0) } ?? "unknown"
            print("Fetching changes since \(dateStr)…")
        }

        let enumerator = IncrementalEnumerator()
        let (pairs, newToken) = enumerator.enumerate(token: state.lastSyncToken) { processed, total in
            if processed % 500 == 0 || processed == total {
                print("  \(processed)/\(total) assets scanned", terminator: "\r")
                fflush(stdout)
            }
        }
        print()

        if dryRun {
            print("Dry run: \(pairs.count) assets would be exported.")
            return
        }

        if pairs.isEmpty {
            print("No new assets since last sync.")
            state.lastSyncToken = newToken
            state.lastSyncDate = Date()
            try state.save(to: stateURL)
            return
        }

        // Disk space pre-check
        let assets = pairs.map { $0.0 }
        let libraryPath: String? = nil  // PHPhotoLibrary has no public API for library path
        if let warning = checkDiskSpace(stagingDir: stagingDir, assets: assets, photosLibraryPath: libraryPath) {
            if warning.isSystemVolume {
                fputs("""
                ⚠️  Photos Library is on the system volume. The Photos framework will temporarily
                    download iCloud photos to the system volume cache during sync.
                    Consider moving your Photos Library to an external drive first.
                    Run: photobridge setup\n
                """, stderr)
            }
            if Double(warning.estimatedBytes) > Double(warning.freeBytes) * 0.8 {
                fputs("⚠️  Estimated download size (\(formatBytes(warning.estimatedBytes))) may exceed 80% of free space (\(formatBytes(warning.freeBytes))). Proceeding anyway.\n", stderr)
            }
        }

        print("Exporting \(pairs.count) new assets to \(stagingDir.path)…")
        var exported = 0
        var exportFailed = 0

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

        state.lastSyncToken = newToken
        state.lastSyncDate = Date()
        state.exportedCount += exported
        try state.save(to: stateURL)

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
            print("Done. \(exported) exported, \(exportFailed) failed (total ever: \(state.exportedCount)).")
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

    private func formatDate(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return f.string(from: date)
    }
}
