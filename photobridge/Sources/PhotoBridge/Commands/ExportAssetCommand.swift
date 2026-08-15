import ArgumentParser
import CryptoKit
import Foundation
import ImageIO
import PhotoBridgeLib
import Photos

enum ExportAssetError: Error, LocalizedError {
    case assetNotFound(String)
    case noOriginalResource
    case currentRenditionUnavailable
    case incompleteExistingPackage

    var errorDescription: String? {
        switch self {
        case .assetNotFound(let identifier): return "Photos asset not found: \(identifier)"
        case .noOriginalResource: return "The asset has no finished photo resource."
        case .currentRenditionUnavailable: return "Photos could not produce the current rendition."
        case .incompleteExistingPackage: return "The destination exists without a complete manifest."
        }
    }
}

@available(macOS 13, *)
struct ExportAssetCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "export-asset",
        abstract: "Export an immutable original and optional current Photos rendition"
    )

    @Option(name: .long) var identifier: String
    @Option(name: .long, help: "Final rendition package directory") var output: String

    func run() async throws {
        let authorization = try await requestPhotoLibraryAccess()
        if case .limited = authorization { throw AuthError.limited }
        let destination = URL(fileURLWithPath: output)
        let existingManifest = destination.appendingPathComponent("manifest.json")
        if FileManager.default.fileExists(atPath: destination.path) {
            guard FileManager.default.fileExists(atPath: existingManifest.path) else {
                throw ExportAssetError.incompleteExistingPackage
            }
            print(existingManifest.path)
            return
        }

        let fetch = PHAsset.fetchAssets(withLocalIdentifiers: [identifier], options: nil)
        guard let asset = fetch.firstObject else { throw ExportAssetError.assetNotFound(identifier) }
        let resources = PHAssetResource.assetResources(for: asset)
        guard let originalResource = resources.first(where: { $0.type == .photo }) else {
            throw ExportAssetError.noOriginalResource
        }
        let adjusted = resources.contains { $0.type == .adjustmentData || $0.type == .fullSizePhoto }
        let parent = destination.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        let temporary = parent.appendingPathComponent(".\(destination.lastPathComponent).partial-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temporary, withIntermediateDirectories: true)
        do {
            let initialPaths = renditionPackagePaths(originalFilename: originalResource.originalFilename)
            let originalURL = temporary.appendingPathComponent(initialPaths.originalRelativePath)
            try FileManager.default.createDirectory(
                at: originalURL.deletingLastPathComponent(), withIntermediateDirectories: true
            )
            try await writeAssetResource(originalResource, to: originalURL)
            let originalProperties = imageProperties(originalURL)
            let original = try fileManifest(
                url: originalURL,
                relativePath: initialPaths.originalRelativePath,
                role: "original",
                originalFilename: originalResource.originalFilename,
                uti: originalResource.uniformTypeIdentifier,
                provenance: "photokit_resource",
                bytePreserved: true,
                orientation: originalProperties.orientation,
                width: originalProperties.width,
                height: originalProperties.height,
                colorSpace: originalProperties.colorSpace,
                generationKey: nil
            )

            var current: RenditionFileManifest?
            if adjusted {
                let rendition = try await requestCurrentRendition(asset)
                let paths = renditionPackagePaths(
                    originalFilename: originalResource.originalFilename,
                    currentUniformTypeIdentifier: rendition.uti
                )
                let currentURL = temporary.appendingPathComponent(paths.currentRelativePath)
                try FileManager.default.createDirectory(
                    at: currentURL.deletingLastPathComponent(), withIntermediateDirectories: true
                )
                try rendition.data.write(to: currentURL, options: .atomic)
                let properties = imageProperties(currentURL)
                current = try fileManifest(
                    url: currentURL,
                    relativePath: paths.currentRelativePath,
                    role: "current",
                    originalFilename: nil,
                    uti: rendition.uti,
                    provenance: "photokit_current",
                    bytePreserved: false,
                    orientation: rendition.orientation,
                    width: properties.width,
                    height: properties.height,
                    colorSpace: properties.colorSpace,
                    generationKey: asset.modificationDate.map(iso8601)
                )
            }
            let manifest = RenditionPackageManifest(
                sourceIdentifier: identifier,
                originalFilename: originalResource.originalFilename,
                adjusted: adjusted,
                original: original,
                current: current
            )
            let manifestURL = temporary.appendingPathComponent("manifest.json")
            try renditionManifestEncoder().encode(manifest).write(to: manifestURL, options: .atomic)
            try FileManager.default.moveItem(at: temporary, to: destination)
            print(destination.appendingPathComponent("manifest.json").path)
        } catch {
            try? FileManager.default.removeItem(at: temporary)
            throw error
        }
    }
}

@available(macOS 13, *)
private func requestCurrentRendition(_ asset: PHAsset) async throws -> (data: Data, uti: String, orientation: Int) {
    try await withCheckedThrowingContinuation { continuation in
        let options = PHImageRequestOptions()
        options.version = .current
        options.deliveryMode = .highQualityFormat
        options.isNetworkAccessAllowed = true
        PHImageManager.default().requestImageDataAndOrientation(for: asset, options: options) {
            data, uti, orientation, info in
            if let error = info?[PHImageErrorKey] as? Error {
                continuation.resume(throwing: error)
            } else if let data, let uti {
                continuation.resume(returning: (data, uti, Int(orientation.rawValue)))
            } else {
                continuation.resume(throwing: ExportAssetError.currentRenditionUnavailable)
            }
        }
    }
}

private func fileManifest(
    url: URL, relativePath: String, role: String, originalFilename: String?, uti: String,
    provenance: String, bytePreserved: Bool, orientation: Int?, width: Int, height: Int,
    colorSpace: String?, generationKey: String?
) throws -> RenditionFileManifest {
    let data = try Data(contentsOf: url, options: .mappedIfSafe)
    let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    return RenditionFileManifest(
        role: role, relativePath: relativePath, originalFilename: originalFilename,
        uniformTypeIdentifier: uti, mimeType: mimeType(forUTI: uti), sha256: digest,
        byteSize: Int64(data.count), width: width, height: height, provenance: provenance,
        bytePreserved: bytePreserved, orientationMode: "metadata",
        sourceOrientation: orientation, displayOrientation: orientation,
        colorSpace: colorSpace, generationKey: generationKey
    )
}

private func imageProperties(_ url: URL) -> (width: Int, height: Int, orientation: Int?, colorSpace: String?) {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let value = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
    else { return (0, 0, nil, nil) }
    return (
        value[kCGImagePropertyPixelWidth as String] as? Int ?? 0,
        value[kCGImagePropertyPixelHeight as String] as? Int ?? 0,
        value[kCGImagePropertyOrientation as String] as? Int,
        value[kCGImagePropertyColorModel as String] as? String
    )
}

private func iso8601(_ date: Date) -> String {
    ISO8601DateFormatter().string(from: date)
}
