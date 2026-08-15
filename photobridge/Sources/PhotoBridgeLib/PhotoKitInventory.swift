import Foundation
import Photos

@available(macOS 13, *)
public func serializeChangeToken(_ token: PHPersistentChangeToken) -> Data? {
    try? NSKeyedArchiver.archivedData(withRootObject: token, requiringSecureCoding: true)
}

@available(macOS 13, *)
public func deserializeChangeToken(_ data: Data) -> PHPersistentChangeToken? {
    try? NSKeyedUnarchiver.unarchivedObject(ofClass: PHPersistentChangeToken.self, from: data)
}

@available(macOS 13, *)
public func makeInventoryRecord(_ asset: PHAsset) -> InventoryAssetRecord {
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
                type: photoResourceTypeName($0.type),
                originalFilename: $0.originalFilename,
                uniformTypeIdentifier: $0.uniformTypeIdentifier
            )
        }
    )
}

@available(macOS 13, *)
public func writeApplePhotoInventory(
    to finalURL: URL,
    progress: @escaping @Sendable (Int, Int) -> Void = { _, _ in }
) throws -> Int {
    let temporaryURL = finalURL.appendingPathExtension("partial")
    try FileManager.default.createDirectory(at: finalURL.deletingLastPathComponent(), withIntermediateDirectories: true)
    try? FileManager.default.removeItem(at: temporaryURL)
    FileManager.default.createFile(atPath: temporaryURL.path, contents: nil)
    let output = try FileHandle(forWritingTo: temporaryURL)
    defer { try? output.close() }
    let encoder = inventoryEncoder()
    func write<T: Encodable>(_ record: T) throws {
        try output.write(contentsOf: encoder.encode(record))
        try output.write(contentsOf: Data([0x0a]))
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
            progress(count, result.count)
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
    try output.synchronize()
    try? FileManager.default.removeItem(at: finalURL)
    try FileManager.default.moveItem(at: temporaryURL, to: finalURL)
    return count
}

private func photoResourceTypeName(_ type: PHAssetResourceType) -> String {
    switch type {
    case .photo: "photo"
    case .video: "video"
    case .audio: "audio"
    case .alternatePhoto: "alternate_photo"
    case .fullSizePhoto: "full_size_photo"
    case .fullSizeVideo: "full_size_video"
    case .adjustmentData: "adjustment_data"
    case .adjustmentBasePhoto: "adjustment_base_photo"
    case .adjustmentBaseVideo: "adjustment_base_video"
    case .adjustmentBasePairedVideo: "adjustment_base_paired_video"
    case .pairedVideo: "paired_video"
    case .fullSizePairedVideo: "full_size_paired_video"
    case .photoProxy: "photo_proxy"
    @unknown default: "unknown_\(type.rawValue)"
    }
}
