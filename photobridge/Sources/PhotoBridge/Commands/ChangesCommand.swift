import ArgumentParser
import Foundation
import PhotoBridgeLib
import Photos

enum ChangesError: Error, LocalizedError {
    case invalidToken
    case expiredHistory(String)

    var errorDescription: String? {
        switch self {
        case .invalidToken: return "The checkpoint is invalid. Run a full inventory."
        case .expiredHistory(let message):
            return "PhotoKit change history is unavailable (\(message)). Run a full inventory."
        }
    }
}

@available(macOS 13, *)
struct ChangesCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "changes",
        abstract: "Write metadata changes since a committed PicManager checkpoint"
    )

    @Option(name: .long, help: "Base64 PhotoKit checkpoint from PicManager")
    var checkpoint: String

    @Option(name: .long, help: "NDJSON output path (stdout when omitted)")
    var output: String?

    func run() async throws {
        let authorization = try await requestPhotoLibraryAccess()
        if case .limited = authorization { throw AuthError.limited }
        guard let beforeData = Data(base64Encoded: checkpoint),
              let beforeToken = deserializeChangeToken(beforeData) else {
            throw ChangesError.invalidToken
        }
        let library = PHPhotoLibrary.shared()
        let snapshot = serializeChangeToken(library.currentChangeToken)
        var changed = Set<String>()
        var removed = Set<String>()
        do {
            let result = try library.fetchPersistentChanges(since: beforeToken)
            for change in result {
                let details = try change.changeDetails(for: PHObjectType.asset)
                changed.formUnion(details.insertedLocalIdentifiers)
                changed.formUnion(details.updatedLocalIdentifiers)
                removed.formUnion(details.deletedLocalIdentifiers)
            }
        } catch {
            throw ChangesError.expiredHistory(error.localizedDescription)
        }
        changed.subtract(removed)

        let outputHandle: FileHandle
        var temporaryURL: URL?
        var finalURL: URL?
        if let output {
            finalURL = URL(fileURLWithPath: output)
            temporaryURL = URL(fileURLWithPath: output + ".partial")
            FileManager.default.createFile(atPath: temporaryURL!.path, contents: nil)
            outputHandle = try FileHandle(forWritingTo: temporaryURL!)
        } else { outputHandle = .standardOutput }
        defer { if output != nil { try? outputHandle.close() } }
        let encoder = inventoryEncoder()
        func write<T: Encodable>(_ record: T) throws {
            try outputHandle.write(contentsOf: encoder.encode(record))
            try outputHandle.write(contentsOf: Data([0x0a]))
        }

        try write(InventoryChangeBoundaryRecord(
            recordType: "changes_header", checkpointBefore: beforeData, checkpointAfter: nil
        ))
        let fetched = PHAsset.fetchAssets(withLocalIdentifiers: Array(changed), options: nil)
        var assetCount = 0
        var writeError: Error?
        fetched.enumerateObjects { asset, _, stop in
            do {
                try write(makeInventoryRecord(asset))
                assetCount += 1
            } catch {
                writeError = error
                stop.pointee = true
            }
        }
        if let writeError { throw writeError }
        for identifier in removed.sorted() {
            try write(InventoryRemovalRecord(localIdentifier: identifier))
        }
        try write(InventoryChangeBoundaryRecord(
            recordType: "changes_end", checkpointBefore: nil, checkpointAfter: snapshot,
            assetCount: assetCount, removedCount: removed.count
        ))
        try outputHandle.synchronize()
        if let temporaryURL, let finalURL {
            try? FileManager.default.removeItem(at: finalURL)
            try FileManager.default.moveItem(at: temporaryURL, to: finalURL)
            print("Wrote \(assetCount) changed and \(removed.count) removed assets to \(finalURL.path)")
        }
    }
}
