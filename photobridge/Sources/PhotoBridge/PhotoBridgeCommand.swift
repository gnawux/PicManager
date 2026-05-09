import ArgumentParser
import Foundation

/// Resolves a user-supplied executable path (absolute or relative) to an absolute URL.
/// `URL(fileURLWithPath:)` preserves `..` literally, which causes Foundation's pre-flight
/// existence check in `Process.run()` to resolve incorrectly. This function uses
/// `standardized` after anchoring the path to CWD so `../foo` works as expected.
func resolveExecutableURL(_ path: String) -> URL {
    let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
    return URL(fileURLWithPath: path, relativeTo: cwd).standardized
}

@main
@available(macOS 10.15, macCatalyst 13, iOS 13, tvOS 13, watchOS 6, *)
struct PhotoBridge: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "photobridge",
        abstract: "iCloud Photos → PicManager import bridge",
        version: "0.1.0",
        subcommands: [
            ExportCommand.self,
            SyncCommand.self,
            StatusCommand.self,
            FixTimestampsCommand.self,
            FixOrientationsCommand.self,
            SetupCommand.self,
        ]
    )
}
