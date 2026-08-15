import Foundation
import PhotoBridgeLib
import Photos

@available(macOS 13, *)
func serializeChangeToken(_ token: PHPersistentChangeToken) -> Data? {
    try? NSKeyedArchiver.archivedData(withRootObject: token, requiringSecureCoding: true)
}

@available(macOS 13, *)
func deserializeChangeToken(_ data: Data) -> PHPersistentChangeToken? {
    try? NSKeyedUnarchiver.unarchivedObject(ofClass: PHPersistentChangeToken.self, from: data)
}

@available(macOS 13, *)
func makeInventoryRecord(_ asset: PHAsset) -> InventoryAssetRecord {
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

private func photoResourceTypeName(_ type: PHAssetResourceType) -> String {
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
