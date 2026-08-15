import Foundation

public enum LibraryPresentationTarget: Equatable, Sendable {
    case embedded(URL)
    case systemBrowser(URL)
    case unavailable
}

public enum AppActivationMode: Equatable, Sendable {
    case accessory
    case regular
}

public func appActivationMode(libraryWindowVisible: Bool) -> AppActivationMode {
    libraryWindowVisible ? .regular : .accessory
}

public func libraryPresentationTarget(for configuration: MacAppConfiguration) -> LibraryPresentationTarget {
    guard let url = configuration.serviceURL else { return .unavailable }
    return switch configuration.presentationMode {
    case .embedded: .embedded(url)
    case .systemBrowser: .systemBrowser(url)
    }
}
