import Darwin
import Foundation

public enum LibraryOwnershipError: LocalizedError, Equatable {
    case unavailable(String)
    case system(Int32)

    public var errorDescription: String? {
        switch self {
        case .unavailable:
            "This PicManager library is already open in another app instance."
        case let .system(code):
            "The PicManager library lock failed with system error \(code)."
        }
    }
}

public final class LibraryOwnershipLock: @unchecked Sendable {
    public let libraryPath: String
    private let descriptor: Int32

    public init(libraryURL: URL) throws {
        try FileManager.default.createDirectory(at: libraryURL, withIntermediateDirectories: true)
        libraryPath = libraryURL.standardizedFileURL.path
        let lockURL = libraryURL.appendingPathComponent(".picmanager-app.lock")
        descriptor = open(lockURL.path, O_RDWR | O_CREAT | O_CLOEXEC, S_IRUSR | S_IWUSR)
        guard descriptor >= 0 else { throw LibraryOwnershipError.system(errno) }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            let code = errno
            close(descriptor)
            if code == EWOULDBLOCK { throw LibraryOwnershipError.unavailable(libraryPath) }
            throw LibraryOwnershipError.system(code)
        }
        let owner = "pid=\(ProcessInfo.processInfo.processIdentifier)\n"
        _ = ftruncate(descriptor, 0)
        _ = owner.withCString { write(descriptor, $0, strlen($0)) }
    }

    deinit {
        flock(descriptor, LOCK_UN)
        close(descriptor)
    }
}
