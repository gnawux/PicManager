import Photos

public enum AuthError: Error, Equatable, LocalizedError {
    case denied
    case restricted
    case limited
    case unknown

    public var errorDescription: String? {
        switch self {
        case .denied:
            return """
                Photos access denied.
                Grant access in System Settings → Privacy & Security → Photos → photobridge (Full Access).
                If photobridge doesn't appear there yet, re-sign the binary first:
                  codesign --force --sign - \\
                    --entitlements Sources/PhotoBridge/PhotoBridge.entitlements \\
                    .build/release/photobridge
                Then run photobridge again — macOS will show a consent dialog.
                """
        case .restricted:
            return "Photos access is restricted by a device policy and cannot be granted."
        case .limited:
            return "Photos access is limited. Grant Full Access in System Settings → Privacy & Security → Photos."
        case .unknown:
            return "Photos authorization status is undetermined. Try running photobridge again."
        }
    }
}

public enum AuthResult {
    case authorized
    case limited
}

/// Requests full Photos library read access, awaiting the system dialog if needed.
/// - Throws: `AuthError` if access cannot be granted.
@available(macOS 13, *)
public func requestPhotoLibraryAccess() async throws -> AuthResult {
    let status = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
    return try mapStatus(status)
}

/// Maps a raw PHAuthorizationStatus to our domain type.
public func mapStatus(_ status: PHAuthorizationStatus) throws -> AuthResult {
    switch status {
    case .authorized:
        return .authorized
    case .limited:
        return .limited
    case .denied:
        throw AuthError.denied
    case .restricted:
        throw AuthError.restricted
    case .notDetermined:
        throw AuthError.unknown
    @unknown default:
        throw AuthError.unknown
    }
}
