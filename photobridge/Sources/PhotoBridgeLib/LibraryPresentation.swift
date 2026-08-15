import Foundation

public enum LibraryPresentationTarget: Equatable, Sendable {
    case embedded(URL)
    case systemBrowser(URL)
    case unavailable
}

public func libraryPresentationTarget(for configuration: MacAppConfiguration) -> LibraryPresentationTarget {
    guard let url = configuration.serviceURL else { return .unavailable }
    return switch configuration.presentationMode {
    case .embedded: .embedded(url)
    case .systemBrowser: .systemBrowser(url)
    }
}
