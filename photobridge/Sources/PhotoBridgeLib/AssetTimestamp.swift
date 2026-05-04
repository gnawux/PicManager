import Foundation

/// Sets the modification date and creation date of the file at `url` to `date`.
public func applyTimestamp(to url: URL, date: Date) throws {
    try FileManager.default.setAttributes(
        [.modificationDate: date, .creationDate: date],
        ofItemAtPath: url.path
    )
}
