import Photos

@available(macOS 13, *)
public func loadApplePhotoInventory() -> ApplePhotoInventory {
    let options = PHFetchOptions()
    options.includeAssetSourceTypes = [.typeUserLibrary, .typeCloudShared]
    return ApplePhotoInventory(
        imageCount: PHAsset.fetchAssets(with: .image, options: options).count,
        videoCount: PHAsset.fetchAssets(with: .video, options: options).count
    )
}
