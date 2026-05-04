import ArgumentParser

@available(macOS 10.15, *)
struct SyncCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "sync",
        abstract: "Incremental sync of new photos since last export"
    )

    @Option(help: "Staging directory for exported files")
    var output: String?

    @Option(help: "Photos per batch (default: 200)")
    var batchSize: Int = 200

    @Option(help: "Max concurrent iCloud downloads (default: 4)")
    var maxConcurrent: Int = 4

    @Option(help: "Path to picmanager executable")
    var picmanager: String?

    func run() async throws {
        print("Not yet implemented: sync")
    }
}
