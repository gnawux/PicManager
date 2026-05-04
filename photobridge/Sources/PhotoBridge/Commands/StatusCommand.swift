import ArgumentParser

@available(macOS 10.15, *)
struct StatusCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "status",
        abstract: "Show sync status and last run information"
    )

    func run() async throws {
        print("Not yet implemented: status")
    }
}
