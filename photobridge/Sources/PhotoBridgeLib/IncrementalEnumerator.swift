import Photos
import Foundation

@available(macOS 13, *)
public struct IncrementalEnumerator {

    public static var defaultStateURL: URL {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!
        return support.appendingPathComponent("PhotoBridge/state.json")
    }

    public init() {}

    /// Enumerates assets to export since the last sync.
    /// - Parameters:
    ///   - token: Serialized `PHPersistentChangeToken` from a previous sync, or nil for a full scan.
    ///   - progress: Called with (processed, total) during enumeration.
    /// - Returns: (pairs to export, new serialized token to persist)
    public func enumerate(
        token: Data?,
        progress: @escaping @Sendable (Int, Int) -> Void = { _, _ in }
    ) -> (pairs: [(PHAsset, PHAssetResource)], newToken: Data?) {
        guard let tokenData = token,
              let changeToken = deserializeToken(tokenData) else {
            // No prior token: full enumeration
            let pairs = LibraryEnumerator().enumerate(progress: progress)
            let newToken = serializeCurrentToken()
            return (pairs, newToken)
        }

        // Collect changed asset IDs from all change batches since stored token
        var assetIDs: [String] = []
        var latestTokenData: Data? = nil
        do {
            let result = try PHPhotoLibrary.shared().fetchPersistentChanges(since: changeToken)
            for change in result {
                let det = try change.changeDetails(for: PHObjectType.asset)
                assetIDs.append(contentsOf: det.insertedLocalIdentifiers)
                assetIDs.append(contentsOf: det.updatedLocalIdentifiers)
            }
            // Snapshot the current token after processing all changes
            latestTokenData = serializeCurrentToken()
        } catch {
            // History expired or unavailable — fall back to full enumeration
            let pairs = LibraryEnumerator().enumerate(progress: progress)
            let newToken = serializeCurrentToken()
            return (pairs, newToken)
        }

        guard !assetIDs.isEmpty else {
            return ([], latestTokenData)
        }

        // Fetch PHAssets for changed IDs
        let fetchResult = PHAsset.fetchAssets(
            withLocalIdentifiers: Array(Set(assetIDs)),
            options: nil
        )

        let total = fetchResult.count
        var pairs: [(PHAsset, PHAssetResource)] = []
        fetchResult.enumerateObjects { asset, idx, _ in
            progress(idx + 1, total)
            guard asset.mediaType == .image else { return }
            let resources = PHAssetResource.assetResources(for: asset)
            let isBurst = asset.representsBurst
            let isUserPick = asset.burstSelectionTypes.contains(.userPick)
            if let resource = selectExportResource(
                from: resources, isBurst: isBurst, isUserPick: isUserPick
            ), let concrete = resource as? PHAssetResource {
                pairs.append((asset, concrete))
            }
        }

        return (pairs, latestTokenData)
    }

    // MARK: - Token serialization helpers (public for testing)

    public func serializeToken(_ token: PHPersistentChangeToken) -> Data? {
        try? NSKeyedArchiver.archivedData(withRootObject: token, requiringSecureCoding: true)
    }

    public func deserializeToken(_ data: Data) -> PHPersistentChangeToken? {
        try? NSKeyedUnarchiver.unarchivedObject(ofClass: PHPersistentChangeToken.self, from: data)
    }

    // MARK: - Private

    private func serializeCurrentToken() -> Data? {
        serializeToken(PHPhotoLibrary.shared().currentChangeToken)
    }
}
