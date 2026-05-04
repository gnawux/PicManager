import ArgumentParser

@available(macOS 10.15, *)
struct ExportCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "export",
        abstract: "Full export of all photos from Photos library"
    )

    @Option(help: "Staging directory for exported files")
    var output: String?

    @Option(help: "Photos per batch (default: 200)")
    var batchSize: Int = 200

    @Option(help: "Max concurrent iCloud downloads (default: 4)")
    var maxConcurrent: Int = 4

    @Option(help: "Path to picmanager executable")
    var picmanager: String?

    @Flag(help: "Count assets only, do not export")
    var dryRun: Bool = false

    func run() async throws {
        print("Not yet implemented: export")
    }
}
