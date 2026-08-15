import SwiftUI
import AppKit
import PhotoBridgeLib

@main
struct PicManagerMacApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        MenuBarExtra("PicManager", systemImage: "photo.on.rectangle.angled") {
            MenuBarContent(model: model)
        }
        Window("PicManager", id: "library") {
            LibraryShellView(model: model)
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
        }
        Button("Open Library") { openWindow(id: "library") }
        Button("Refresh Status") { Task { await model.refreshDashboard() } }
        SettingsLink { Text("Settings…") }
        Divider()
        Button("Quit PicManager") { NSApplication.shared.terminate(nil) }
    }
}

private struct LibraryShellView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        if model.onboardingRequired {
            OnboardingAssistant(model: model)
        } else {
            DashboardView(model: model)
                .task {
                    model.prepareServiceExecutable()
                    await model.refreshDashboard()
                }
        }
    }
}

private struct DashboardView: View {
    @ObservedObject var model: AppModel

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
            TextField("Library", text: $model.configuration.libraryPath)
            TextField("Service executable", text: Binding(
                get: { model.configuration.serviceExecutablePath ?? "" },
                set: { model.configuration.serviceExecutablePath = $0.isEmpty ? nil : $0 }
            ))
            Picker("Open photos in", selection: $model.configuration.presentationMode) {
                Text("Embedded window").tag(PhotoPresentationMode.embedded)
                Text("System browser").tag(PhotoPresentationMode.systemBrowser)
            }
            if let error = model.lastError {
                Text(error).foregroundStyle(.red)
            }
            Button("Save") { model.saveConfiguration() }
        }
    }
}
