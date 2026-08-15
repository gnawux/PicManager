import ArgumentParser
import Foundation
import PhotoBridgeLib
import Photos

@available(macOS 13, *)
struct InventoryCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "inventory",
        abstract: "Write a metadata-only inventory of the system Photos library"
    )

    @Option(name: .long, help: "NDJSON output path (stdout when omitted)")
    var output: String?

    func run() async throws {
        let authResult = try await requestPhotoLibraryAccess()
        if authResult == .limited {
            fputs("Warning: Photos access is limited; inventory is incomplete.\n", stderr)
        }

        let outputHandle: FileHandle
        var temporaryURL: URL?
        var finalURL: URL?
        if let output {
            finalURL = URL(fileURLWithPath: output)
            temporaryURL = URL(fileURLWithPath: output + ".partial")
            FileManager.default.createFile(atPath: temporaryURL!.path, contents: nil)
            outputHandle = try FileHandle(forWritingTo: temporaryURL!)
        } else {
            outputHandle = .standardOutput
        }
        defer { if output != nil { try? outputHandle.close() } }

        let encoder = inventoryEncoder()
        func write<T: Encodable>(_ record: T) throws {
            try outputHandle.write(contentsOf: encoder.encode(record))
            try outputHandle.write(contentsOf: Data([0x0a]))
        }

        let library = PHPhotoLibrary.shared()
        try write(InventoryBoundaryRecord(
            recordType: "inventory_header",
            checkpoint: serializeChangeToken(library.currentChangeToken)
        ))

        let options = PHFetchOptions()
        options.includeAssetSourceTypes = [.typeUserLibrary, .typeCloudShared]
        let result = PHAsset.fetchAssets(with: .image, options: options)
        var count = 0
        var writeError: Error?
        result.enumerateObjects { asset, _, stop in
            do {
                try write(makeInventoryRecord(asset))
                count += 1
            } catch {
                writeError = error
                stop.pointee = true
            }
        }
        if let writeError { throw writeError }
        try write(InventoryBoundaryRecord(
            recordType: "inventory_end",
            checkpoint: serializeChangeToken(library.currentChangeToken),
            assetCount: count
        ))
        try outputHandle.synchronize()

        if let temporaryURL, let finalURL {
            try? FileManager.default.removeItem(at: finalURL)
            try FileManager.default.moveItem(at: temporaryURL, to: finalURL)
            print("Wrote \(count) Photos assets to \(finalURL.path)")
        }
    }

}
