import PhotoBridgeLib

func runLaunchAtLoginPreferenceTests() {
    suite("Launch at login preference") {
        test("registers and unregisters only when state changes") {
            try expect(launchAtLoginAction(preferred: true, registered: false), equals: .register)
            try expect(launchAtLoginAction(preferred: false, registered: true), equals: .unregister)
            try expect(launchAtLoginAction(preferred: true, registered: true), equals: .none)
            try expect(launchAtLoginAction(preferred: false, registered: false), equals: .none)
        }
    }
}
