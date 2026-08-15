import Foundation

public enum PhotoPresentationMode: String, Codable, CaseIterable, Sendable {
    case embedded
    case systemBrowser
}

public struct MacAppConfiguration: Codable, Equatable, Sendable {
    public var libraryPath: String
    public var libraryBookmark: Data?
    public var serviceExecutablePath: String?
    public var host: String
    public var port: UInt16
    public var presentationMode: PhotoPresentationMode
    public var launchAtLogin: Bool

    public init(
        libraryPath: String,
        libraryBookmark: Data? = nil,
        serviceExecutablePath: String? = nil,
        host: String = "127.0.0.1",
        port: UInt16 = 8080,
        presentationMode: PhotoPresentationMode = .embedded,
        launchAtLogin: Bool = false
    ) {
        self.libraryPath = libraryPath
        self.libraryBookmark = libraryBookmark
        self.serviceExecutablePath = serviceExecutablePath
        self.host = host
        self.port = port
        self.presentationMode = presentationMode
        self.launchAtLogin = launchAtLogin
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
