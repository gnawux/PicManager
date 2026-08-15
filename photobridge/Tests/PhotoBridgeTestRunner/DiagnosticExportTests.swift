import Foundation
import PhotoBridgeLib

func runDiagnosticExportTests() {
    suite("Safe diagnostic export") {
        test("omits bookmarks, executable paths, library paths, and media names") {
            let configuration = MacAppConfiguration(
                libraryPath: "/Volumes/Private Photos/PicManager",
                libraryBookmark: Data("secret-bookmark".utf8),
                serviceExecutablePath: "/Users/person/private/picmanager"
            )
            let files = try makeDiagnosticExport(
                configuration: configuration,
                dashboard: nil,
                serviceLog: "opened /Volumes/Private Photos/PicManager/2026/IMG_0042.HEIC token=secret"
            )
            let combined = files.values.map { String(decoding: $0, as: UTF8.self) }.joined()
            try expect(!combined.contains("secret-bookmark"))
            try expect(!combined.contains("/Users/person"))
            try expect(!combined.contains("/Volumes/Private Photos"))
            try expect(!combined.contains("IMG_0042.HEIC"))
            try expect(!combined.contains("token=secret"))
            try expect(files.keys.sorted(), equals: ["configuration.json", "health.json", "service.log"])
        }
    }
}
