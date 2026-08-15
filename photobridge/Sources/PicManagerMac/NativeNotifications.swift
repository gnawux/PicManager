import UserNotifications

@MainActor
final class NativeNotifications {
    func requestAuthorization() async {
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
    }

    func serviceFailure(_ message: String) async {
        let content = UNMutableNotificationContent()
        content.title = "PicManager needs attention"
        content.body = message
        content.sound = .default
        let request = UNNotificationRequest(
            identifier: "picmanager-service-failure",
            content: content,
            trigger: nil
        )
        try? await UNUserNotificationCenter.current().add(request)
    }
}
