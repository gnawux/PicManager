import Foundation
import PhotoBridgeLib

func runPicManagerRunnerTests() {
    suite("resolveExecutableURL") {
        test("absolute path returned unchanged") {
            let url = resolveExecutableURL("/usr/local/bin/picmanager")
            try expect(url.path == "/usr/local/bin/picmanager", "absolute path unchanged")
        }

        test("relative path resolved against CWD") {
            let cwd = FileManager.default.currentDirectoryPath
            let url = resolveExecutableURL("target/release/picmanager")
            try expect(url.path == "\(cwd)/target/release/picmanager", "simple relative path")
        }

        test("parent-relative path (../) resolved correctly") {
            // This is the regression case: ../target/release/picmanager must go UP one
            // directory from CWD, not drop the .. and stay in CWD.
            let cwd = FileManager.default.currentDirectoryPath
            let parent = (cwd as NSString).deletingLastPathComponent
            let url = resolveExecutableURL("../target/release/picmanager")
            try expect(url.path == "\(parent)/target/release/picmanager",
                       ".. correctly backs up one directory (got \(url.path))")
        }

        test("multiple .. segments resolved correctly") {
            let cwd = FileManager.default.currentDirectoryPath
            let grandparent = ((cwd as NSString).deletingLastPathComponent as NSString).deletingLastPathComponent
            let url = resolveExecutableURL("../../bin/picmanager")
            try expect(url.path == "\(grandparent)/bin/picmanager",
                       "multiple .. resolved (got \(url.path))")
        }
    }

    suite("PicManagerRunner NDJSON parsing") {
        test("all-imported log → all in succeededPaths") {
            let log = """
            {"path":"/staging/A_L0_001.heic","status":"imported","sha256":"abc","error":null,"ts":"2026-01-01T00:00:00Z"}
            {"path":"/staging/B_L0_002.jpg","status":"imported","sha256":"def","error":null,"ts":"2026-01-01T00:00:00Z"}
            """
            let result = parseImportLog(log)
            try expect(result.succeededPaths.count == 2, "2 succeeded")
            try expect(result.skippedPaths.isEmpty,      "0 skipped")
            try expect(result.failedPaths.isEmpty,       "0 failed")
            try expect(result.succeededPaths[0] == URL(fileURLWithPath: "/staging/A_L0_001.heic"), "correct URL")
        }

        test("mixed imported/skipped/failed → correct buckets") {
            let log = """
            {"path":"/s/a.heic","status":"imported","sha256":"1","error":null,"ts":"t"}
            {"path":"/s/b.jpg","status":"skipped","sha256":"2","error":null,"ts":"t"}
            {"path":"/s/c.png","status":"failed","sha256":null,"error":"bad","ts":"t"}
            """
            let result = parseImportLog(log)
            try expect(result.succeededPaths.count == 1, "1 succeeded")
            try expect(result.skippedPaths.count  == 1, "1 skipped")
            try expect(result.failedPaths.count   == 1, "1 failed")
        }

        test("unknown status →归入 failedPaths") {
            let log = """
            {"path":"/s/x.jpg","status":"unknown_status","sha256":null,"error":null,"ts":"t"}
            """
            let result = parseImportLog(log)
            try expect(result.failedPaths.count == 1, "unknown status counted as failed")
            try expect(result.succeededPaths.isEmpty, "not in succeeded")
        }

        test("empty log → all buckets empty") {
            let result = parseImportLog("")
            try expect(result.succeededPaths.isEmpty, "no succeeded")
            try expect(result.skippedPaths.isEmpty,   "no skipped")
            try expect(result.failedPaths.isEmpty,    "no failed")
        }

        test("malformed lines ignored, valid lines still parsed") {
            let log = """
            not json at all
            {"path":"/s/a.jpg","status":"imported","sha256":"x","error":null,"ts":"t"}
            {broken
            """
            let result = parseImportLog(log)
            try expect(result.succeededPaths.count == 1, "valid line parsed")
            try expect(result.failedPaths.isEmpty, "malformed lines not counted as failed")
        }

        test("blank lines ignored") {
            let log = "\n\n{\"path\":\"/s/a.jpg\",\"status\":\"imported\",\"sha256\":\"x\",\"error\":null,\"ts\":\"t\"}\n\n"
            let result = parseImportLog(log)
            try expect(result.succeededPaths.count == 1, "1 succeeded despite blank lines")
        }
    }
}
