import Foundation

// MARK: - Result types

public struct ImportResult {
    public let succeededPaths: [URL]
    public let skippedPaths:   [URL]
    public let failedPaths:    [URL]

    public init(succeededPaths: [URL], skippedPaths: [URL], failedPaths: [URL]) {
        self.succeededPaths = succeededPaths
        self.skippedPaths   = skippedPaths
        self.failedPaths    = failedPaths
    }
}

public enum PicManagerError: Error, LocalizedError {
    case notFound
    case exitFailure(Int32)

    public var errorDescription: String? {
        switch self {
        case .notFound:
            return """
            picmanager executable not found.
            Install it or pass --picmanager /path/to/picmanager.
            """
        case .exitFailure(let code):
            return "picmanager exited with code \(code)"
        }
    }
}

// MARK: - Pure log parser (testable without subprocess)

private struct LogEntry: Decodable {
    let path: String
    let status: String
}

/// Parses a multi-line NDJSON string (picmanager import --log output) into an ImportResult.
/// Malformed lines are silently ignored.
public func parseImportLog(_ text: String) -> ImportResult {
    var succeeded: [URL] = []
    var skipped:   [URL] = []
    var failed:    [URL] = []

    for line in text.components(separatedBy: "\n") {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { continue }
        guard let data = trimmed.data(using: .utf8),
              let entry = try? JSONDecoder().decode(LogEntry.self, from: data) else { continue }
        let url = URL(fileURLWithPath: entry.path)
        switch entry.status {
        case "imported": succeeded.append(url)
        case "skipped":  skipped.append(url)
        case "failed":   failed.append(url)
        default:         failed.append(url)
        }
    }
    return ImportResult(succeededPaths: succeeded, skippedPaths: skipped, failedPaths: failed)
}

// MARK: - Runner (requires subprocess)

public struct PicManagerRunner {
    public let executableURL: URL

    public init(executableURL: URL) {
        self.executableURL = executableURL
    }

    /// Searches PATH for `picmanager`. Returns nil if not found.
    public static func findInPath() -> URL? {
        let env = ProcessInfo.processInfo.environment
        let paths = (env["PATH"] ?? "").components(separatedBy: ":")
        for dir in paths {
            let candidate = URL(fileURLWithPath: dir).appendingPathComponent("picmanager")
            if FileManager.default.isExecutableFile(atPath: candidate.path) {
                return candidate
            }
        }
        return nil
    }

    /// Calls `picmanager import --log <logFile> --batch-size <n> <stagingDir>`,
    /// waits for completion, parses the NDJSON log, returns ImportResult.
    public func importBatch(stagingDir: URL, batchSize: Int) async throws -> ImportResult {
        let logFile = stagingDir.appendingPathComponent(".picmanager-import.ndjson")
        // Remove stale log from a previous run
        try? FileManager.default.removeItem(at: logFile)

        let process = Process()
        process.executableURL = executableURL
        process.arguments = [
            "import",
            "--log", logFile.path,
            "--batch-size", "\(batchSize)",
            stagingDir.path,
        ]
        process.standardOutput = FileHandle.nullDevice
        process.standardError  = FileHandle.standardError

        try process.run()
        process.waitUntilExit()

        let exitCode = process.terminationStatus
        guard exitCode == 0 else {
            throw PicManagerError.exitFailure(exitCode)
        }

        let logText = (try? String(contentsOf: logFile, encoding: .utf8)) ?? ""
        try? FileManager.default.removeItem(at: logFile)
        return parseImportLog(logText)
    }
}
