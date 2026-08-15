import ServiceManagement
import PhotoBridgeLib

@MainActor
final class LaunchAtLoginController {
    var isRegistered: Bool {
        SMAppService.mainApp.status == .enabled
    }

    var statusLabel: String {
        switch SMAppService.mainApp.status {
        case .enabled: "Enabled"
        case .requiresApproval: "Requires approval in System Settings"
        case .notFound: "Unavailable outside the installed app"
        case .notRegistered: "Disabled"
        @unknown default: "Unknown"
        }
    }

    func apply(preferred: Bool) throws {
        switch launchAtLoginAction(preferred: preferred, registered: isRegistered) {
        case .register:
            try SMAppService.mainApp.register()
        case .unregister:
            try SMAppService.mainApp.unregister()
        case .none:
            break
        }
    }
}
