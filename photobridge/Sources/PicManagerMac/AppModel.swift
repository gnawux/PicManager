import Foundation
import AppKit
import Photos
import UniformTypeIdentifiers
import OSLog
import PhotoBridgeLib

@MainActor
final class AppModel: ObservableObject {
    private let appleSyncLog = Logger(subsystem: "io.picmanager.mac", category: "apple-sync")
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
    private var appleSyncMonitorTask: Task<Void, Never>?
    private var alertPolicy = ServiceFailureAlertPolicy()
    private let readinessPolicy = ServiceReadinessPolicy()
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
                || previous?.port != configuration.port
                || previous?.garminEmail != configuration.garminEmail {
                stopService()
                libraryOwnership = nil
                dashboard = nil
                Task {
                    if await ensureServiceRunning() {
                        await refreshDashboard()
                        startHealthMonitoring()
                        startAppleSyncMonitoring()
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

    func presentGarminCredentials() {
        let alert = NSAlert(); alert.messageText = "Connect Garmin China"; alert.informativeText = "Credentials are saved only in macOS Keychain."
        let stack = NSStackView(); stack.orientation = .vertical
        let email = NSTextField(string: configuration.garminEmail ?? ""); email.placeholderString = "Garmin account"
        let password = NSSecureTextField(); password.placeholderString = "Garmin password"
        stack.addArrangedSubview(email); stack.addArrangedSubview(password); alert.accessoryView = stack
        alert.addButton(withTitle: "Save and sync"); alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        do { configuration.garminEmail = email.stringValue; try GarminKeychain.save(password: password.stringValue); saveConfiguration() }
        catch { lastError = error.localizedDescription }
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

    /// PhotoKit is only available while the foreground app is running. Keep discovery
    /// deliberately infrequent and serialize it with the manual refresh control.
    func startAppleSyncMonitoring() {
        guard appleSyncMonitorTask == nil else { return }
        appleSyncMonitorTask = Task { [weak self] in
            guard let self else { return }
            await synchronizeAppleInventoryIfEnabled()
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(900))
                await synchronizeAppleInventoryIfEnabled()
            }
        }
    }

    private func synchronizeAppleInventoryIfEnabled() async {
        guard configuration.applePhotosSyncPolicy != .disabled,
              photoAccess == .authorized else { return }
        await synchronizeAppleInventory()
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
        let garminPassword = try? GarminKeychain.password()
        serviceProcess.start(executableURL: serviceExecutable.url, configuration: configuration, garminPassword: garminPassword ?? nil)
        serviceStatus = "Preparing library"
        lastError = nil
        return await waitForServiceReadiness(at: configuration.serviceURL)
    }

    private func waitForServiceReadiness(at serviceURL: URL?) async -> Bool {
        guard let serviceURL else {
            serviceStatus = "Invalid configuration"
            lastError = ServiceDashboardError.invalidBaseURL.localizedDescription
            return false
        }
        let client = dashboardClient(baseURL: serviceURL)
        for attempt in 1...readinessPolicy.maximumAttempts {
            guard !Task.isCancelled else { return false }
            do {
                dashboard = try await client.load()
                serviceStatus = dashboard?.health.status.capitalized ?? "Available"
                lastError = nil
                return true
            } catch {
                switch readinessPolicy.decision(afterAttempt: attempt, error: error) {
                case .retry:
                    serviceStatus = "Preparing library (check \(attempt)/\(readinessPolicy.maximumAttempts))"
                    lastError = nil
                    try? await Task.sleep(for: readinessPolicy.pollInterval)
                case .fail:
                    dashboard = nil
                    serviceStatus = error is ServiceDashboardError ? "Incompatible service" : "Unavailable"
                    lastError = error.localizedDescription
                    return false
                case .timedOut:
                    dashboard = nil
                    serviceStatus = "Startup timed out"
                    lastError = "The local service did not become ready within 30 seconds. Check diagnostics for startup errors."
                    return false
                }
            }
        }
        return false
    }

    func stopService() {
        healthMonitorTask?.cancel()
        healthMonitorTask = nil
        appleSyncMonitorTask?.cancel()
        appleSyncMonitorTask = nil
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
        guard !inventorySyncInProgress else { return }
        if serviceExecutable == nil { prepareServiceExecutable() }
        guard let serviceExecutable else {
            inventorySyncProgress = "Apple Photos sync cannot start: local service executable unavailable."
            return
        }
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
            if configuration.applePhotosSyncPolicy == .importNew {
                try await synchronizeQueuedAppleExports(maximum: 3)
            }
            inventorySyncProgress = "Apple Photos inventory is up to date."
            await refreshDashboard()
        } catch {
            inventorySyncProgress = "Inventory refresh failed."
            lastError = error.localizedDescription
        }
    }

    private func synchronizeQueuedAppleExports(maximum: Int) async throws {
        guard let serviceURL = configuration.serviceURL else { return }
        let client = dashboardClient(baseURL: serviceURL)
        let workerID = "mac-\(ProcessInfo.processInfo.processIdentifier)-\(UUID().uuidString)"
        let staging = URL(fileURLWithPath: configuration.libraryPath, isDirectory: true)
            .appendingPathComponent(".sync-staging/apple", isDirectory: true)
        for index in 0..<maximum {
            guard let claim = try await client.claimAppleExport(workerID: workerID) else { break }
            appleSyncLog.info("claimed Apple export item \(claim.itemID, privacy: .public)")
            let package = staging.appendingPathComponent("source-\(claim.source.id)", isDirectory: true)
            inventorySyncProgress = "Downloading Apple Photo \(index + 1) of up to \(maximum)…"
            let heartbeat = Task {
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(45))
                    try? await client.renewAppleExport(itemID: claim.itemID, workerID: workerID)
                }
            }
            do {
                appleSyncLog.info("checking Apple Photos authorization")
                let authorization = try await requestPhotoLibraryAccess()
                if case .limited = authorization { throw AuthError.limited }
                appleSyncLog.info("starting Apple resource export")
                // PhotoKit's synchronous asset lookup must not inherit the app
                // main actor. The command-line exporter runs off the main actor;
                // keeping the same execution context here avoids a stalled
                // lookup while the WebKit UI continues to repaint.
                let identifier = claim.source.externalID
                try await Task.detached(priority: .utility) {
                    try await exportAppleRenditionPackage(identifier: identifier, destination: package)
                }.value
                try await client.commitAppleExport(sourceID: claim.source.id, workerID: workerID, packageURL: package)
                appleSyncLog.info("committed Apple rendition package")
                try? FileManager.default.removeItem(at: package)
            } catch {
                heartbeat.cancel()
                let message = error.localizedDescription
                appleSyncLog.error("Apple export failed: \(message, privacy: .public)")
                try? await client.failAppleExport(itemID: claim.itemID, workerID: workerID, error: message)
                try? FileManager.default.removeItem(at: package)
                inventorySyncProgress = "Apple Photos export failed; continuing with the next item."
                continue
            }
            heartbeat.cancel()
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
