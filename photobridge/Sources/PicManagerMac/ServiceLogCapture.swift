import Foundation
import PhotoBridgeLib

final class ServiceLogCapture: @unchecked Sendable {
    private let logURL: URL
    private let maximumBytes: UInt64
    private let queue = DispatchQueue(label: "io.picmanager.service-log")
    private var libraryPath = ""

    init(logURL: URL, maximumBytes: UInt64 = 1_000_000) {
        self.logURL = logURL
        self.maximumBytes = maximumBytes
    }

    func configure(libraryPath: String) {
        queue.sync { self.libraryPath = libraryPath }
    }

    func append(_ data: Data) {
        guard !data.isEmpty else { return }
        queue.async { [self] in
            let text = String(decoding: data, as: UTF8.self)
            let safe = redactServiceLog(text, libraryPath: libraryPath)
            guard let safeData = safe.data(using: .utf8) else { return }
            do {
                try FileManager.default.createDirectory(
                    at: logURL.deletingLastPathComponent(),
                    withIntermediateDirectories: true
                )
                try rotateIfNeeded(incomingBytes: UInt64(safeData.count))
                if !FileManager.default.fileExists(atPath: logURL.path) {
                    try Data().write(to: logURL, options: .atomic)
                }
                let handle = try FileHandle(forWritingTo: logURL)
                try handle.seekToEnd()
                try handle.write(contentsOf: safeData)
                try handle.close()
            } catch {
                // Logging must never interrupt the managed service.
            }
        }
    }

    private func rotateIfNeeded(incomingBytes: UInt64) throws {
        let current = (try? logURL.resourceValues(forKeys: [.fileSizeKey]).fileSize).map(UInt64.init) ?? 0
        guard current + incomingBytes > maximumBytes else { return }
        let previous = logURL.deletingPathExtension().appendingPathExtension("previous.log")
        try? FileManager.default.removeItem(at: previous)
        if FileManager.default.fileExists(atPath: logURL.path) {
            try FileManager.default.moveItem(at: logURL, to: previous)
        }
    }
}
