import Foundation

public struct ServiceContract: Decodable, Equatable, Sendable {
    public let apiVersion: String
    public let serviceVersion: String
    private let minimumClientApi: String
    public let localTrustedOnly: Bool
    public let libraryFingerprint: String?
    public let capabilities: [String]

    public var minimumClientAPI: String { minimumClientApi }

    public func servesLibrary(at path: String) -> Bool {
        libraryFingerprint == fingerprintLibraryPath(path)
    }
}

public struct ServiceHealth: Decodable, Equatable, Sendable {
    public let status: String
    public let checkedAt: String
    public let sqliteQuickCheck: String
    public let journalMode: String
    public let schemaVersion: Int
    public let applicationJobsQueued: Int
    public let applicationJobsRunning: Int
    public let applicationJobsFailed: Int
    public let syncJobsFailed: Int
}

public struct WorkerMetrics: Decodable, Equatable, Sendable {
    public let queued: Int
    public let running: Int
    public let retryWait: Int
    public let succeeded: Int
    public let failed: Int
    public let cancelled: Int
}

public struct ServiceTask: Decodable, Equatable, Sendable, Identifiable {
    public let id: Int
    public let kind: String
    public let provider: String?
    public let status: String
    public let totalItems: Int
    public let completedItems: Int
    public let failedItems: Int
    public let error: String?
    public let updatedAt: String
    public let source: String
    public let progressStage: String?

    public var canRetry: Bool { ["failed", "cancelled"].contains(status) }
    public var canCancel: Bool { ["queued", "leased", "running", "retry_wait"].contains(status) }
}

public struct ServiceTaskList: Decodable, Equatable, Sendable {
    public let tasks: [ServiceTask]
    private let nextBeforeId: Int?

    public var nextBeforeID: Int? { nextBeforeId }
}

public struct AppleSourceStatusPage: Decodable, Equatable, Sendable {
    public let statusCounts: [String: Int]
}

public struct AppleSource: Decodable, Equatable, Sendable, Identifiable {
    public let id: Int
    public let externalID: String
    public let originalFilename: String?
    public let syncStatus: String
}

public struct AppleSourcePage: Decodable, Equatable, Sendable {
    public let sources: [AppleSource]
    public let nextBeforeID: Int?
}

public struct AppleExportClaim: Decodable, Equatable, Sendable {
    public let itemID: Int
    public let source: AppleSource
}

public struct ApplePhotoInventory: Equatable, Sendable {
    public let imageCount: Int
    public let videoCount: Int

    public init(imageCount: Int, videoCount: Int) {
        self.imageCount = imageCount
        self.videoCount = videoCount
    }

    public var totalCount: Int { imageCount + videoCount }
}

public struct ServiceDashboard: Equatable, Sendable {
    public let contract: ServiceContract
    public let health: ServiceHealth
    public let metrics: WorkerMetrics
    public let tasks: [ServiceTask]
    public let sourceStatusCounts: [String: Int]

    public init(
        contract: ServiceContract,
        health: ServiceHealth,
        metrics: WorkerMetrics,
        tasks: [ServiceTask],
        sourceStatusCounts: [String: Int]
    ) {
        self.contract = contract
        self.health = health
        self.metrics = metrics
        self.tasks = tasks
        self.sourceStatusCounts = sourceStatusCounts
    }

    public var latestAppleSync: ServiceTask? {
        tasks.first { $0.provider == "apple_photos" }
    }

    public var synchronizedSourceCount: Int {
        sourceStatusCounts["synced", default: 0]
    }

    public var pendingSourceCount: Int {
        sourceStatusCounts
            .filter { !["synced", "excluded", "missing"].contains($0.key) }
            .reduce(0) { $0 + $1.value }
    }

    public var failedWorkCount: Int {
        metrics.failed + health.syncJobsFailed
    }
}

public enum ServiceDashboardError: LocalizedError, Equatable {
    case invalidBaseURL
    case incompatibleAPI(String)
    case response(Int)
    case libraryMismatch

    public var errorDescription: String? {
        switch self {
        case .invalidBaseURL:
            "The local service URL is invalid."
        case let .incompatibleAPI(version):
            "The local service API \(version) is not compatible with this app."
        case let .response(status):
            "The local service returned HTTP \(status)."
        case .libraryMismatch:
            "The configured port belongs to a different PicManager library."
        }
    }
}

