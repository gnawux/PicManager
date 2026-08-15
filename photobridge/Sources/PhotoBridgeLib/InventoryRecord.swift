import Foundation

public struct InventoryResourceRecord: Encodable, Sendable, Equatable {
    public let type: String
    public let originalFilename: String
    public let uniformTypeIdentifier: String

    public init(type: String, originalFilename: String, uniformTypeIdentifier: String) {
        self.type = type
        self.originalFilename = originalFilename
        self.uniformTypeIdentifier = uniformTypeIdentifier
    }
}

public struct InventoryAssetRecord: Encodable, Sendable, Equatable {
    public let recordType = "asset"
    public let schemaVersion = 1
    public let localIdentifier: String
    public let originalFilename: String?
    public let mediaType: String
    public let pixelWidth: Int
    public let pixelHeight: Int
    public let creationDate: Date?
    public let modificationDate: Date?
    public let favorite: Bool
    public let hidden: Bool
    public let livePhoto: Bool
    public let adjusted: Bool
    public let burstIdentifier: String?
    public let excludedReason: String?
    public let resources: [InventoryResourceRecord]

    public init(
        localIdentifier: String,
        originalFilename: String?,
        mediaType: String,
        pixelWidth: Int,
        pixelHeight: Int,
        creationDate: Date?,
        modificationDate: Date?,
        favorite: Bool,
        hidden: Bool,
        livePhoto: Bool,
        adjusted: Bool,
        burstIdentifier: String?,
        excludedReason: String?,
        resources: [InventoryResourceRecord]
    ) {
        self.localIdentifier = localIdentifier
        self.originalFilename = originalFilename
        self.mediaType = mediaType
        self.pixelWidth = pixelWidth
        self.pixelHeight = pixelHeight
        self.creationDate = creationDate
        self.modificationDate = modificationDate
        self.favorite = favorite
        self.hidden = hidden
        self.livePhoto = livePhoto
        self.adjusted = adjusted
        self.burstIdentifier = burstIdentifier
        self.excludedReason = excludedReason
        self.resources = resources
    }
}

public struct InventoryBoundaryRecord: Encodable, Sendable, Equatable {
    public let recordType: String
    public let schemaVersion = 1
    public let checkpoint: Data?
    public let assetCount: Int?

    public init(recordType: String, checkpoint: Data?, assetCount: Int? = nil) {
        self.recordType = recordType
        self.checkpoint = checkpoint
        self.assetCount = assetCount
    }
}

public func inventoryEncoder() -> JSONEncoder {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    encoder.keyEncodingStrategy = .convertToSnakeCase
    return encoder
}
