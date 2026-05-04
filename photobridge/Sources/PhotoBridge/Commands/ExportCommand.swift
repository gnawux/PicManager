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

        print("Exporting \(pairs.count) assets to \(stagingDir.path)…")
        var exported = 0
        var failed = 0

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
                            try await writeAssetResource(resource, to: destURL)
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
            failed += batch.count - batchSucceeded
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

        print("Done. \(exported) exported, \(failed) failed.")
        print("Next: run picmanager import --batch-size \(batchSize) '\(stagingDir.path)'")
    }

    private func resolveStagingDir() -> URL {
        if let output {
            return URL(fileURLWithPath: output)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return support.appendingPathComponent("PhotoBridge/staging")
    }
}
