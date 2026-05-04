import Photos
import Foundation

// MARK: - Pure helpers (testable without Photos runtime)

/// Maps a UTI string to a file extension.
public func fileExtension(forUTI uti: String) -> String {
    switch uti {
    case "public.jpeg", "public.jpg":    return "jpg"
    case "public.heic":                  return "heic"
    case "public.heif":                  return "heif"
    case "public.png":                   return "png"
    case "public.tiff", "public.tif":    return "tiff"
    case "public.webp":                  return "webp"
    case "public.gif":                   return "gif"
    default:                             return "data"
    }
}

/// Builds the destination URL for an exported asset.
/// Slashes in localIdentifier are replaced with underscores to produce a flat filename.
public func exportDestinationURL(
    stagingDir: URL,
    localIdentifier: String,
    uti: String
) -> URL {
    let safeName = localIdentifier.replacingOccurrences(of: "/", with: "_")
    let ext = fileExtension(forUTI: uti)
    return stagingDir.appendingPathComponent("\(safeName).\(ext)")
}

// MARK: - Exported asset record

public struct ExportedAsset: Sendable {
    public let localIdentifier: String
    public let fileURL: URL
    public let takenAt: Date?
}

// MARK: - Exporter (requires Photos runtime)

/// Writes a single PHAssetResource to destURL, downloading from iCloud if needed.
@available(macOS 13, *)
public func writeAssetResource(
    _ resource: PHAssetResource,
    to destURL: URL
) async throws {
    let options = PHAssetResourceRequestOptions()
    options.isNetworkAccessAllowed = true

    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        PHAssetResourceManager.default().writeData(
            for: resource,
            toFile: destURL,
            options: options
        ) { error in
            if let error {
                continuation.resume(throwing: error)
            } else {
                continuation.resume()
            }
        }
    }
}
