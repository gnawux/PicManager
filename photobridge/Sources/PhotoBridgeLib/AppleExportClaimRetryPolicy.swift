import Foundation

public enum AppleExportClaimRetryPolicy {
    public static let maximumAttempts = 5

    public static func delayMilliseconds(afterFailure failureCount: Int) -> Int {
        let exponent = max(0, min(failureCount - 1, 4))
        return min(2_000, 200 * (1 << exponent))
    }

    public static func shouldRetry(_ error: Error, afterFailure failureCount: Int) -> Bool {
        guard failureCount < maximumAttempts, !(error is CancellationError) else { return false }
        if case let ServiceDashboardError.response(status) = error {
            return status == 408 || status == 429 || (500...599).contains(status)
        }
        guard let urlError = error as? URLError else { return false }
        return [
            .timedOut, .cannotConnectToHost, .networkConnectionLost,
            .notConnectedToInternet, .cannotFindHost,
        ].contains(urlError.code)
    }
}
