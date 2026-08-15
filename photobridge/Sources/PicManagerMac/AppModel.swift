import Foundation
import PhotoBridgeLib

@MainActor
final class AppModel: ObservableObject {
    @Published var configuration: MacAppConfiguration
    @Published var serviceStatus = "Stopped"
    @Published var lastError: String?

    init() {
        configuration = (try? MacAppConfiguration.load(from: MacAppConfiguration.applicationSupportURL))
            ?? .default
    }

    func saveConfiguration() {
        do {
            try configuration.save(to: MacAppConfiguration.applicationSupportURL)
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }
}
