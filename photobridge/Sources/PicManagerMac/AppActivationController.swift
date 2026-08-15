import AppKit
import PhotoBridgeLib

@MainActor
final class AppActivationController: ObservableObject {
    private var libraryWindowVisible = false

    func setLibraryWindowVisible(_ visible: Bool) {
        guard visible != libraryWindowVisible else { return }
        libraryWindowVisible = visible
        let policy: NSApplication.ActivationPolicy = switch appActivationMode(
            libraryWindowVisible: visible
        ) {
        case .regular: .regular
        case .accessory: .accessory
        }
        NSApplication.shared.setActivationPolicy(policy)
        if visible {
            NSApplication.shared.activate()
        }
    }
}
