import ArgumentParser

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
            SetupCommand.self,
        ]
    )
}
