import PhotoBridgeLib

func runLibraryPresentationTests() {
    suite("Mac library presentation") {
        test("selects an embedded local Web destination") {
            let configuration = MacAppConfiguration(
                libraryPath: "/tmp/library",
                port: 18080,
                presentationMode: .embedded
            )
            try expect(
                libraryPresentationTarget(for: configuration),
                equals: .embedded(configuration.serviceURL!)
            )
        }

        test("selects the system browser without changing the service URL") {
            let configuration = MacAppConfiguration(
                libraryPath: "/tmp/library",
                presentationMode: .systemBrowser
            )
            try expect(
                libraryPresentationTarget(for: configuration),
                equals: .systemBrowser(configuration.serviceURL!)
            )
        }
    }
}
