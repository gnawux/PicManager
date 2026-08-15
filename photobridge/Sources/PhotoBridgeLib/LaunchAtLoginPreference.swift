public enum LaunchAtLoginAction: Equatable, Sendable {
    case none
    case register
    case unregister
}

public func launchAtLoginAction(preferred: Bool, registered: Bool) -> LaunchAtLoginAction {
    switch (preferred, registered) {
    case (true, false): .register
    case (false, true): .unregister
    default: .none
    }
}
