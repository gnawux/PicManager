import Foundation
import AppKit
import Photos
import PhotoBridgeLib

@MainActor
final class AppModel: ObservableObject {
    @Published var configuration: MacAppConfiguration
    @Published var serviceStatus = "Stopped"
    @Published var lastError: String?
    @Published var onboardingRequired: Bool
    @Published var libraryConfirmed = false
    @Published var photoAccess: PhotoAccessReadiness
    @Published var appleInventory: ApplePhotoInventory?
    @Published var dashboard: ServiceDashboard?
    @Published var dashboardLoading = false

    init() {
        let saved = try? MacAppConfiguration.load(from: MacAppConfiguration.applicationSupportURL)
        configuration = saved ?? .default
        onboardingRequired = saved == nil
        libraryConfirmed = saved != nil
        photoAccess = Self.photoAccessReadiness()
    }

    func saveConfiguration() {
        do {
            try configuration.save(to: MacAppConfiguration.applicationSupportURL)
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    var onboardingReadiness: OnboardingReadiness {
        OnboardingReadiness(libraryConfirmed: libraryConfirmed, photoAccess: photoAccess)
    }

    func chooseLibrary() {
        let panel = NSOpenPanel()
        panel.title = "Choose PicManager Library"
        panel.prompt = "Use This Folder"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let bookmark = try url.bookmarkData(
                options: .withSecurityScope,
                includingResourceValuesForKeys: nil,
                relativeTo: nil
            )
            configuration.libraryPath = url.path
            configuration.libraryBookmark = bookmark
            libraryConfirmed = true
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    func requestPhotosAccess() async {
        do {
            _ = try await requestPhotoLibraryAccess()
        } catch {
            lastError = error.localizedDescription
        }
        photoAccess = Self.photoAccessReadiness()
    }

    func finishOnboarding() {
        guard onboardingReadiness.canFinish else { return }
        do {
            try FileManager.default.createDirectory(
                atPath: configuration.libraryPath,
                withIntermediateDirectories: true
            )
            try configuration.save(to: MacAppConfiguration.applicationSupportURL)
            onboardingRequired = false
            lastError = nil
            Task { await refreshDashboard() }
        } catch {
            lastError = error.localizedDescription
        }
    }

    func refreshDashboard() async {
        guard !onboardingRequired else { return }
        dashboardLoading = true
        defer { dashboardLoading = false }
        if photoAccess == .authorized {
            appleInventory = loadApplePhotoInventory()
        }
        guard let serviceURL = configuration.serviceURL else {
            serviceStatus = "Invalid configuration"
            lastError = ServiceDashboardError.invalidBaseURL.localizedDescription
            return
        }
        do {
            dashboard = try await ServiceDashboardClient(baseURL: serviceURL).load()
            serviceStatus = dashboard?.health.status.capitalized ?? "Available"
            lastError = nil
        } catch {
            dashboard = nil
            serviceStatus = "Unavailable"
            lastError = error.localizedDescription
        }
    }

    private static func photoAccessReadiness() -> PhotoAccessReadiness {
        switch PHPhotoLibrary.authorizationStatus(for: .readWrite) {
        case .authorized: .authorized
        case .limited: .limited
        case .denied: .denied
        case .restricted: .restricted
        case .notDetermined: .notDetermined
        @unknown default: .restricted
        }
    }
}