public struct ServiceDashboardClient: Sendable {
    public let baseURL: URL
    private let session: URLSession
    private let expectedLibraryPath: String?

    public init(
        baseURL: URL,
        expectedLibraryPath: String? = nil,
        session: URLSession = .shared
    ) {
        self.baseURL = baseURL
        self.expectedLibraryPath = expectedLibraryPath
        self.session = session
    }

    public func load() async throws -> ServiceDashboard {
        async let contract: ServiceContract = get("api/v1/service")
        async let health: ServiceHealth = get("api/v1/health")
        async let metrics: WorkerMetrics = get("api/v1/metrics")
        async let tasks: ServiceTaskList = get("api/v1/tasks?limit=20")
        async let sources: AppleSourceStatusPage = get("api/apple/sources?limit=1")
        let snapshot = try await (contract, health, metrics, tasks, sources)
        guard snapshot.0.apiVersion == "v1", snapshot.0.minimumClientAPI == "v1" else {
            throw ServiceDashboardError.incompatibleAPI(snapshot.0.apiVersion)
        }
        if let expectedLibraryPath,
           !snapshot.0.servesLibrary(at: expectedLibraryPath) {
            throw ServiceDashboardError.libraryMismatch
        }
        return ServiceDashboard(
            contract: snapshot.0,
            health: snapshot.1,
            metrics: snapshot.2,
            tasks: snapshot.3.tasks,
            sourceStatusCounts: snapshot.4.statusCounts
        )
    }

    public func retry(taskID: Int) async throws -> ServiceTask {
        try await request("api/v1/tasks/\(taskID)/retry", method: "POST")
    }

    public func cancel(taskID: Int) async throws -> ServiceTask {
        try await request("api/v1/tasks/\(taskID)/cancel", method: "POST")
    }

    public func appleSources(status: String, limit: Int = 50) async throws -> AppleSourcePage {
        try await get("api/apple/sources?status=\(status)&limit=\(min(max(limit, 1), 100))")
    }

    public func claimAppleExport(workerID: String) async throws -> AppleExportClaim? {
        try await request("api/apple/exports/claim", method: "POST", body: AppleExportWorker(workerID: workerID))
    }

    public func renewAppleExport(itemID: Int, workerID: String) async throws {
        try await requestNoContent("api/apple/exports/\(itemID)/renew", body: AppleExportWorker(workerID: workerID))
    }

    public func commitAppleExport(sourceID: Int, workerID: String, packageURL: URL) async throws {
        let _: AppleExportCommit = try await request(
            "api/apple/sources/\(sourceID)/commit-package", method: "POST",
            body: AppleExportCommitRequest(workerID: workerID, packagePath: packageURL.path)
        )
    }

    private func get<Response: Decodable & Sendable>(_ path: String) async throws -> Response {
        try await request(path, method: "GET")
    }

    private func request<Response: Decodable & Sendable>(
        _ path: String,
        method: String
    ) async throws -> Response {
        guard let url = URL(string: path, relativeTo: baseURL) else {
            throw ServiceDashboardError.invalidBaseURL
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw ServiceDashboardError.response((response as? HTTPURLResponse)?.statusCode ?? 0)
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(Response.self, from: data)
    }

    private func request<Response: Decodable & Sendable, Body: Encodable>(
        _ path: String, method: String, body: Body
    ) async throws -> Response {
        guard let url = URL(string: path, relativeTo: baseURL) else { throw ServiceDashboardError.invalidBaseURL }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(body)
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw ServiceDashboardError.response((response as? HTTPURLResponse)?.statusCode ?? 0)
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(Response.self, from: data)
    }

    private func requestNoContent<Body: Encodable>(_ path: String, body: Body) async throws {
        guard let url = URL(string: path, relativeTo: baseURL) else { throw ServiceDashboardError.invalidBaseURL }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(body)
        let (_, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw ServiceDashboardError.response((response as? HTTPURLResponse)?.statusCode ?? 0)
        }
    }
}

private struct AppleExportWorker: Encodable { let workerID: String }
private struct AppleExportCommitRequest: Encodable { let workerID: String; let packagePath: String }
private struct AppleExportCommit: Decodable, Sendable {}
