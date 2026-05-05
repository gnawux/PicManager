import Photos
import Foundation
import ImageIO

// MARK: - Pure helpers (testable without Photos runtime)

/// Maps a UTI string to a file extension.
public func fileExtension(forUTI uti: String) -> String {
    switch uti {
    case "public.jpeg", "public.jpg":    return "jpg"
    case "public.heic":                  return "heic"
    case "public.heif":                  return "heif"
    case "public.png":                   return "png"
    case "public.tiff", "public.tif":    return "tiff"
    case "public.webp":                  return "webp"
    case "public.gif":                   return "gif"
    default:                             return "data"
    }
}

/// Builds the destination URL for an exported asset.
/// Slashes in localIdentifier are replaced with underscores to produce a flat filename.
public func exportDestinationURL(
    stagingDir: URL,
    localIdentifier: String,
    uti: String
) -> URL {
    let safeName = localIdentifier.replacingOccurrences(of: "/", with: "_")
    let ext = fileExtension(forUTI: uti)
    return stagingDir.appendingPathComponent("\(safeName).\(ext)")
}

// MARK: - Exported asset record

public struct ExportedAsset: Sendable {
    public let localIdentifier: String
    public let fileURL: URL
    public let takenAt: Date?
}

// MARK: - Exporter (requires Photos runtime)

/// Writes a single PHAssetResource to destURL, downloading from iCloud if needed.
@available(macOS 13, *)
public func writeAssetResource(
    _ resource: PHAssetResource,
    to destURL: URL
) async throws {
    let options = PHAssetResourceRequestOptions()
    options.isNetworkAccessAllowed = true

    try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
        PHAssetResourceManager.default().writeData(
            for: resource,
            toFile: destURL,
            options: options
        ) { error in
            if let error {
                continuation.resume(throwing: error)
            } else {
                continuation.resume()
            }
        }
    }
}

/// Writes a PHAssetResource to destURL and corrects the EXIF orientation if
/// Apple Photos has a user-applied rotation that differs from the embedded tag.
/// Falls back silently to the unmodified file if orientation querying fails.
@available(macOS 13, *)
public func writeAssetResourceOrientationFixed(
    _ resource: PHAssetResource,
    for asset: PHAsset,
    to destURL: URL
) async throws {
    try await writeAssetResource(resource, to: destURL)

    guard let photosOrientation = await requestPhotosDisplayOrientation(for: asset) else { return }
    guard let fileOrientation = readExifOrientationFromFile(at: destURL) else { return }

    if photosOrientation != fileOrientation {
        fixExifOrientationInFile(at: destURL, orientation: photosOrientation)
    }
}

/// Queries Photos for the orientation it uses to display `asset` (with user edits).
/// Returns the EXIF-compatible integer value (1–8), or nil if unavailable.
@available(macOS 13, *)
private func requestPhotosDisplayOrientation(for asset: PHAsset) async -> Int? {
    await withCheckedContinuation { continuation in
        let options = PHImageRequestOptions()
        options.deliveryMode = .fastFormat
        options.isNetworkAccessAllowed = false  // avoid iCloud re-download
        options.version = .current              // include user rotation edits

        PHImageManager.default().requestImageDataAndOrientation(
            for: asset,
            options: options
        ) { _, _, cgOrientation, info in
            if info?[PHImageErrorKey] != nil {
                continuation.resume(returning: nil)
            } else {
                continuation.resume(returning: Int(cgOrientation.rawValue))
            }
        }
    }
}

/// Reads the EXIF Orientation tag from a HEIC or JPEG file. Returns nil on failure.
private func readExifOrientationFromFile(at url: URL) -> Int? {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
    else { return nil }
    return props[kCGImagePropertyOrientation as String] as? Int
}

/// Overwrites the EXIF Orientation tag in-place.
/// Uses exiftool for HEIC/HEIF (sips cannot write orientation to HEIC);
/// uses sips for JPEG/TIFF (fast, no Homebrew dependency).
private func fixExifOrientationInFile(at url: URL, orientation: Int) {
    let ext = url.pathExtension.lowercased()
    if ext == "heic" || ext == "heif" {
        // exiftool is required; silently skip if not installed
        guard let exiftool = findExecutable("exiftool") else { return }
        runProcess(exiftool, args: [
            "-overwrite_original",
            "-Orientation=\(orientation)",
            "-n",
            url.path
        ])
    } else {
        runProcess(URL(fileURLWithPath: "/usr/bin/sips"),
                   args: ["-s", "orientation", "\(orientation)", url.path])
    }
}

private func findExecutable(_ name: String) -> URL? {
    let candidates = [
        "/opt/homebrew/bin/\(name)",
        "/usr/local/bin/\(name)",
        "/usr/bin/\(name)",
    ]
    for path in candidates {
        let url = URL(fileURLWithPath: path)
        if FileManager.default.isExecutableFile(atPath: path) { return url }
    }
    // PATH search via `which`
    let which = Process()
    which.executableURL = URL(fileURLWithPath: "/usr/bin/which")
    which.arguments = [name]
    let pipe = Pipe()
    which.standardOutput = pipe
    which.standardError = FileHandle.nullDevice
    try? which.run()
    which.waitUntilExit()
    let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?
        .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return out.isEmpty ? nil : URL(fileURLWithPath: out)
}

private func runProcess(_ url: URL, args: [String]) {
    let proc = Process()
    proc.executableURL = url
    proc.arguments = args
    proc.standardOutput = FileHandle.nullDevice
    proc.standardError = FileHandle.nullDevice
    try? proc.run()
    proc.waitUntilExit()
}
