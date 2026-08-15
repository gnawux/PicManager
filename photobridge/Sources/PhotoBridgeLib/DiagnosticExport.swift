import Foundation

public func makeDiagnosticExport(
    configuration: MacAppConfiguration,
    dashboard: ServiceDashboard?,
    serviceLog: String
) throws -> [String: Data] {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let configurationRecord = DiagnosticConfigurationRecord(
        libraryPath: "<redacted>",
        host: configuration.host,
        port: configuration.port,
        presentationMode: configuration.presentationMode.rawValue,
        launchAtLogin: configuration.launchAtLogin
    )
    let healthRecord = DiagnosticHealthRecord(
        serviceVersion: dashboard?.contract.serviceVersion,
        apiVersion: dashboard?.contract.apiVersion,
        healthStatus: dashboard?.health.status ?? "unavailable",
        schemaVersion: dashboard?.health.schemaVersion,
        queued: dashboard?.metrics.queued ?? 0,
        running: dashboard?.metrics.running ?? 0,
        failed: dashboard?.failedWorkCount ?? 0,
        appleSourceStatusCounts: dashboard?.sourceStatusCounts ?? [:]
    )
    return [
        "configuration.json": try encoder.encode(configurationRecord),
        "health.json": try encoder.encode(healthRecord),
        "service.log": Data(redactServiceLog(serviceLog, libraryPath: configuration.libraryPath).utf8),
    ]
}

private struct DiagnosticConfigurationRecord: Encodable {
    let libraryPath: String
    let host: String
    let port: UInt16
    let presentationMode: String
    let launchAtLogin: Bool
}

private struct DiagnosticHealthRecord: Encodable {
    let serviceVersion: String?
    let apiVersion: String?
    let healthStatus: String
    let schemaVersion: Int?
    let queued: Int
    let running: Int
    let failed: Int
    let appleSourceStatusCounts: [String: Int]
}
