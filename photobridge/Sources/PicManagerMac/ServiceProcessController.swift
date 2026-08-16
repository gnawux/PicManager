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
    private let logCapture: ServiceLogCapture
    private var outputPipe: Pipe?
    private var errorPipe: Pipe?

    init(
        restartPolicy: ServiceRestartPolicy = ServiceRestartPolicy(),
        logURL: URL = MacAppConfiguration.applicationSupportURL
            .deletingLastPathComponent().appendingPathComponent("Logs/service.log")
    ) {
        self.restartPolicy = restartPolicy
        logCapture = ServiceLogCapture(logURL: logURL)
    }

    func start(executableURL: URL, configuration: MacAppConfiguration) {
        guard process == nil else { return }
        launchConfiguration = LaunchConfiguration(
            executableURL: executableURL,
            libraryPath: configuration.libraryPath,
            host: configuration.host,
            port: configuration.port
        )
        logCapture.configure(libraryPath: configuration.libraryPath)
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
        var environment = SystemProxyEnvironment.applyingMacSystemProxy(
            to: ProcessInfo.processInfo.environment
        )
        environment["PICMANAGER_LIBRARY_PATH"] = launchConfiguration.libraryPath
        environment["PICMANAGER_HOST"] = launchConfiguration.host
        environment["PICMANAGER_PORT"] = String(launchConfiguration.port)
        process.environment = environment
        let outputPipe = Pipe()
        let errorPipe = Pipe()
        outputPipe.fileHandleForReading.readabilityHandler = { [logCapture] handle in
            logCapture.append(handle.availableData)
        }
        errorPipe.fileHandleForReading.readabilityHandler = { [logCapture] handle in
            logCapture.append(handle.availableData)
        }
        process.standardOutput = outputPipe
        process.standardError = errorPipe
        process.terminationHandler = { [weak self] process in
            let status = process.terminationStatus
            Task { @MainActor [weak self] in self?.handleTermination(status) }
        }
        do {
            try process.run()
            self.process = process
            self.outputPipe = outputPipe
            self.errorPipe = errorPipe
            state = .running(processID: process.processIdentifier)
        } catch {
            self.process = nil
            handleTermination(-1)
        }
    }

    private func handleTermination(_ exitCode: Int32) {
        outputPipe?.fileHandleForReading.readabilityHandler = nil
        errorPipe?.fileHandleForReading.readabilityHandler = nil
        outputPipe = nil
        errorPipe = nil
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
