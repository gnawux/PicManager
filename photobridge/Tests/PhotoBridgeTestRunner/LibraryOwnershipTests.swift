import Foundation
import PhotoBridgeLib

func runLibraryOwnershipTests() {
    suite("Mac library ownership") {
        test("only one app lock owns a library at a time") {
            let library = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: library) }
            var first: LibraryOwnershipLock? = try LibraryOwnershipLock(libraryURL: library)
            var secondWasBlocked = false
            do {
                _ = try LibraryOwnershipLock(libraryURL: library)
            } catch LibraryOwnershipError.unavailable {
                secondWasBlocked = true
            } catch {
                throw error
            }
            try expect(secondWasBlocked)
            try expect(first != nil)
            first = nil
            let replacement = try LibraryOwnershipLock(libraryURL: library)
            try expect(replacement.libraryPath, equals: library.standardizedFileURL.path)
        }
    }
}
