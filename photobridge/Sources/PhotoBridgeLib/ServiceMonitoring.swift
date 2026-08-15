import Foundation

public struct ServiceFailureAlertPolicy: Equatable, Sendable {
    public let failureThreshold: Int
    private(set) public var consecutiveFailures = 0
    private var alertDelivered = false

    public init(failureThreshold: Int = 3) {
        self.failureThreshold = max(1, failureThreshold)
    }

    public mutating func record(success: Bool) -> Bool {
        if success {
            consecutiveFailures = 0
            alertDelivered = false
            return false
        }
        consecutiveFailures += 1
        guard consecutiveFailures >= failureThreshold, !alertDelivered else { return false }
        alertDelivered = true
        return true
    }
}

public func redactServiceLog(_ text: String, libraryPath: String) -> String {
    var redacted = text
    let sensitiveRoots = [libraryPath, FileManager.default.homeDirectoryForCurrentUser.path]
        .filter { !$0.isEmpty }
    for root in sensitiveRoots {
        let escaped = NSRegularExpression.escapedPattern(for: root)
        redacted = replacingMatches(
            in: redacted,
            pattern: "\(escaped)(?:/[^\\s\\\"']*)?",
            replacement: root == libraryPath ? "<library-path>" : "<user-path>"
        )
    }
    return replacingMatches(
        in: redacted,
        pattern: "(?i)(authorization|token|cookie|password)([=:]\\s*)[^\\s,;]+",
        replacement: "$1$2<redacted>"
    )
}

private func replacingMatches(in value: String, pattern: String, replacement: String) -> String {
    guard let expression = try? NSRegularExpression(pattern: pattern) else { return value }
    let range = NSRange(value.startIndex..<value.endIndex, in: value)
    return expression.stringByReplacingMatches(in: value, range: range, withTemplate: replacement)
}
