import Photos

public enum AuthError: Error, Equatable {
    case denied
    case restricted
    case limited       // partial access — warn user to grant full access
    case unknown
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
