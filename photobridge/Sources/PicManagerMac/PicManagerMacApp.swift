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
        ContentUnavailableView(
            "PicManager service is not running",
            systemImage: "photo.stack",
            description: Text("The native shell will display the local Web library here.")
        )
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
