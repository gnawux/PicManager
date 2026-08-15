import Foundation
import AppKit
import Photos
import UniformTypeIdentifiers
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
    @Published var serviceExecutable: ResolvedServiceExecutable?
    @Published var inventorySyncInProgress = false
    @Published var inventorySyncProgress = ""
    @Published var launchAtLoginStatus = "Disabled"
    private let serviceProcess = ServiceProcessController()
    private let notifications = NativeNotifications()
    private var healthMonitorTask: Task<Void, Never>?
    private var alertPolicy = ServiceFailureAlertPolicy()
    private var libraryOwnership: LibraryOwnershipLock?
    private let launchAtLogin = LaunchAtLoginController()

    init() {
        let saved = try? MacAppConfiguration.load(from: MacAppConfiguration.applicationSupportURL)
        configuration = saved ?? .default
        onboardingRequired = saved == nil
        libraryConfirmed = saved != nil
        photoAccess = Self.photoAccessReadiness()
        serviceProcess.onStateChange = { [weak self] state in
            self?.serviceStatus = state.label
        }
        launchAtLoginStatus = launchAtLogin.statusLabel
    }

    func saveConfiguration() {
        do {
            let previous = try? MacAppConfiguration.load(from: MacAppConfiguration.applicationSupportURL)
            try launchAtLogin.apply(preferred: configuration.launchAtLogin)
            try configuration.save(to: MacAppConfiguration.applicationSupportURL)
            launchAtLoginStatus = launchAtLogin.statusLabel
            lastError = nil
            if previous?.libraryPath != configuration.libraryPath
                || previous?.host != configuration.host
                || previous?.port != configuration.port {
                stopService()
                libraryOwnership = nil
                dashboard = nil
                Task {
                    if await ensureServiceRunning() {
                        await refreshDashboard()
                        startHealthMonitoring()
                    }
                }
            }
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
            dashboard = try await dashboardClient(baseURL: serviceURL).load()
            serviceStatus = dashboard?.health.status.capitalized ?? "Available"
            lastError = nil
        } catch {
            dashboard = nil
            serviceStatus = "Unavailable"
            lastError = error.localizedDescription
        }
    }

    func startHealthMonitoring() {
        guard healthMonitorTask == nil else { return }
        healthMonitorTask = Task { [weak self] in
            guard let self else { return }
            await notifications.requestAuthorization()
            while !Task.isCancelled {
                let succeeded = await pollServiceHealth()
                if alertPolicy.record(success: succeeded) {
                    await notifications.serviceFailure("The local photo service is unavailable after repeated health checks.")
                }
                try? await Task.sleep(for: .seconds(5))
            }
        }
    }

    private func pollServiceHealth() async -> Bool {
        guard let serviceURL = configuration.serviceURL else { return false }
        do {
            dashboard = try await dashboardClient(baseURL: serviceURL).load()
            serviceStatus = dashboard?.health.status.capitalized ?? "Available"
            lastError = nil
            return dashboard?.health.status == "healthy"
        } catch {
            serviceStatus = "Unavailable"
            lastError = error.localizedDescription
            return false
        }
    }

    func prepareServiceExecutable() {
        do {
            let locator = ServiceExecutableLocator()
            let located = try locator.locate(
                configuredPath: configuration.serviceExecutablePath,
                bundleResourceURL: Bundle.main.resourceURL,
                applicationSupportURL: MacAppConfiguration.applicationSupportURL.deletingLastPathComponent(),
                hostExecutableURL: Bundle.main.executableURL
            )
            serviceExecutable = try locator.validate(located)
            configuration.serviceExecutablePath = located.path
            lastError = nil
        } catch {
            serviceExecutable = nil
            lastError = error.localizedDescription
        }
    }

    func ensureServiceRunning() async -> Bool {
        do {
            if libraryOwnership?.libraryPath != URL(fileURLWithPath: configuration.libraryPath).standardizedFileURL.path {
                libraryOwnership = try LibraryOwnershipLock(
                    libraryURL: URL(fileURLWithPath: configuration.libraryPath, isDirectory: true)
                )
            }
        } catch {
            serviceStatus = "Library in use"
            lastError = error.localizedDescription
            return false
        }
        prepareServiceExecutable()
        guard let serviceExecutable else { return false }
        if let serviceURL = configuration.serviceURL {
            do {
                _ = try await dashboardClient(baseURL: serviceURL).load()
                serviceStatus = "Healthy"
                return true
            } catch ServiceDashboardError.libraryMismatch {
                serviceStatus = "Wrong library"
                lastError = ServiceDashboardError.libraryMismatch.localizedDescription
                return false
            } catch {
                // No compatible service is listening, so start the owned service below.
            }
        }
        serviceProcess.start(executableURL: serviceExecutable.url, configuration: configuration)
        return true
    }

    func stopService() {
        healthMonitorTask?.cancel()
        healthMonitorTask = nil
        serviceProcess.stop()
    }

    func openLibraryInSystemBrowser() {
        guard let url = configuration.serviceURL else {
            lastError = ServiceDashboardError.invalidBaseURL.localizedDescription
            return
        }
        NSWorkspace.shared.open(url)
    }

    func exportDiagnostics() async {
        let panel = NSSavePanel()
        panel.title = "Export PicManager Diagnostics"
        panel.nameFieldStringValue = "PicManager-Diagnostics.zip"
        panel.allowedContentTypes = [.zip]
        guard panel.runModal() == .OK, let destination = panel.url else { return }
        do {
            let logURL = MacAppConfiguration.applicationSupportURL
                .deletingLastPathComponent().appendingPathComponent("Logs/service.log")
            let log = (try? String(contentsOf: logURL, encoding: .utf8)) ?? ""
            let files = try makeDiagnosticExport(
                configuration: configuration,
                dashboard: dashboard,
                serviceLog: log
            )
            try await Task.detached { try writeDiagnosticArchive(files: files, to: destination) }.value
            lastError = nil
        } catch {
            lastError = error.localizedDescription
        }
    }

    func synchronizeAppleInventory() async {
        guard !inventorySyncInProgress, let serviceExecutable else { return }
        inventorySyncInProgress = true
        inventorySyncProgress = "Preparing Apple Photos inventory…"
        defer { inventorySyncInProgress = false }
        let inventoryURL = MacAppConfiguration.applicationSupportURL
            .deletingLastPathComponent().appendingPathComponent("Sync/apple-inventory.ndjson")
        defer { try? FileManager.default.removeItem(at: inventoryURL) }
        do {
            let updateProgress: @Sendable (Int, Int) -> Void = { [weak self] completed, total in
                guard completed % 250 == 0 || completed == total else { return }
                Task { @MainActor [weak self] in
                    self?.inventorySyncProgress = "Reading Apple Photos: \(completed)/\(total)"
                }
            }
            let count = try await Task.detached {
                try writeApplePhotoInventory(to: inventoryURL, progress: updateProgress)
            }.value
            inventorySyncProgress = "Comparing \(count) Apple Photos items…"
            _ = try await ingestAppleInventory(AppleInventoryIngestCommand(
                executableURL: serviceExecutable.url,
                inventoryURL: inventoryURL,
                libraryPath: configuration.libraryPath
            ))
            inventorySyncProgress = "Apple Photos inventory is up to date."
            await refreshDashboard()
        } catch {
            inventorySyncProgress = "Inventory refresh failed."
            lastError = error.localizedDescription
        }
    }

    func retryTask(_ task: ServiceTask) async {
        await updateTask(task, retry: true)
    }

    func cancelTask(_ task: ServiceTask) async {
        await updateTask(task, retry: false)
    }

    private func updateTask(_ task: ServiceTask, retry: Bool) async {
        guard let serviceURL = configuration.serviceURL else { return }
        do {
            let client = dashboardClient(baseURL: serviceURL)
            if retry { _ = try await client.retry(taskID: task.id) }
            else { _ = try await client.cancel(taskID: task.id) }
            await refreshDashboard()
        } catch {
            lastError = error.localizedDescription
        }
    }

    private func dashboardClient(baseURL: URL) -> ServiceDashboardClient {
        ServiceDashboardClient(baseURL: baseURL, expectedLibraryPath: configuration.libraryPath)
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
