import Foundation
import PhotoBridgeLib

func runServiceMonitoringTests() {
    suite("Native service monitoring") {
        test("retries startup connection failures within a bounded readiness window") {
            let policy = ServiceReadinessPolicy(maximumAttempts: 3, pollInterval: .zero)
            try expect(
                policy.decision(afterAttempt: 1, error: URLError(.cannotConnectToHost)),
                equals: .retry
            )
            try expect(
                policy.decision(afterAttempt: 3, error: URLError(.cannotConnectToHost)),
                equals: .timedOut
            )
        }

        test("fails readiness immediately for a different library") {
            let policy = ServiceReadinessPolicy(maximumAttempts: 3, pollInterval: .zero)
            try expect(
                policy.decision(afterAttempt: 1, error: ServiceDashboardError.libraryMismatch),
                equals: .fail
            )
        }

        test("alerts once after consecutive failures and resets after recovery") {
            var policy = ServiceFailureAlertPolicy(failureThreshold: 2)
            try expect(!policy.record(success: false))
            try expect(policy.record(success: false))
            try expect(!policy.record(success: false))
            try expect(!policy.record(success: true))
            try expect(!policy.record(success: false))
            try expect(policy.record(success: false))
        }

        test("redacts media paths, user paths, and credentials") {
            let library = "/Volumes/Photos/PicManager"
            let home = FileManager.default.homeDirectoryForCurrentUser.path
            let input = "open \(library)/2026/IMG_0042.HEIC home=\(home)/secret token=abc123"
            let output = redactServiceLog(input, libraryPath: library)
            try expect(!output.contains("IMG_0042.HEIC"))
            try expect(!output.contains(home))
            try expect(!output.contains("abc123"))
            try expect(output.contains("<library-path>"))
        }
    }
}
