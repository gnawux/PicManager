import Foundation
import PhotoBridgeLib

func runAppleInventoryIngestTests() {
    suite("Native Apple inventory controls") {
        test("builds a metadata-only Rust ingest command") {
            let command = AppleInventoryIngestCommand(
                executableURL: URL(fileURLWithPath: "/Applications/PicManager/picmanager"),
                inventoryURL: URL(fileURLWithPath: "/tmp/apple.ndjson"),
                libraryPath: "/Volumes/Photos/PicManager"
            )
            try expect(command.arguments, equals: ["apple", "inventory", "/tmp/apple.ndjson", "--json"])
            try expect(command.libraryPath, equals: "/Volumes/Photos/PicManager")
        }

        test("offers retry and cancel only for valid task states") {
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            func task(_ status: String) throws -> ServiceTask {
                let json = #"{"id":1,"kind":"apple_full","provider":"apple_photos","status":"STATUS","total_items":10,"completed_items":2,"failed_items":0,"error":null,"updated_at":"2026-08-15T00:00:00Z","source":"sync","progress_stage":null}"#
                    .replacingOccurrences(of: "STATUS", with: status)
                return try decoder.decode(ServiceTask.self, from: Data(json.utf8))
            }
            try expect(try task("failed").canRetry)
            try expect(try task("running").canCancel)
            try expect(!(try task("completed").canRetry))
            try expect(!(try task("completed").canCancel))
        }
    }
}
