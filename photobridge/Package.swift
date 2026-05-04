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
            path: "Sources/PhotoBridgeLib"
        ),
        // CLI executable
        .executableTarget(
            name: "PhotoBridge",
            dependencies: [
                "PhotoBridgeLib",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ],
            path: "Sources/PhotoBridge"
        ),
        // Test runner executable
        .executableTarget(
            name: "PhotoBridgeTestRunner",
            dependencies: ["PhotoBridgeLib"],
            path: "Tests/PhotoBridgeTestRunner"
        ),
    ]
)
