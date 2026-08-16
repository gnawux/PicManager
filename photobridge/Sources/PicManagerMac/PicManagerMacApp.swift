import SwiftUI
import AppKit
import PhotoBridgeLib

@main
struct PicManagerMacApp: App {
    @StateObject private var model = AppModel()
    @StateObject private var activation = AppActivationController()

    var body: some Scene {
        MenuBarExtra("PicManager", systemImage: "photo.on.rectangle.angled") {
            MenuBarContent(model: model)
        }
        Window("PicManager", id: "library") {
            LibraryShellView(model: model, activation: activation)
                .frame(minWidth: 900, minHeight: 600)
        }
        Settings {
            ConfigurationView(model: model)
                .frame(width: 520)
                .padding()
        }
    }
}

private struct MenuBarContent: View {
    @ObservedObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text("Service: \(model.serviceStatus)")
        if let dashboard = model.dashboard {
            Text("Tasks: \(dashboard.metrics.running) running, \(dashboard.metrics.queued) queued")
            Text("Apple Photos: \(dashboard.synchronizedSourceCount) synchronized")
            if let failed = dashboard.tasks.first(where: \.canRetry) {
                Button("Retry \(failed.kind.replacingOccurrences(of: "_", with: " "))") {
                    Task { await model.retryTask(failed) }
                }
            }
        }
        if model.inventorySyncInProgress {
            Text(model.inventorySyncProgress)
        }
        Button("Refresh Apple Photos Inventory") {
            Task { await model.synchronizeAppleInventory() }
        }
        .disabled(model.inventorySyncInProgress || model.serviceExecutable == nil)
        Button("Open Library") {
            switch libraryPresentationTarget(for: model.configuration) {
            case .embedded: openWindow(id: "library")
            case .systemBrowser: model.openLibraryInSystemBrowser()
            case .unavailable: break
            }
        }
        Button("Refresh Status") { Task { await model.refreshDashboard() } }
        Button("Export Diagnostics…") { Task { await model.exportDiagnostics() } }
        SettingsLink { Text("Settings…") }
        Divider()
        Button("Quit PicManager") { NSApplication.shared.terminate(nil) }
    }
}

private struct LibraryShellView: View {
    @ObservedObject var model: AppModel
    @ObservedObject var activation: AppActivationController

    var body: some View {
        Group {
            if model.onboardingRequired {
                OnboardingAssistant(model: model)
            } else {
                LibraryPresentationView(model: model)
                    .task {
                        if await model.ensureServiceRunning() {
                            await model.refreshDashboard()
                            model.startHealthMonitoring()
                            model.startAppleSyncMonitoring()
                        }
                    }
                    .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                        model.stopService()
                    }
            }
        }
        .onAppear { activation.setLibraryWindowVisible(true) }
        .onDisappear { activation.setLibraryWindowVisible(false) }
    }
}

private struct LibraryPresentationView: View {
    @ObservedObject var model: AppModel
    @State private var showingStatus = false

    var body: some View {
        Group {
            switch libraryPresentationTarget(for: model.configuration) {
            case let .embedded(url):
                ZStack {
                    WebLibraryView(serviceURL: url, serviceAvailable: model.dashboard != nil)
                    if model.dashboard == nil {
                        ContentUnavailableView(
                            "Starting PicManager",
                            systemImage: "photo.stack",
                            description: Text(model.lastError ?? "Waiting for the local photo service…")
                        )
                    }
                }
            case .systemBrowser:
                DashboardView(model: model, showsBrowserButton: true)
            case .unavailable:
                ContentUnavailableView("Invalid service URL", systemImage: "link.badge.plus")
            }
        }
        .toolbar {
            ToolbarItemGroup {
                Button("Status", systemImage: "waveform.path.ecg") { showingStatus = true }
                Button("Open in Browser", systemImage: "safari") { model.openLibraryInSystemBrowser() }
            }
        }
        .sheet(isPresented: $showingStatus) {
            DashboardView(model: model)
                .frame(minWidth: 760, minHeight: 520)
        }
    }
}

