import ArgumentParser
import Foundation
import PhotoBridgeLib

@available(macOS 13, *)
struct StatusCommand: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "status",
        abstract: "Show last sync status"
    )

    func run() async throws {
        let stateURL = IncrementalEnumerator.defaultStateURL
        let state = SyncState.load(from: stateURL)

        print("PhotoBridge status")

        if state.lastSyncToken == nil && state.lastSyncDate == nil && state.exportedCount == 0 {
            print("  (Never synced)")
        } else {
            if let date = state.lastSyncDate {
                let f = DateFormatter()
                f.dateFormat = "yyyy-MM-dd HH:mm:ss"
                print("  Last sync:    \(f.string(from: date))")
            }
            let formatted = formatCount(state.exportedCount)
            print("  Total synced: \(formatted) assets")
        }

        print("  State file:   \(stateURL.path)")
    }

    private func formatCount(_ n: Int) -> String {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        return f.string(from: NSNumber(value: n)) ?? "\(n)"
    }
}
