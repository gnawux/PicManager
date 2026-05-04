import Photos
import Foundation

/// Enumerates all exportable image assets from the system Photos library.
@available(macOS 13, *)
public struct LibraryEnumerator {
    public init() {}

    /// Returns all (asset, resource) pairs eligible for export.
    /// - Progress callback receives (processed, total).
    public func enumerate(
        progress: @escaping @Sendable (Int, Int) -> Void = { _, _ in }
    ) -> [(PHAsset, PHAssetResource)] {
        let options = PHFetchOptions()
        options.includeAssetSourceTypes = [.typeUserLibrary, .typeCloudShared]
        let fetchResult = PHAsset.fetchAssets(with: .image, options: options)
        let total = fetchResult.count
        var results: [(PHAsset, PHAssetResource)] = []

        fetchResult.enumerateObjects { asset, idx, _ in
            progress(idx + 1, total)
            let resources = PHAssetResource.assetResources(for: asset)
            let isBurst = asset.representsBurst
            let isUserPick = asset.burstSelectionTypes.contains(.userPick)
            if let resource = selectExportResource(
                from: resources,
                isBurst: isBurst,
                isUserPick: isUserPick
            ) {
                // PHAssetResource conforms to AssetResourceInfo — safe to cast
                if let concrete = resource as? PHAssetResource {
                    results.append((asset, concrete))
                }
            }
        }
        return results
    }
}
