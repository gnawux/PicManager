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
        Button("Open Library") { openWindow(id: "library") }
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
            ContentUnavailableView(
                "PicManager service is not running",
                systemImage: "photo.stack",
                description: Text("The native shell will display the local Web library here.")
            )
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
