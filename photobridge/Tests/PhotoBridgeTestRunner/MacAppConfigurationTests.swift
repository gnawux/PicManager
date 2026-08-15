import Foundation
import PhotoBridgeLib

func runMacAppConfigurationTests() {
    suite("Mac app configuration") {
        test("configuration round trips atomically") {
            let directory = FileManager.default.temporaryDirectory
                .appendingPathComponent(UUID().uuidString)
            let url = directory.appendingPathComponent("config.json")
            defer { try? FileManager.default.removeItem(at: directory) }
            let expected = MacAppConfiguration(
                libraryPath: "/Volumes/Photos/PicManager",
                serviceExecutablePath: "/Applications/PicManager.app/Contents/MacOS/picmanager",
                port: 18080,
                presentationMode: .systemBrowser,
                launchAtLogin: true
            )
            try expected.save(to: url)
            try expect(try MacAppConfiguration.load(from: url), equals: expected)
        }

        test("service URL remains loopback by default") {
            let configuration = MacAppConfiguration.default
            try expect(configuration.serviceURL?.host, equals: "127.0.0.1")
        }
    }
}
