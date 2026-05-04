import Foundation
import PhotoBridgeLib

func runSyncStateTests() {
    suite("SyncState") {
        test("default init has nil token, nil date, zero count") {
            let state = SyncState()
            try expect(state.lastSyncToken == nil, "expected nil token")
            try expect(state.lastSyncDate == nil, "expected nil date")
            try expect(state.exportedCount == 0, "expected 0 count")
        }

        test("Codable round-trip with no token") {
            let state = SyncState(lastSyncToken: nil, lastSyncDate: nil, exportedCount: 0)
            let data = try JSONEncoder().encode(state)
            let decoded = try JSONDecoder().decode(SyncState.self, from: data)
            try expect(decoded.lastSyncToken == nil, "token should be nil")
            try expect(decoded.lastSyncDate == nil, "date should be nil")
            try expect(decoded.exportedCount == 0, "count should be 0")
        }

        test("Codable round-trip with token and date") {
            let tokenData = Data([0x01, 0x02, 0x03])
            let date = Date(timeIntervalSince1970: 1_700_000_000)
            let state = SyncState(lastSyncToken: tokenData, lastSyncDate: date, exportedCount: 42)
            let data = try JSONEncoder().encode(state)
            let decoded = try JSONDecoder().decode(SyncState.self, from: data)
            try expect(decoded.lastSyncToken == tokenData, "token mismatch")
            try expect(decoded.exportedCount == 42, "count should be 42")
            // Date round-trips within JSON precision (1 second)
            let diff = abs(decoded.lastSyncDate!.timeIntervalSince(date))
            try expect(diff < 1.0, "date should round-trip within 1 second")
        }

        test("load from missing file returns default state") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_test_nonexistent_\(Int.random(in: 0..<Int.max)).json")
            let state = SyncState.load(from: url)
            try expect(state.lastSyncToken == nil, "expected nil token for missing file")
            try expect(state.exportedCount == 0, "expected zero count for missing file")
        }

        test("save then load round-trips correctly") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_test_syncstate_\(Int.random(in: 0..<Int.max)).json")
            defer { try? FileManager.default.removeItem(at: url) }
            let tokenData = Data([0xAB, 0xCD])
            let date = Date(timeIntervalSince1970: 1_710_000_000)
            var state = SyncState(lastSyncToken: tokenData, lastSyncDate: date, exportedCount: 99)
            try state.save(to: url)
            let loaded = SyncState.load(from: url)
            try expect(loaded.lastSyncToken == tokenData, "token mismatch after save/load")
            try expect(loaded.exportedCount == 99, "count mismatch after save/load")
        }

        test("save creates parent directories if needed") {
            let dir = URL(fileURLWithPath: "/tmp/photobridge_test_dir_\(Int.random(in: 0..<Int.max))")
            let url = dir.appendingPathComponent("state.json")
            defer { try? FileManager.default.removeItem(atPath: dir.path) }
            var state = SyncState(exportedCount: 7)
            try state.save(to: url)
            let loaded = SyncState.load(from: url)
            try expect(loaded.exportedCount == 7, "count mismatch")
        }
    }
}
