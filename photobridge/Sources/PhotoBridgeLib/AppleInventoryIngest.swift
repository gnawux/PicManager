import Foundation

public struct AppleInventoryIngestCommand: Equatable, Sendable {
    public let executableURL: URL
    public let arguments: [String]
    public let libraryPath: String

    public init(executableURL: URL, inventoryURL: URL, libraryPath: String) {
        self.executableURL = executableURL
        arguments = ["apple", "inventory", inventoryURL.path, "--json"]
        self.libraryPath = libraryPath
    }
}

public func ingestAppleInventory(_ command: AppleInventoryIngestCommand) async throws -> String {
    try await withCheckedThrowingContinuation { continuation in
        let process = Process()
        let output = Pipe()
        let errors = Pipe()
        process.executableURL = command.executableURL
        process.arguments = command.arguments
        var environment = ProcessInfo.processInfo.environment
        environment["PICMANAGER_LIBRARY_PATH"] = command.libraryPath
        process.environment = environment
        process.standardOutput = output
        process.standardError = errors
        process.terminationHandler = { process in
            let stdout = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            let stderr = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            if process.terminationStatus == 0 {
                continuation.resume(returning: stdout)
            } else {
                continuation.resume(throwing: ServiceExecutableError.versionCheckFailed(
                    process.terminationStatus,
                    stderr.trimmingCharacters(in: .whitespacesAndNewlines)
                ))
            }
        }
        do {
            try process.run()
        } catch {
            continuation.resume(throwing: error)
        }
    }
}
