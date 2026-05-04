import Photos

// Protocol over PHAssetResource so tests can inject fakes without the Photos runtime.
public protocol AssetResourceInfo: Sendable {
    var type: PHAssetResourceType { get }
    var uniformTypeIdentifier: String { get }
}

extension PHAssetResource: AssetResourceInfo {}

/// Selects the single resource to export for a given asset.
///
/// Rules:
/// - Burst frames that are not user-selected (userPick) are skipped → nil.
/// - For Live Photos the `.pairedVideo` resource is skipped; only `.photo` is returned.
/// - For RAW+JPEG pairs the `.alternatePhoto` (RAW) is skipped; only `.photo` is returned.
/// - A RAW-only asset (no `.photo` resource) is skipped → nil.
public func selectExportResource(
    from resources: [any AssetResourceInfo],
    isBurst: Bool,
    isUserPick: Bool
) -> (any AssetResourceInfo)? {
    // Burst frames that aren't the user's pick are skipped entirely.
    if isBurst && !isUserPick { return nil }

    // Pick the first .photo resource (JPEG or HEIC); skip RAW-only assets.
    return resources.first { $0.type == .photo }
}
