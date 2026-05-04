import Foundation
import Photos

/// Estimates the total download size for a set of assets.
/// Uses pixelWidth × pixelHeight × 3 / 8 as a rough JPEG approximation.
public func estimatedBytes(for assets: [PHAsset]) -> Int64 {
    assets.reduce(0) { acc, asset in
        acc + Int64(asset.pixelWidth) * Int64(asset.pixelHeight) * 3 / 8
    }
}

/// Returns the number of free bytes available at the given URL, or nil on error.
public func freeBytes(at url: URL) -> Int64? {
    let values = try? url.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
    if let cap = values?.volumeAvailableCapacityForImportantUsage {
        return cap
    }
    // Fallback: statvfs
    var st = statvfs()
    guard statvfs(url.path, &st) == 0 else { return nil }
    return Int64(st.f_bavail) * Int64(st.f_frsize)
}

public struct DiskWarning {
    public let estimatedBytes: Int64
    public let freeBytes: Int64
    public let isSystemVolume: Bool
}

/// Returns a warning if space may be insufficient, or if Photos Library is on the system volume.
/// Returns nil if everything looks fine.
/// `estimatedBytesOverride` / `freeBytesOverride` are for testing only.
public func checkDiskSpace(
    stagingDir: URL,
    assets: [PHAsset],
    photosLibraryPath: String? = nil,
    estimatedBytesOverride: Int64? = nil,
    freeBytesOverride: Int64? = nil
) -> DiskWarning? {
    let estimated = estimatedBytesOverride ?? estimatedBytes(for: assets)
    let free      = freeBytesOverride      ?? freeBytes(at: stagingDir) ?? Int64.max

    let onSystemVolume: Bool
    if let lib = photosLibraryPath {
        onSystemVolume = lib.hasPrefix("/System") || lib == "/"
    } else {
        onSystemVolume = false
    }

    if Double(estimated) > Double(free) * 0.8 || onSystemVolume {
        return DiskWarning(
            estimatedBytes: estimated,
            freeBytes: free,
            isSystemVolume: onSystemVolume
        )
    }
    return nil
}

public func formatBytes(_ bytes: Int64) -> String {
    let gb = Double(bytes) / 1_073_741_824
    if gb >= 1 { return String(format: "%.1f GB", gb) }
    let mb = Double(bytes) / 1_048_576
    return String(format: "%.0f MB", mb)
}
