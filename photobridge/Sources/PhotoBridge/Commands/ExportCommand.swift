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
            await withTaskGroup(of: Void.self) { group in
                for (asset, resource) in batch {
                    group.addTask {
                        let destURL = exportDestinationURL(
                            stagingDir: stagingDir,
                            localIdentifier: asset.localIdentifier,
                            uti: resource.uniformTypeIdentifier
                        )
                        do {
                            try await writeAssetResource(resource, to: destURL)
                        } catch {
                            fputs("  ✗ \(asset.localIdentifier): \(error)\n", stderr)
                        }
                    }
                }
            }
            exported += batch.count
            idx = batchEnd
            print("  \(exported)/\(pairs.count) exported", terminator: "\r")
            fflush(stdout)
        }
        print()
        print("Done. \(exported - failed) exported, \(failed) failed.")
        print("Next: run picmanager import --copy --batch-size \(batchSize) '\(stagingDir.path)'")
    }

    private func resolveStagingDir() -> URL {
        if let output {
            return URL(fileURLWithPath: output)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return support.appendingPathComponent("PhotoBridge/staging")
    }
}
