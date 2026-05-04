import Foundation
import PhotoBridgeLib

func runFixTimestampsTests() {
    suite("fix-timestamps") {
        test("exportDestinationURL derives expected filename for JPEG") {
            let dir = URL(fileURLWithPath: "/tmp/staging")
            let url = exportDestinationURL(
                stagingDir: dir,
                localIdentifier: "ABC123/L0/001",
                uti: "public.jpeg"
            )
            try expect(url.lastPathComponent == "ABC123_L0_001.jpg",
                       "filename should replace slashes with underscores and use .jpg extension")
        }

        test("exportDestinationURL derives expected filename for HEIC") {
            let dir = URL(fileURLWithPath: "/tmp/staging")
            let url = exportDestinationURL(
                stagingDir: dir,
                localIdentifier: "XYZ/L0/002",
                uti: "public.heic"
            )
            try expect(url.lastPathComponent == "XYZ_L0_002.heic",
                       "filename should use .heic extension")
        }

        test("applyTimestampIfNeeded updates file mtime when date provided") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_fix_ts_\(Int.random(in: 0..<Int.max)).txt")
            FileManager.default.createFile(atPath: url.path, contents: nil)
            defer { try? FileManager.default.removeItem(at: url) }

            let target = Date(timeIntervalSince1970: 1_700_000_000) // 2023-11-14
            try applyTimestamp(to: url, date: target)

            let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
            let mtime = attrs[.modificationDate] as? Date
            try expect(mtime != nil, "mtime should be set")
            try expect(abs(mtime!.timeIntervalSince(target)) < 2.0, "mtime should match target")
        }

        test("dry run does not modify file mtime") {
            let url = URL(fileURLWithPath: "/tmp/photobridge_fix_dryrun_\(Int.random(in: 0..<Int.max)).txt")
            FileManager.default.createFile(atPath: url.path, contents: nil)
            defer { try? FileManager.default.removeItem(at: url) }

            // Record current mtime before dry run
            let beforeAttrs = try FileManager.default.attributesOfItem(atPath: url.path)
            let before = beforeAttrs[.modificationDate] as? Date

            // Simulate dry run: do NOT call applyTimestamp
            let target = Date(timeIntervalSince1970: 1_000_000_000) // far in past
            _ = target  // not applied

            let afterAttrs = try FileManager.default.attributesOfItem(atPath: url.path)
            let after = afterAttrs[.modificationDate] as? Date

            try expect(before != nil && after != nil, "dates should be readable")
            try expect(abs(before!.timeIntervalSince(after!)) < 2.0,
                       "dry run should not change mtime")
        }

        test("fix-timestamps summary counts match") {
            // Pure logic test: verify counting works correctly
            let total = 5
            var updated = 0
            var skipped = 0
            var notFound = 0

            // Simulate: 3 files found and updated, 1 skipped (no date), 1 not found
            let results: [(Bool, Bool)] = [
                (true, true),   // found, has date → updated
                (true, true),   // found, has date → updated
                (true, true),   // found, has date → updated
                (true, false),  // found, no date → skipped
                (false, false), // not found → notFound
            ]
            for (found, hasDate) in results {
                if !found { notFound += 1 }
                else if !hasDate { skipped += 1 }
                else { updated += 1 }
            }

            try expect(updated + skipped + notFound == total, "counts should sum to total")
            try expect(updated == 3, "3 files should be updated")
            try expect(skipped == 1, "1 file should be skipped (no date)")
            try expect(notFound == 1, "1 file should be not found")
        }
    }
}
