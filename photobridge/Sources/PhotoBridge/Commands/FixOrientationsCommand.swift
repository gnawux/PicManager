import ArgumentParser
import Foundation
import PhotoBridgeLib
import Photos
import SQLite3

// MARK: - Command

@available(macOS 13, *)
struct FixOrientationsCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "fix-orientations",
        abstract: "Fix EXIF orientation for HEIC files where Apple Photos has a rotation correction"
    )

    @Option(name: .long, help: "Directory to scan (staging or PicManager library)")
    var dir: String

    @Option(name: .long, help: "PicManager SQLite DB (enables DB update + thumbnail clearing)")
    var picmanagerDb: String?

    @Option(name: .long, help: "PicManager thumbs directory (required with --picmanager-db)")
    var thumbsDir: String?

    @Option(name: .long, help: "Max concurrent Photos queries (default: 16)")
    var maxConcurrent: Int = 16

    @Flag(name: .long, help: "Report mismatches only; do not modify files or DB")
    var dryRun: Bool = false

    func run() async throws {
        let authResult = try await requestPhotoLibraryAccess()
        if authResult == .limited {
            fputs("⚠️  Limited Photos access — some assets may be skipped.\n", stderr)
        }

        let dirURL = URL(fileURLWithPath: dir)
        guard FileManager.default.fileExists(atPath: dirURL.path) else {
            fputs("Error: directory not found: \(dir)\n", stderr)
            throw ExitCode.failure
        }

        let exiftool: URL?
        if dryRun {
            exiftool = nil
        } else {
            exiftool = findExecutable("exiftool")
            if exiftool == nil {
                fputs("Error: exiftool not found. Install with: brew install exiftool\n", stderr)
                throw ExitCode.failure
            }
        }

        let db: PicManagerOrientationDB?
        if let dbPath = picmanagerDb {
            db = PicManagerOrientationDB(path: dbPath)
            if db == nil {
                fputs("Error: cannot open PicManager DB at \(dbPath)\n", stderr)
                throw ExitCode.failure
            }
        } else {
            db = nil
        }
        let thumbsDirURL = thumbsDir.map { URL(fileURLWithPath: $0) }

        // 1. Scan for HEIC/HEIF files (recursive)
        print("Scanning \(dir)…")
        let heicFiles = scanHEICFiles(in: dirURL)
        print("Found \(heicFiles.count) HEIC/HEIF files.")
        guard !heicFiles.isEmpty else { return }

        // 2. Extract localIdentifiers from filenames
        let pairs: [(fileURL: URL, localID: String)] = heicFiles.compactMap { url in
            guard let lid = localIdentifierFromFilename(url.lastPathComponent) else { return nil }
            return (url, lid)
        }
        let unparsed = heicFiles.count - pairs.count
        if unparsed > 0 {
            fputs("⚠️  \(unparsed) files skipped (name doesn't match PhotoBridge convention).\n", stderr)
        }

        // 3. Batch-fetch PHAssets (500 per call to stay within API limits)
        print("Fetching Photos asset metadata…")
        let allLocalIDs = pairs.map { $0.localID }
        var idToAsset: [String: PHAsset] = [:]
        let chunkSize = 500
        var fetched = 0
        for chunkStart in stride(from: 0, to: allLocalIDs.count, by: chunkSize) {
            let chunk = Array(allLocalIDs[chunkStart..<min(chunkStart + chunkSize, allLocalIDs.count)])
            let result = PHAsset.fetchAssets(withLocalIdentifiers: chunk, options: nil)
            result.enumerateObjects { asset, _, _ in idToAsset[asset.localIdentifier] = asset }
            fetched += chunk.count
            print("  \(fetched)/\(allLocalIDs.count) IDs resolved", terminator: "\r")
            fflush(stdout)
        }
        print()
        print("Matched \(idToAsset.count)/\(pairs.count) assets in Photos library.")

        // 4. Check and fix orientations in batches
        let total = pairs.count
        var checked = 0
        var fixed = 0
        var dryMismatches = 0
        var notInPhotos = 0
        var noExif = 0
        var errors = 0

        var reportLines: [(String, Int, Int)] = []  // (filename, fileOrient, photosOrient)

        var idx = 0
        while idx < pairs.count {
            let batchEnd = min(idx + maxConcurrent, pairs.count)
            let batch = Array(pairs[idx..<batchEnd])

            let results: [(fileURL: URL, photosOrient: Int?, fileOrient: Int?)]
                = await withTaskGroup(
                    of: (URL, Int?, Int?).self,
                    returning: [(URL, Int?, Int?)].self
                ) { group in
                    for (fileURL, localID) in batch {
                        let asset = idToAsset[localID]
                        group.addTask {
                            let fileOrient = readExifOrientationFromFile(at: fileURL)
                            guard let asset else { return (fileURL, nil, fileOrient) }
                            let photosOrient = await requestPhotosDisplayOrientation(for: asset)
                            return (fileURL, photosOrient, fileOrient)
                        }
                    }
                    var out: [(URL, Int?, Int?)] = []
                    for await r in group { out.append(r) }
                    return out
                }

            for (fileURL, photosOrient, fileOrient) in results {
                checked += 1

                guard let photosOrient else { notInPhotos += 1; continue }
                guard let fileOrient else { noExif += 1; continue }
                guard photosOrient != fileOrient else { continue }

                // Mismatch: Photos canonical orientation differs from embedded EXIF
                if dryRun {
                    reportLines.append((fileURL.lastPathComponent, fileOrient, photosOrient))
                    dryMismatches += 1
                    continue
                }

                // Fix EXIF in file
                guard let exiftool else { errors += 1; continue }
                runProcess(exiftool, args: [
                    "-overwrite_original", "-Orientation=\(photosOrient)", "-n", fileURL.path
                ])

                // Update PicManager DB + clear thumbnail
                if let db, let thumbsDirURL {
                    if let photoID = await db.updateOrientation(
                        filePath: fileURL.path, orientation: photosOrient
                    ) {
                        let thumbURL = thumbsDirURL.appendingPathComponent("\(photoID).jpg")
                        try? FileManager.default.removeItem(at: thumbURL)
                    }
                } else if let db {
                    let _ = await db.updateOrientation(filePath: fileURL.path, orientation: photosOrient)
                }

                fixed += 1
                print("  fixed: \(fileURL.lastPathComponent)  \(fileOrient) → \(photosOrient)")
            }

            idx = batchEnd
            let pct = checked * 100 / total
            print("  progress: \(checked)/\(total) (\(pct)%)", terminator: "\r")
            fflush(stdout)
        }
        print()

        // Summary
        if dryRun {
            if !reportLines.isEmpty {
                print("\nMismatches (would fix):")
                for (name, file, photos) in reportLines {
                    print("  \(name)  file=\(file) → photos=\(photos)")
                }
            }
            print("\nDry run: \(dryMismatches) would be fixed, \(notInPhotos) not in Photos library, \(noExif) no EXIF tag.")
        } else {
            let dbNote = db != nil ? " (DB updated)" : ""
            print("Done: \(fixed) fixed\(dbNote), \(notInPhotos) not in Photos library, \(noExif) no EXIF tag, \(errors) errors.")
        }
    }
}

