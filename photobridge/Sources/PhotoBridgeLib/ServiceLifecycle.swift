import Foundation

public enum ServiceLifecycleState: Equatable, Sendable {
    case stopped
    case starting
    case running(processID: Int32)
    case stopping
    case restarting(exitCode: Int32, attempt: Int)
    case failed(exitCode: Int32)

    public var label: String {
        switch self {
        case .stopped: "Stopped"
        case .starting: "Starting"
        case .running: "Running"
        case .stopping: "Stopping"
        case let .restarting(_, attempt): "Restarting (attempt \(attempt))"
        case let .failed(code): "Failed (exit \(code))"
        }
    }
}

public struct ServiceRestartPolicy: Equatable, Sendable {
    public let maximumAttempts: Int
    public let baseDelaySeconds: Double
    public let maximumDelaySeconds: Double

    public init(maximumAttempts: Int = 3, baseDelaySeconds: Double = 1, maximumDelaySeconds: Double = 8) {
        self.maximumAttempts = maximumAttempts
        self.baseDelaySeconds = baseDelaySeconds
        self.maximumDelaySeconds = maximumDelaySeconds
    }

    public func delaySeconds(for attempt: Int) -> Double? {
        guard attempt > 0, attempt <= maximumAttempts else { return nil }
        return min(baseDelaySeconds * pow(2, Double(attempt - 1)), maximumDelaySeconds)
    }
}
