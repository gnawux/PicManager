import Foundation

public enum PhotoPresentationMode: String, Codable, CaseIterable, Sendable {
    case embedded
    case systemBrowser
}

public enum ApplePhotosSyncPolicy: String, Codable, CaseIterable, Sendable {
    case disabled
    case inventoryOnly
    case importNew
}

public struct MacAppConfiguration: Codable, Equatable, Sendable {
    public var libraryPath: String
    public var libraryBookmark: Data?
    public var serviceExecutablePath: String?
    public var host: String
    public var port: UInt16
    public var presentationMode: PhotoPresentationMode
    public var launchAtLogin: Bool
    /// Metadata discovery is safe for existing libraries; downloading is always explicit.
    public var applePhotosSyncPolicy: ApplePhotosSyncPolicy
    public var garminEmail: String?

    public init(
        libraryPath: String,
        libraryBookmark: Data? = nil,
        serviceExecutablePath: String? = nil,
        host: String = "127.0.0.1",
        port: UInt16 = 8080,
        presentationMode: PhotoPresentationMode = .embedded,
        launchAtLogin: Bool = false,
        applePhotosSyncPolicy: ApplePhotosSyncPolicy = .inventoryOnly, garminEmail: String? = nil
    ) {
        self.libraryPath = libraryPath
        self.libraryBookmark = libraryBookmark
        self.serviceExecutablePath = serviceExecutablePath
        self.host = host
        self.port = port
        self.presentationMode = presentationMode
        self.launchAtLogin = launchAtLogin
        self.applePhotosSyncPolicy = applePhotosSyncPolicy
        self.garminEmail = garminEmail
    }

    private enum CodingKeys: String, CodingKey {
        case libraryPath, libraryBookmark, serviceExecutablePath, host, port
        case presentationMode, launchAtLogin, applePhotosSyncPolicy, garminEmail
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        libraryPath = try values.decode(String.self, forKey: .libraryPath)
        libraryBookmark = try values.decodeIfPresent(Data.self, forKey: .libraryBookmark)
        serviceExecutablePath = try values.decodeIfPresent(String.self, forKey: .serviceExecutablePath)
        host = try values.decodeIfPresent(String.self, forKey: .host) ?? "127.0.0.1"
        port = try values.decodeIfPresent(UInt16.self, forKey: .port) ?? 8080
        presentationMode = try values.decodeIfPresent(PhotoPresentationMode.self, forKey: .presentationMode) ?? .embedded
        launchAtLogin = try values.decodeIfPresent(Bool.self, forKey: .launchAtLogin) ?? false
        applePhotosSyncPolicy = try values.decodeIfPresent(ApplePhotosSyncPolicy.self, forKey: .applePhotosSyncPolicy) ?? .inventoryOnly
        garminEmail = try values.decodeIfPresent(String.self, forKey: .garminEmail)
    }

    public static var `default`: MacAppConfiguration {
        let pictures = FileManager.default.urls(for: .picturesDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Pictures")
        return MacAppConfiguration(
            libraryPath: pictures.appendingPathComponent("PicManager").path
        )
    }

    public var serviceURL: URL? {
        URL(string: "http://\(host):\(port)")
    }

    public static func load(from url: URL) throws -> MacAppConfiguration {
        try JSONDecoder().decode(MacAppConfiguration.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL) throws {
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        let data = try JSONEncoder.sorted.encode(self)
        try data.write(to: url, options: .atomic)
    }

    public static var applicationSupportURL: URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("PicManager/config.json")
    }
}

private extension JSONEncoder {
    static var sorted: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return encoder
    }
}
