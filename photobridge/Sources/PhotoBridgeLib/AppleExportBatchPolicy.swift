public enum AppleSyncInitiator: Sendable {
    case automatic
    case userInitiated
}

public enum AppleExportBatchPolicy {
    public static let automaticMaximumItems = 3

    public static func maximumItems(for initiator: AppleSyncInitiator) -> Int? {
        switch initiator {
        case .automatic: automaticMaximumItems
        case .userInitiated: nil
        }
    }
}
