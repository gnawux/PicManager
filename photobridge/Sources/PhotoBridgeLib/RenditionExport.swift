import Foundation

public struct RenditionFileManifest: Codable, Sendable, Equatable {
    public let role: String
    public let relativePath: String
    public let originalFilename: String?
    public let uniformTypeIdentifier: String
    public let mimeType: String
    public let sha256: String
    public let byteSize: Int64
    public let width: Int
    public let height: Int
    public let provenance: String
    public let bytePreserved: Bool
    public let orientationMode: String
    public let sourceOrientation: Int?
    public let displayOrientation: Int?
    public let colorSpace: String?
    public let generationKey: String?

    public init(
        role: String, relativePath: String, originalFilename: String?,
        uniformTypeIdentifier: String, mimeType: String, sha256: String,
        byteSize: Int64, width: Int, height: Int, provenance: String,
        bytePreserved: Bool, orientationMode: String, sourceOrientation: Int?,
        displayOrientation: Int?, colorSpace: String?, generationKey: String?
    ) {
        self.role = role
        self.relativePath = relativePath
        self.originalFilename = originalFilename
        self.uniformTypeIdentifier = uniformTypeIdentifier
        self.mimeType = mimeType
        self.sha256 = sha256
        self.byteSize = byteSize
        self.width = width
        self.height = height
        self.provenance = provenance
        self.bytePreserved = bytePreserved
        self.orientationMode = orientationMode
        self.sourceOrientation = sourceOrientation
        self.displayOrientation = displayOrientation
        self.colorSpace = colorSpace
        self.generationKey = generationKey
    }
}

public struct RenditionPackageManifest: Codable, Sendable, Equatable {
    public let schemaVersion: Int
    public let sourceIdentifier: String
    public let originalFilename: String
    public let adjusted: Bool
    public let original: RenditionFileManifest
    public let current: RenditionFileManifest?

    public init(
        schemaVersion: Int = 1, sourceIdentifier: String, originalFilename: String,
        adjusted: Bool, original: RenditionFileManifest, current: RenditionFileManifest?
    ) {
        self.schemaVersion = schemaVersion
        self.sourceIdentifier = sourceIdentifier
        self.originalFilename = originalFilename
        self.adjusted = adjusted
        self.original = original
        self.current = current
    }
}

public struct RenditionPackagePaths: Sendable, Equatable {
    public let originalRelativePath: String
    public let currentRelativePath: String
    public let manifestRelativePath: String
}

public func renditionPackagePaths(
    originalFilename: String,
    currentUniformTypeIdentifier: String = "public.jpeg"
) -> RenditionPackagePaths {
    let safeOriginal = URL(fileURLWithPath: originalFilename).lastPathComponent
    let fallback = safeOriginal.isEmpty ? "original.data" : safeOriginal
    return RenditionPackagePaths(
        originalRelativePath: "original/\(fallback)",
        currentRelativePath: "current/current.\(fileExtension(forUTI: currentUniformTypeIdentifier))",
        manifestRelativePath: "manifest.json"
    )
}

public func mimeType(forUTI uti: String) -> String {
    switch uti {
    case "public.jpeg", "public.jpg": return "image/jpeg"
    case "public.heic": return "image/heic"
    case "public.heif": return "image/heif"
    case "public.png": return "image/png"
    case "public.tiff", "public.tif": return "image/tiff"
    case "public.gif": return "image/gif"
    default: return "application/octet-stream"
    }
}

public func renditionManifestEncoder() -> JSONEncoder {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    encoder.keyEncodingStrategy = .convertToSnakeCase
    return encoder
}