private struct DashboardView: View {
    @ObservedObject var model: AppModel
    var showsBrowserButton = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack {
                    VStack(alignment: .leading) {
                        Text("PicManager").font(.largeTitle.bold())
                        Text("Local library and synchronization status")
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    if model.dashboardLoading { ProgressView() }
                    if showsBrowserButton {
                        Button("Open Library in Browser") { model.openLibraryInSystemBrowser() }
                            .buttonStyle(.borderedProminent)
                    }
                    Button("Refresh") { Task { await model.refreshDashboard() } }
                }
                HStack(alignment: .top, spacing: 16) {
                    inventoryCard
                    synchronizationCard
                    workCard
                }
                if let dashboard = model.dashboard {
                    GroupBox("Recent background tasks") {
                        VStack(spacing: 0) {
                            ForEach(dashboard.tasks.prefix(8)) { task in
                                HStack {
                                    Image(systemName: taskIcon(task.status))
                                    Text(task.kind.replacingOccurrences(of: "_", with: " ").capitalized)
                                    Spacer()
                                    Text(task.status.capitalized).foregroundStyle(.secondary)
                                    Text("\(task.completedItems)/\(task.totalItems)")
                                        .monospacedDigit().foregroundStyle(.secondary)
                                    if task.canRetry {
                                        Button("Retry") { Task { await model.retryTask(task) } }
                                    } else if task.canCancel {
                                        Button("Cancel") { Task { await model.cancelTask(task) } }
                                    }
                                }
                                .padding(.vertical, 8)
                                if task.id != dashboard.tasks.prefix(8).last?.id { Divider() }
                            }
                            if dashboard.tasks.isEmpty {
                                Text("No background tasks yet.")
                                    .foregroundStyle(.secondary).padding()
                            }
                        }.padding(.horizontal, 8)
                    }
                } else {
                    ContentUnavailableView(
                        "Local service unavailable",
                        systemImage: "bolt.horizontal.circle",
                        description: Text(model.lastError ?? "Start the PicManager service to see task and synchronization health.")
                    )
                }
            }
            .padding(28)
        }
    }

    private var inventoryCard: some View {
        GroupBox("Apple Photos source") {
            VStack(alignment: .leading, spacing: 8) {
                Text("\(model.appleInventory?.totalCount ?? 0)").font(.title.bold()).monospacedDigit()
                Text("\(model.appleInventory?.imageCount ?? 0) photos · \(model.appleInventory?.videoCount ?? 0) videos")
                    .foregroundStyle(.secondary)
            }.frame(maxWidth: .infinity, alignment: .leading).padding(8)
        }
    }

    private var synchronizationCard: some View {
        GroupBox("Synchronization") {
            VStack(alignment: .leading, spacing: 8) {
                Text("\(model.dashboard?.synchronizedSourceCount ?? 0) synchronized")
                    .font(.title2.bold()).monospacedDigit()
                Text("\(model.dashboard?.pendingSourceCount ?? 0) need attention")
                    .foregroundStyle(.secondary)
                Text(model.dashboard?.latestAppleSync?.status.capitalized ?? "No sync recorded")
                    .foregroundStyle(.secondary)
                if model.inventorySyncInProgress {
                    ProgressView().controlSize(.small)
                    Text(model.inventorySyncProgress).foregroundStyle(.secondary)
                } else {
                    Button("Refresh Inventory") { Task { await model.synchronizeAppleInventory() } }
                        .disabled(model.serviceExecutable == nil)
                }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(8)
        }
    }

    private var workCard: some View {
        GroupBox("Background work") {
            VStack(alignment: .leading, spacing: 8) {
                Text("\(model.dashboard?.metrics.running ?? 0) running")
                    .font(.title2.bold()).monospacedDigit()
                Text("\(model.dashboard?.metrics.queued ?? 0) queued · \(model.dashboard?.metrics.retryWait ?? 0) retrying")
                    .foregroundStyle(.secondary)
                Text("Service: \(model.serviceStatus)").foregroundStyle(.secondary)
                if let executable = model.serviceExecutable {
                    Text("Version \(executable.version.description)").foregroundStyle(.secondary)
                }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(8)
        }
    }

    private func taskIcon(_ status: String) -> String {
        switch status {
        case "succeeded", "completed": "checkmark.circle.fill"
        case "failed": "exclamationmark.triangle.fill"
        case "running", "leased": "arrow.trianglehead.2.clockwise.rotate.90"
        default: "clock"
        }
    }
}

private struct OnboardingAssistant: View {
    @ObservedObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Set up PicManager").font(.largeTitle.bold())
            GroupBox("1. Choose your PicManager library") {
                HStack {
                    Text(model.configuration.libraryPath).lineLimit(1)
                    Spacer()
                    Button("Choose…") { model.chooseLibrary() }
                }.padding(8)
            }
            GroupBox("2. Allow Apple Photos access") {
                HStack {
                    Text(photoAccessLabel)
                    Spacer()
                    Button("Request Full Access") {
                        Task { await model.requestPhotosAccess() }
                    }
                }.padding(8)
            }
            Text("PicManager reads the System Photo Library selected in Photos.app. It does not switch or move that library.")
                .foregroundStyle(.secondary)
            if let guidance = model.onboardingReadiness.guidance {
                Label(guidance, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.orange)
            }
            if let error = model.lastError {
                Text(error).foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button("Finish Setup") { model.finishOnboarding() }
                    .buttonStyle(.borderedProminent)
                    .disabled(!model.onboardingReadiness.canFinish)
            }
        }
        .padding(36)
        .frame(maxWidth: 720)
    }

    private var photoAccessLabel: String {
        switch model.photoAccess {
        case .authorized: "Full Access granted"
        case .limited: "Limited Access"
        case .denied: "Access denied"
        case .restricted: "Access restricted"
        case .notDetermined: "Not requested"
        }
    }
}

private struct ConfigurationView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        Form {
            LabeledContent("Library") {
                HStack {
                    Text(model.configuration.libraryPath).lineLimit(1).textSelection(.enabled)
                    Button("Choose…") { model.chooseLibrary() }
                }
            }
            TextField("Service executable", text: Binding(
                get: { model.configuration.serviceExecutablePath ?? "" },
                set: { model.configuration.serviceExecutablePath = $0.isEmpty ? nil : $0 }
            ))
            Picker("Open photos in", selection: $model.configuration.presentationMode) {
                Text("Embedded window").tag(PhotoPresentationMode.embedded)
                Text("System browser").tag(PhotoPresentationMode.systemBrowser)
            }
            Toggle("Launch PicManager at login", isOn: $model.configuration.launchAtLogin)
            Picker("Apple Photos sync", selection: $model.configuration.applePhotosSyncPolicy) {
                Text("Off").tag(ApplePhotosSyncPolicy.disabled)
                Text("Discover new photos").tag(ApplePhotosSyncPolicy.inventoryOnly)
                Text("Discover and import new photos").tag(ApplePhotosSyncPolicy.importNew)
            }
            Text("Discovery compares Apple Photos metadata every 15 minutes while PicManager is open. Importing downloads only newly queued photos and can be interrupted safely.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(model.launchAtLoginStatus)
                .font(.caption)
                .foregroundStyle(.secondary)
            if let error = model.lastError {
                Text(error).foregroundStyle(.red)
            }
            Button("Export Diagnostics…") { Task { await model.exportDiagnostics() } }
            Button("Save") { model.saveConfiguration() }
        }
    }
}
