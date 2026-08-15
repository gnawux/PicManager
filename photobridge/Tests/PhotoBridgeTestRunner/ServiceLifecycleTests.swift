import PhotoBridgeLib

func runServiceLifecycleTests() {
    suite("Rust service lifecycle") {
        test("uses bounded exponential restart delays") {
            let policy = ServiceRestartPolicy(maximumAttempts: 3, baseDelaySeconds: 0.5, maximumDelaySeconds: 2)
            try expect(policy.delaySeconds(for: 1), equals: 0.5)
            try expect(policy.delaySeconds(for: 2), equals: 1.0)
            try expect(policy.delaySeconds(for: 3), equals: 2.0)
            try expect(policy.delaySeconds(for: 4) == nil)
        }

        test("lifecycle states have stable user labels") {
            try expect(ServiceLifecycleState.running(processID: 42).label, equals: "Running")
            try expect(ServiceLifecycleState.restarting(exitCode: 1, attempt: 2).label, equals: "Restarting (attempt 2)")
        }
    }
}
