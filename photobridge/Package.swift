// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "PhotoBridge",
    platforms: [.macOS(.v26)],
    dependencies: [
        .package(
            url: "https://github.com/apple/swift-argument-parser",
            from: "1.5.0"
        ),
    ],
    targets: [
        // Business logic library (testable)
        .target(
            name: "PhotoBridgeLib",
            path: "Sources/PhotoBridgeLib",
            linkerSettings: [.linkedFramework("Photos")]
        ),
        // CLI executable
        .executableTarget(
            name: "PhotoBridge",
            dependencies: [
                "PhotoBridgeLib",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ],
            path: "Sources/PhotoBridge",
            exclude: ["Info.plist", "PhotoBridge.entitlements"],
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                // Embed Info.plist into the __TEXT,__info_plist section so macOS TCC
                // can find NSPhotoLibraryUsageDescription and show a consent dialog.
                .unsafeFlags([
                    "-Xlinker", "-sectcreate",
                    "-Xlinker", "__TEXT",
                    "-Xlinker", "__info_plist",
                    "-Xlinker", "Sources/PhotoBridge/Info.plist",
                ])
            ]
        ),
        // Test runner executable
        .executableTarget(
            name: "PhotoBridgeTestRunner",
            dependencies: ["PhotoBridgeLib"],
            path: "Tests/PhotoBridgeTestRunner"
        ),
    ]
)
