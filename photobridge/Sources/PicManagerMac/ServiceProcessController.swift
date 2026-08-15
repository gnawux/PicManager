import Foundation
import PhotoBridgeLib

@MainActor
final class ServiceProcessController {
    var onStateChange: ((ServiceLifecycleState) -> Void)?

    private(set) var state: ServiceLifecycleState = .stopped {
        didSet { onStateChange?(state) }
    }
    private var process: Process?
    private var launchConfiguration: LaunchConfiguration?
    private var restartAttempt = 0
    private var restartTask: Task<Void, Never>?
    private var requestedStop = false
    private let restartPolicy: ServiceRestartPolicy

    init(restartPolicy: ServiceRestartPolicy = ServiceRestartPolicy()) {
        self.restartPolicy = restartPolicy
    }

    func start(executableURL: URL, configuration: MacAppConfiguration) {
        guard process == nil else { return }
        launchConfiguration = LaunchConfiguration(
            executableURL: executableURL,
            libraryPath: configuration.libraryPath,
            host: configuration.host,
            port: configuration.port
        )
        requestedStop = false
        restartAttempt = 0
        launch()
    }

    func stop() {
        requestedStop = true
        restartTask?.cancel()
        restartTask = nil
        guard let process else {
            state = .stopped
            return
        }
        state = .stopping
        process.terminate()
    }

    private func launch() {
        guard let launchConfiguration, !requestedStop else { return }
        state = .starting
        let process = Process()
        process.executableURL = launchConfiguration.executableURL
        process.arguments = ["serve"]
        var environment = ProcessInfo.processInfo.environment
        environment["PICMANAGER_LIBRARY_PATH"] = launchConfiguration.libraryPath
        environment["PICMANAGER_HOST"] = launchConfiguration.host
        environment["PICMANAGER_PORT"] = String(launchConfiguration.port)
        process.environment = environment
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        process.terminationHandler = { [weak self] process in
            let status = process.terminationStatus
            Task { @MainActor [weak self] in self?.handleTermination(status) }
        }
        do {
            try process.run()
            self.process = process
            state = .running(processID: process.processIdentifier)
        } catch {
            self.process = nil
            handleTermination(-1)
        }
    }

    private func handleTermination(_ exitCode: Int32) {
        process = nil
        if requestedStop {
            state = .stopped
            return
        }
        restartAttempt += 1
        guard let delay = restartPolicy.delaySeconds(for: restartAttempt) else {
            state = .failed(exitCode: exitCode)
            return
        }
        state = .restarting(exitCode: exitCode, attempt: restartAttempt)
        restartTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled else { return }
            self?.launch()
        }
    }
}

private struct LaunchConfiguration {
    let executableURL: URL
    let libraryPath: String
    let host: String
    let port: UInt16
}
