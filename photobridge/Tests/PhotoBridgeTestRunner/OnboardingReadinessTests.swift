import PhotoBridgeLib

func runOnboardingReadinessTests() {
    suite("Mac app onboarding readiness") {
        test("requires a confirmed library") {
            let state = OnboardingReadiness(libraryConfirmed: false, photoAccess: .authorized)
            try expect(!state.canFinish)
            try expect(state.guidance, equals: "Choose a PicManager library folder.")
        }

        test("requires full Photos access") {
            let limited = OnboardingReadiness(libraryConfirmed: true, photoAccess: .limited)
            try expect(!limited.canFinish)
            try expect(limited.guidance?.contains("Full Access") == true)
        }

        test("finishes only when both requirements are ready") {
            let ready = OnboardingReadiness(libraryConfirmed: true, photoAccess: .authorized)
            try expect(ready.canFinish)
            try expect(ready.guidance == nil)
        }
    }
}
