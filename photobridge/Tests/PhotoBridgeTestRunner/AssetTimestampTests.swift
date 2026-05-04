import Foundation
import PhotoBridgeLib

func runAssetTimestampTests() {
    suite("applyTimestamp") {
        test("sets mtime and ctime to given date") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_ts_\(Int.random(in: 0..<Int.max)).txt")
            FileManager.default.createFile(atPath: url.path, contents: nil)
            defer { try? FileManager.default.removeItem(at: url) }

            let target = Date(timeIntervalSince1970: 1_678_838_400) // 2023-03-15 00:00:00 UTC
            try applyTimestamp(to: url, date: target)

            let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
            let mtime = attrs[.modificationDate] as? Date
            let ctime = attrs[.creationDate] as? Date

            try expect(mtime != nil, "modificationDate should be set")
            try expect(ctime != nil, "creationDate should be set")
            try expect(abs(mtime!.timeIntervalSince(target)) < 2.0, "mtime should match within 2s")
            try expect(abs(ctime!.timeIntervalSince(target)) < 2.0, "ctime should match within 2s")
        }

        test("throws for non-existent path") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_nonexistent_\(Int.random(in: 0..<Int.max)).txt")
            var threw = false
            do {
                try applyTimestamp(to: url, date: Date())
            } catch {
                threw = true
            }
            try expect(threw, "should throw for non-existent file")
        }

        test("preserves file contents after applying timestamp") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_ts_content_\(Int.random(in: 0..<Int.max)).txt")
            let content = Data("hello".utf8)
            FileManager.default.createFile(atPath: url.path, contents: content)
            defer { try? FileManager.default.removeItem(at: url) }

            try applyTimestamp(to: url, date: Date(timeIntervalSince1970: 1_000_000_000))

            let read = try Data(contentsOf: url)
            try expect(read == content, "file contents should be unchanged")
        }
    }
}
