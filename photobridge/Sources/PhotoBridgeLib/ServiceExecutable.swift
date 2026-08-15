import Foundation

public struct ServiceVersion: Equatable, Sendable, CustomStringConvertible {
    public let major: Int
    public let minor: Int
    public let patch: Int

    public init(major: Int, minor: Int, patch: Int) {
        self.major = major
        self.minor = minor
        self.patch = patch
    }

    public var description: String { "\(major).\(minor).\(patch)" }

    public static func parse(_ output: String) -> ServiceVersion? {
        let token = output.split(whereSeparator: { $0.isWhitespace })
            .last(where: { $0.first?.isNumber == true })?
            .split(separator: ".")
        guard let token, token.count >= 3,
              let major = Int(token[0]), let minor = Int(token[1]),
              let patch = Int(token[2].prefix(while: { $0.isNumber })) else { return nil }
        return ServiceVersion(major: major, minor: minor, patch: patch)
    }
}

public struct ResolvedServiceExecutable: Equatable, Sendable {
    public let url: URL
    public let version: ServiceVersion

    public init(url: URL, version: ServiceVersion) {
        self.url = url
        self.version = version
    }
}

public enum ServiceExecutableError: LocalizedError, Equatable {
    case notFound
    case invalidVersionOutput
    case incompatibleVersion(ServiceVersion)
    case versionCheckFailed(Int32, String)

    public var errorDescription: String? {
        switch self {
        case .notFound:
            "The bundled PicManager service could not be found."
        case .invalidVersionOutput:
            "The PicManager service returned an invalid version."
        case let .incompatibleVersion(version):
            "PicManager service \(version) is not compatible with this app."
        case let .versionCheckFailed(code, message):
            "PicManager service version check failed (\(code)): \(message)"
        }
    }
}

public struct ServiceExecutableLocator: Sendable {
    public static let supportedMajorVersion = 1

    public init() {}

    public func locate(
        configuredPath: String?,
        bundleResourceURL: URL?,
        applicationSupportURL: URL,
        hostExecutableURL: URL?,
        pathEnvironment: String? = ProcessInfo.processInfo.environment["PATH"]
    ) throws -> URL {
        let fileManager = FileManager.default
        let installed = applicationSupportURL
            .appendingPathComponent("Service", isDirectory: true)
            .appendingPathComponent("picmanager")
        let bundled = bundleResourceURL?.appendingPathComponent("picmanager")
        let siblings: [URL] = hostExecutableURL.map {
            let directory = $0.deletingLastPathComponent()
            return [directory.appendingPathComponent("picmanager-service"), directory.appendingPathComponent("picmanager")]
        } ?? []
        var candidates: [URL] = configuredPath.map { [URL(fileURLWithPath: $0)] } ?? []
        candidates.append(installed)
        if let match = candidates.first(where: { fileManager.isExecutableFile(atPath: $0.path) }) {
            return match
        }
        if let bundled, fileManager.isExecutableFile(atPath: bundled.path) {
            return try installBundledService(from: bundled, to: installed)
        }
        candidates = siblings + pathCandidates(pathEnvironment)
        if let match = candidates.first(where: { fileManager.isExecutableFile(atPath: $0.path) }) {
            return match
        }
        throw ServiceExecutableError.notFound
    }

    public func validate(_ executableURL: URL) throws -> ResolvedServiceExecutable {
        let process = Process()
        let output = Pipe()
        let errors = Pipe()
        process.executableURL = executableURL
        process.arguments = ["--version"]
        process.standardOutput = output
        process.standardError = errors
        try process.run()
        process.waitUntilExit()
        let stdout = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        let stderr = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        guard process.terminationStatus == 0 else {
            throw ServiceExecutableError.versionCheckFailed(
                process.terminationStatus,
                stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            )
        }
        guard let version = ServiceVersion.parse(stdout) else {
            throw ServiceExecutableError.invalidVersionOutput
        }
        guard version.major == Self.supportedMajorVersion else {
            throw ServiceExecutableError.incompatibleVersion(version)
        }
        return ResolvedServiceExecutable(url: executableURL, version: version)
    }

    private func pathCandidates(_ environment: String?) -> [URL] {
        (environment ?? "").split(separator: ":").map {
            URL(fileURLWithPath: String($0)).appendingPathComponent("picmanager")
        }
    }

    private func installBundledService(from source: URL, to destination: URL) throws -> URL {
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
        let staging = destination.appendingPathExtension(UUID().uuidString)
        defer { try? fileManager.removeItem(at: staging) }
        try fileManager.copyItem(at: source, to: staging)
        try fileManager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: staging.path)
        if fileManager.fileExists(atPath: destination.path) {
            _ = try fileManager.replaceItemAt(destination, withItemAt: staging)
        } else {
            try fileManager.moveItem(at: staging, to: destination)
        }
        return destination
    }
}
