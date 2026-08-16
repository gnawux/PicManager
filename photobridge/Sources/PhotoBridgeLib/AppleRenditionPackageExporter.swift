import CryptoKit
import Foundation
import ImageIO
import Photos

public enum AppleRenditionPackageError: Error, LocalizedError {
    case assetNotFound(String)
    case noOriginalResource
    case currentRenditionUnavailable
    case incompleteExistingPackage

    public var errorDescription: String? {
        switch self {
        case let .assetNotFound(identifier): "Apple Photos could not find \(identifier)."
        case .noOriginalResource: "Apple Photos has no exportable photo resource."
        case .currentRenditionUnavailable: "Apple Photos could not produce the current rendition."
        case .incompleteExistingPackage: "The previous export package is incomplete."
        }
    }
}

/// Exports a recoverable package without modifying either the Photos library or PicManager's
/// catalog. The Rust service verifies and commits this package atomically afterwards.
@available(macOS 13, *)
public func exportAppleRenditionPackage(identifier: String, destination: URL) async throws {
    // Keep the native path on the same full-access contract as `photobridge
    // export-asset`. Inventory metadata can be read with limited access, while
    // original-resource export cannot reliably do so.
    let authorization = try await requestPhotoLibraryAccess()
    if case .limited = authorization { throw AuthError.limited }
    let manifestURL = destination.appendingPathComponent("manifest.json")
    if FileManager.default.fileExists(atPath: destination.path) {
        guard FileManager.default.fileExists(atPath: manifestURL.path) else {
            throw AppleRenditionPackageError.incompleteExistingPackage
        }
        return
    }
    // Make the managed staging root observable before contacting PhotoKit. This
    // distinguishes filesystem access failures from cloud/resource failures.
    let parent = destination.deletingLastPathComponent()
    try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
    let fetch = PHAsset.fetchAssets(withLocalIdentifiers: [identifier], options: nil)
    guard let asset = fetch.firstObject else { throw AppleRenditionPackageError.assetNotFound(identifier) }
    let resources = PHAssetResource.assetResources(for: asset)
    guard let original = resources.first(where: { $0.type == .photo }) else {
        throw AppleRenditionPackageError.noOriginalResource
    }
    let adjusted = resources.contains { $0.type == .adjustmentData || $0.type == .fullSizePhoto }
    let temporary = parent.appendingPathComponent(".\(destination.lastPathComponent).partial-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: temporary, withIntermediateDirectories: true)
    do {
        let paths = renditionPackagePaths(originalFilename: original.originalFilename)
        let originalURL = temporary.appendingPathComponent(paths.originalRelativePath)
        try FileManager.default.createDirectory(at: originalURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try await writeAssetResource(original, to: originalURL)
        let originalProperties = packageImageProperties(originalURL)
        let originalManifest = try packageFileManifest(
            url: originalURL, relativePath: paths.originalRelativePath, role: "original",
            originalFilename: original.originalFilename, uti: original.uniformTypeIdentifier,
            provenance: "photokit_resource", bytePreserved: true,
            orientation: originalProperties.orientation, width: originalProperties.width,
            height: originalProperties.height, colorSpace: originalProperties.colorSpace, generationKey: nil
        )
        var currentManifest: RenditionFileManifest?
        if adjusted {
            let current = try await packageCurrentRendition(asset)
            let currentPaths = renditionPackagePaths(
                originalFilename: original.originalFilename, currentUniformTypeIdentifier: current.uti
            )
            let currentURL = temporary.appendingPathComponent(currentPaths.currentRelativePath)
            try FileManager.default.createDirectory(at: currentURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try current.data.write(to: currentURL, options: .atomic)
            let properties = packageImageProperties(currentURL)
            currentManifest = try packageFileManifest(
                url: currentURL, relativePath: currentPaths.currentRelativePath, role: "current",
                originalFilename: nil, uti: current.uti, provenance: "photokit_current", bytePreserved: false,
                orientation: current.orientation, width: properties.width, height: properties.height,
                colorSpace: properties.colorSpace,
                generationKey: asset.modificationDate.map { ISO8601DateFormatter().string(from: $0) }
            )
        }
        let manifest = RenditionPackageManifest(
            sourceIdentifier: identifier, originalFilename: original.originalFilename,
            adjusted: adjusted, original: originalManifest, current: currentManifest
        )
        try renditionManifestEncoder().encode(manifest)
            .write(to: temporary.appendingPathComponent("manifest.json"), options: .atomic)
        try FileManager.default.moveItem(at: temporary, to: destination)
    } catch {
        try? FileManager.default.removeItem(at: temporary)
        throw error
    }
}

@available(macOS 13, *)
private func packageCurrentRendition(_ asset: PHAsset) async throws -> (data: Data, uti: String, orientation: Int) {
    try await withCheckedThrowingContinuation { continuation in
        let options = PHImageRequestOptions()
        options.version = .current
        options.deliveryMode = .highQualityFormat
        options.isNetworkAccessAllowed = true
        PHImageManager.default().requestImageDataAndOrientation(for: asset, options: options) { data, uti, orientation, info in
            if let error = info?[PHImageErrorKey] as? Error { continuation.resume(throwing: error) }
            else if let data, let uti { continuation.resume(returning: (data, uti, Int(orientation.rawValue))) }
            else { continuation.resume(throwing: AppleRenditionPackageError.currentRenditionUnavailable) }
        }
    }
}

private func packageFileManifest(url: URL, relativePath: String, role: String, originalFilename: String?, uti: String, provenance: String, bytePreserved: Bool, orientation: Int?, width: Int, height: Int, colorSpace: String?, generationKey: String?) throws -> RenditionFileManifest {
    let data = try Data(contentsOf: url, options: .mappedIfSafe)
    let sha256 = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    return RenditionFileManifest(role: role, relativePath: relativePath, originalFilename: originalFilename,
        uniformTypeIdentifier: uti, mimeType: mimeType(forUTI: uti), sha256: sha256, byteSize: Int64(data.count),
        width: width, height: height, provenance: provenance, bytePreserved: bytePreserved,
        orientationMode: "metadata", sourceOrientation: orientation, displayOrientation: orientation,
        colorSpace: colorSpace, generationKey: generationKey)
}

private func packageImageProperties(_ url: URL) -> (width: Int, height: Int, orientation: Int?, colorSpace: String?) {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let values = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any] else { return (0, 0, nil, nil) }
    return (values[kCGImagePropertyPixelWidth as String] as? Int ?? 0,
            values[kCGImagePropertyPixelHeight as String] as? Int ?? 0,
            values[kCGImagePropertyOrientation as String] as? Int,
            values[kCGImagePropertyColorModel as String] as? String)
}
