import Foundation

// Minimal test runner — no external framework needed
nonisolated(unsafe) var _passed = 0
nonisolated(unsafe) var _failed = 0

func test(_ name: String, _ body: () throws -> Void) {
    do {
        try body()
        print("  ✓ \(name)")
        _passed += 1
    } catch {
        print("  ✗ \(name): \(error)")
        _failed += 1
    }
}

func expect<T: Equatable>(_ actual: T, equals expected: T, file: String = #file, line: Int = #line) throws {
    if actual != expected {
        throw TestFailure.notEqual(
            "\(actual) != \(expected) at \(URL(fileURLWithPath: file).lastPathComponent):\(line)"
        )
    }
}

func expect(_ condition: Bool, _ message: String = "condition was false", file: String = #file, line: Int = #line) throws {
    if !condition {
        throw TestFailure.conditionFailed("\(message) at \(URL(fileURLWithPath: file).lastPathComponent):\(line)")
    }
}

enum TestFailure: Error, CustomStringConvertible {
    case notEqual(String)
    case conditionFailed(String)

    var description: String {
        switch self {
        case .notEqual(let msg): return msg
        case .conditionFailed(let msg): return msg
        }
    }
}

func suite(_ name: String, _ body: () -> Void) {
    print("\n\(name)")
    body()
}

func finish() -> Never {
    print("\n\(_passed + _failed) tests: \(_passed) passed, \(_failed) failed")
    exit(_failed == 0 ? 0 : 1)
}
