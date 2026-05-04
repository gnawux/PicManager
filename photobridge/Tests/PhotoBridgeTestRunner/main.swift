// Step 39a
suite("PhotoBridge package") {
    test("library compiles") {
        try expect(1 + 1, equals: 2)
    }
}

// Step 39b
runAuthTests()

// Step 40a
runAssetFilterTests()

// Step 40b
runAssetExporterTests()

// Step 41a
runSyncStateTests()

// Step 41b
runIncrementalEnumeratorTests()

finish()
