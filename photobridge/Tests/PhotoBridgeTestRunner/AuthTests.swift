import Photos
@testable import PhotoBridgeLib

func runAuthTests() {
    suite("PhotoLibraryAuth.mapStatus") {
        test("authorized → .authorized") {
            let result = try mapStatus(.authorized)
            try expect(result == .authorized, "expected .authorized")
        }

        test("limited → .limited") {
            let result = try mapStatus(.limited)
            try expect(result == .limited, "expected .limited")
        }

        test("denied → throws AuthError.denied") {
            var threw = false
            do {
                _ = try mapStatus(.denied)
            } catch AuthError.denied {
                threw = true
            }
            try expect(threw, "expected AuthError.denied to be thrown")
        }

        test("restricted → throws AuthError.restricted") {
            var threw = false
            do {
                _ = try mapStatus(.restricted)
            } catch AuthError.restricted {
                threw = true
            }
            try expect(threw, "expected AuthError.restricted to be thrown")
        }

        test("notDetermined → throws AuthError.unknown") {
            var threw = false
            do {
                _ = try mapStatus(.notDetermined)
            } catch AuthError.unknown {
                threw = true
            }
            try expect(threw, "expected AuthError.unknown to be thrown")
        }
    }
}
