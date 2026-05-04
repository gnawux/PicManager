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

        print("Exporting \(pairs.count) new assets to \(stagingDir.path)…")
        var exported = 0
        var failed = 0

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

        state.lastSyncToken = newToken
        state.lastSyncDate = Date()
        state.exportedCount += exported
        try state.save(to: stateURL)

        print("Done. \(exported) exported, \(failed) failed (total ever: \(state.exportedCount)).")
        print("Next: run picmanager import --batch-size \(batchSize) '\(stagingDir.path)'")
    }

    private func resolveStagingDir() -> URL {
        if let output {
            return URL(fileURLWithPath: output)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return support.appendingPathComponent("PhotoBridge/staging")
    }

    private func formatDate(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return f.string(from: date)
    }
}
