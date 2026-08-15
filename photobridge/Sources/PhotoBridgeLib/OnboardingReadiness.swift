public enum PhotoAccessReadiness: String, Codable, Sendable {
    case notDetermined
    case authorized
    case limited
    case denied
    case restricted
}

public struct OnboardingReadiness: Equatable, Sendable {
    public var libraryConfirmed: Bool
    public var photoAccess: PhotoAccessReadiness

    public init(libraryConfirmed: Bool, photoAccess: PhotoAccessReadiness) {
        self.libraryConfirmed = libraryConfirmed
        self.photoAccess = photoAccess
    }

    public var canFinish: Bool {
        libraryConfirmed && photoAccess == .authorized
    }

    public var guidance: String? {
        guard libraryConfirmed else { return "Choose a PicManager library folder." }
        switch photoAccess {
        case .authorized:
            return nil
        case .notDetermined:
            return "Allow Photos access to compare and import your system library."
        case .limited:
            return "PicManager needs Full Access rather than a limited photo selection."
        case .denied:
            return "Enable Photos access for PicManager in System Settings."
        case .restricted:
            return "Photos access is restricted by this Mac's policy."
        }
    }
}
