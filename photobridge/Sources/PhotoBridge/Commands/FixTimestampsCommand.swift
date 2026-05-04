import ArgumentParser
import Foundation
import PhotoBridgeLib
import Photos

@available(macOS 13, *)
struct FixTimestampsCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "fix-timestamps",
        abstract: "Set file mtime/ctime from Photos library for already-exported files"
    )

    @Argument(help: "Directory containing exported files")
    var directory: String

    @Flag(name: .long, help: "Show what would be changed without modifying files")
    var dryRun: Bool = false

    func run() async throws {
        let authResult = try await requestPhotoLibraryAccess()
        if authResult == .limited {
            fputs("⚠️  Photos access is limited. Grant full access for complete results.\n", stderr)
        }

        let stagingDir = URL(fileURLWithPath: directory)
        guard FileManager.default.fileExists(atPath: stagingDir.path) else {
            fputs("Error: directory not found: \(directory)\n", stderr)
            throw ExitCode.failure
        }

        print("Enumerating Photos library…")
        let enumerator = LibraryEnumerator()
        let pairs = enumerator.enumerate { processed, total in
            if processed % 500 == 0 || processed == total {
                print("  \(processed)/\(total) assets scanned", terminator: "\r")
                fflush(stdout)
            }
        }
        print()

        var found = 0
        var updated = 0
        var skipped = 0
        var notFound = 0

        for (asset, resource) in pairs {
            found += 1
            let fileURL = exportDestinationURL(
                stagingDir: stagingDir,
                localIdentifier: asset.localIdentifier,
                uti: resource.uniformTypeIdentifier
            )
            guard FileManager.default.fileExists(atPath: fileURL.path) else {
                notFound += 1
                continue
            }
            guard let date = asset.creationDate ?? asset.modificationDate else {
                skipped += 1
                continue
            }
            if dryRun {
                print("  would update: \(fileURL.lastPathComponent) → \(formatDate(date))")
                updated += 1
            } else {
                do {
                    try applyTimestamp(to: fileURL, date: date)
                    updated += 1
                } catch {
                    fputs("  ✗ \(fileURL.lastPathComponent): \(error)\n", stderr)
                    skipped += 1
                }
            }
        }

        let verb = dryRun ? "would update" : "updated"
        print("Done. \(found) assets in library, \(updated) \(verb), \(skipped) skipped (no date), \(notFound) files not found.")
    }

    private func formatDate(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return f.string(from: date)
    }
}
