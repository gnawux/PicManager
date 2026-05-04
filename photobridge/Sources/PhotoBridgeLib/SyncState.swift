import Foundation

public struct SyncState: Codable, Sendable {
    public var lastSyncToken: Data?
    public var lastSyncDate: Date?
    public var exportedCount: Int

    public init(lastSyncToken: Data? = nil, lastSyncDate: Date? = nil, exportedCount: Int = 0) {
        self.lastSyncToken = lastSyncToken
        self.lastSyncDate = lastSyncDate
        self.exportedCount = exportedCount
    }

    public static func load(from url: URL) -> SyncState {
        guard let data = try? Data(contentsOf: url),
              let state = try? JSONDecoder().decode(SyncState.self, from: data) else {
            return SyncState()
        }
        return state
    }

    public mutating func save(to url: URL) throws {
        let dir = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let data = try JSONEncoder().encode(self)
        try data.write(to: url, options: .atomic)
    }
}
