import Foundation
import PhotoBridgeLib

func runServiceExecutableTests() {
    suite("PicManager service executable") {
        test("parses Cargo service versions") {
            try expect(ServiceVersion.parse("picmanager 1.0.1\n"), equals: ServiceVersion(major: 1, minor: 0, patch: 1))
            try expect(ServiceVersion.parse("not-a-version") == nil)
        }

        test("prefers an explicit executable") {
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: directory) }
            let explicit = directory.appendingPathComponent("custom-picmanager")
            try Data().write(to: explicit)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: explicit.path)
            let located = try ServiceExecutableLocator().locate(
                configuredPath: explicit.path,
                bundleResourceURL: nil,
                applicationSupportURL: directory.appendingPathComponent("support"),
                hostExecutableURL: nil,
                pathEnvironment: ""
            )
            try expect(located, equals: explicit)
        }

        test("installs a bundled executable into Application Support") {
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            let resources = directory.appendingPathComponent("resources")
            let support = directory.appendingPathComponent("support")
            try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: directory) }
            let bundled = resources.appendingPathComponent("picmanager")
            try Data("service".utf8).write(to: bundled)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: bundled.path)
            let located = try ServiceExecutableLocator().locate(
                configuredPath: nil,
                bundleResourceURL: resources,
                applicationSupportURL: support,
                hostExecutableURL: nil,
                pathEnvironment: ""
            )
            try expect(located.path, equals: support.appendingPathComponent("Service/picmanager").path)
            try expect(FileManager.default.isExecutableFile(atPath: located.path))
        }
    }
}
