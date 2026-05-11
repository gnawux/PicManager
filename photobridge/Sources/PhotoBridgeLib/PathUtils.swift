import Foundation

/// Resolves a user-supplied executable path (absolute or relative) to an absolute URL.
/// Uses NSString path APIs which correctly collapse `..` components; `URL.standardized`
/// drops `..` without backing up the directory, causing relative paths like
/// `../bin/picmanager` to resolve incorrectly.
public func resolveExecutableURL(_ path: String) -> URL {
    let nsPath = path as NSString
    let absolute = nsPath.isAbsolutePath
        ? path
        : (FileManager.default.currentDirectoryPath as NSString).appendingPathComponent(path)
    return URL(fileURLWithPath: (absolute as NSString).standardizingPath)
}
