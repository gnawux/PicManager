import Foundation

func writeDiagnosticArchive(files: [String: Data], to destination: URL) throws {
    let fileManager = FileManager.default
    let workspace = fileManager.temporaryDirectory
        .appendingPathComponent("PicManager-Diagnostics-\(UUID().uuidString)", isDirectory: true)
    let stagingArchive = destination.deletingLastPathComponent()
        .appendingPathComponent(".\(destination.lastPathComponent).\(UUID().uuidString).partial")
    defer {
        try? fileManager.removeItem(at: workspace)
        try? fileManager.removeItem(at: stagingArchive)
    }
    try fileManager.createDirectory(at: workspace, withIntermediateDirectories: true)
    for (name, data) in files {
        try data.write(to: workspace.appendingPathComponent(name), options: .atomic)
    }
    let process = Process()
    let errors = Pipe()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
    process.arguments = ["-c", "-k", "--sequesterRsrc", "--keepParent", workspace.path, stagingArchive.path]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = errors
    try process.run()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else {
        let message = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        throw CocoaError(.fileWriteUnknown, userInfo: [NSLocalizedDescriptionKey: message])
    }
    if fileManager.fileExists(atPath: destination.path) {
        _ = try fileManager.replaceItemAt(destination, withItemAt: stagingArchive)
    } else {
        try fileManager.moveItem(at: stagingArchive, to: destination)
    }
}