// MARK: - SQLite DB actor

/// Serializes all PicManager DB writes so concurrent tasks don't conflict.
actor PicManagerOrientationDB {
    // nonisolated(unsafe) lets deinit close the handle without a Sendable requirement.
    nonisolated(unsafe) private var handle: OpaquePointer?

    init?(path: String) {
        guard sqlite3_open(path, &handle) == SQLITE_OK else { return nil }
    }

    deinit { sqlite3_close(handle) }

    /// Updates exif_orientation for the photo at filePath. Returns photo ID, or nil if not found.
    func updateOrientation(filePath: String, orientation: Int) -> Int64? {
        filePath.withCString { filePathCStr -> Int64? in
            // UPDATE exif_orientation
            let updateSQL = "UPDATE photos SET exif_orientation = ? WHERE path = ?"
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(handle, updateSQL, -1, &stmt, nil) == SQLITE_OK else { return nil }
            sqlite3_bind_int64(stmt, 1, Int64(orientation))
            sqlite3_bind_text(stmt, 2, filePathCStr, -1, nil)
            let rc = sqlite3_step(stmt)
            sqlite3_finalize(stmt)
            guard rc == SQLITE_DONE else { return nil }

            // SELECT id
            let selectSQL = "SELECT id FROM photos WHERE path = ?"
            var idStmt: OpaquePointer?
            guard sqlite3_prepare_v2(handle, selectSQL, -1, &idStmt, nil) == SQLITE_OK else { return nil }
            sqlite3_bind_text(idStmt, 1, filePathCStr, -1, nil)
            var photoID: Int64? = nil
            if sqlite3_step(idStmt) == SQLITE_ROW {
                photoID = sqlite3_column_int64(idStmt, 0)
            }
            sqlite3_finalize(idStmt)
            return photoID
        }
    }
}

// MARK: - Helpers

/// Recursively finds all HEIC/HEIF files under `dir`.
private func scanHEICFiles(in dir: URL) -> [URL] {
    guard let enumerator = FileManager.default.enumerator(
        at: dir,
        includingPropertiesForKeys: [.isRegularFileKey],
        options: [.skipsHiddenFiles]
    ) else { return [] }
    var files: [URL] = []
    for case let url as URL in enumerator {
        let ext = url.pathExtension.lowercased()
        if ext == "heic" || ext == "heif" { files.append(url) }
    }
    return files
}

/// Reverses the PhotoBridge filename → localIdentifier transformation.
///
/// Example:
///   "B5A8F3C2-1234-5678-ABCD-000000000001_L0_001.heic"
///   → "B5A8F3C2-1234-5678-ABCD-000000000001/L0/001"
///
/// The UUID is always 36 characters (8-4-4-4-12 with dashes).
/// All `/` after the UUID were replaced with `_` by exportDestinationURL; we reverse that.
private func localIdentifierFromFilename(_ filename: String) -> String? {
    let base = URL(fileURLWithPath: filename).deletingPathExtension().lastPathComponent
    guard base.count > 36 else { return nil }
    let uuidEnd = base.index(base.startIndex, offsetBy: 36)
    // Quick sanity check: UUID dashes at positions 8, 13, 18, 23
    guard base[base.index(base.startIndex, offsetBy: 8)] == "-",
          base[base.index(base.startIndex, offsetBy: 13)] == "-" else { return nil }
    let uuid = String(base[..<uuidEnd])
    let suffix = String(base[uuidEnd...]).replacingOccurrences(of: "_", with: "/")
    return uuid + suffix
}
