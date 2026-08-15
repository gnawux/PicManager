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
            checkpoint: serializeToken(library.currentChangeToken)
        ))

        let options = PHFetchOptions()
        options.includeAssetSourceTypes = [.typeUserLibrary, .typeCloudShared]
        let result = PHAsset.fetchAssets(with: .image, options: options)
        var count = 0
        var writeError: Error?
        result.enumerateObjects { asset, _, stop in
            do {
                try write(makeRecord(asset))
                count += 1
            } catch {
                writeError = error
                stop.pointee = true
            }
        }
        if let writeError { throw writeError }
        try write(InventoryBoundaryRecord(
            recordType: "inventory_end",
            checkpoint: serializeToken(library.currentChangeToken),
            assetCount: count
        ))
        try outputHandle.synchronize()

        if let temporaryURL, let finalURL {
            try? FileManager.default.removeItem(at: finalURL)
            try FileManager.default.moveItem(at: temporaryURL, to: finalURL)
            print("Wrote \(count) Photos assets to \(finalURL.path)")
        }
    }

    private func serializeToken(_ token: PHPersistentChangeToken) -> Data? {
        try? NSKeyedArchiver.archivedData(withRootObject: token, requiringSecureCoding: true)
    }

    private func makeRecord(_ asset: PHAsset) -> InventoryAssetRecord {
        let resources = PHAssetResource.assetResources(for: asset)
        let photo = resources.first { $0.type == .photo }
        let raw = resources.first { $0.type == .alternatePhoto }
        let isUnselectedBurst = asset.representsBurst
            && !asset.burstSelectionTypes.contains(.userPick)
        let excludedReason: String?
        if isUnselectedBurst { excludedReason = "unselected_burst" }
        else if photo == nil && raw != nil { excludedReason = "raw_only" }
        else if photo == nil { excludedReason = "unsupported_resource" }
        else { excludedReason = nil }

        return InventoryAssetRecord(
            localIdentifier: asset.localIdentifier,
            originalFilename: (photo ?? raw ?? resources.first)?.originalFilename,
            mediaType: "image",
            pixelWidth: asset.pixelWidth,
            pixelHeight: asset.pixelHeight,
            creationDate: asset.creationDate,
            modificationDate: asset.modificationDate,
            favorite: asset.isFavorite,
            hidden: asset.isHidden,
            livePhoto: asset.mediaSubtypes.contains(.photoLive),
            adjusted: resources.contains { $0.type == .adjustmentData || $0.type == .fullSizePhoto },
            burstIdentifier: asset.burstIdentifier,
            excludedReason: excludedReason,
            resources: resources.map {
                InventoryResourceRecord(
                    type: resourceTypeName($0.type),
                    originalFilename: $0.originalFilename,
                    uniformTypeIdentifier: $0.uniformTypeIdentifier
                )
            }
        )
    }

    private func resourceTypeName(_ type: PHAssetResourceType) -> String {
        switch type {
        case .photo: return "photo"
        case .video: return "video"
        case .audio: return "audio"
        case .alternatePhoto: return "alternate_photo"
        case .fullSizePhoto: return "full_size_photo"
        case .fullSizeVideo: return "full_size_video"
        case .adjustmentData: return "adjustment_data"
        case .adjustmentBasePhoto: return "adjustment_base_photo"
        case .adjustmentBaseVideo: return "adjustment_base_video"
        case .adjustmentBasePairedVideo: return "adjustment_base_paired_video"
        case .pairedVideo: return "paired_video"
        case .fullSizePairedVideo: return "full_size_paired_video"
        case .photoProxy: return "photo_proxy"
        @unknown default: return "unknown_\(type.rawValue)"
        }
    }
}
