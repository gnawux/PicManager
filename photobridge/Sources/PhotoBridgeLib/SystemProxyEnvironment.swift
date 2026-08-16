import CFNetwork
import Foundation

public enum SystemProxyEnvironment {
    public static func applyingMacSystemProxy(
        to base: [String: String],
        settings: [AnyHashable: Any]? = CFNetworkCopySystemProxySettings()?.takeRetainedValue() as? [AnyHashable: Any]
    ) -> [String: String] {
        guard let settings else { return base }
        var environment = base

        setProxy(
            environment: &environment,
            variable: "HTTP_PROXY",
            enabled: boolValue(settings[kCFNetworkProxiesHTTPEnable]),
            host: stringValue(settings[kCFNetworkProxiesHTTPProxy]),
            port: intValue(settings[kCFNetworkProxiesHTTPPort]),
            scheme: "http"
        )
        setProxy(
            environment: &environment,
            variable: "HTTPS_PROXY",
            enabled: boolValue(settings[kCFNetworkProxiesHTTPSEnable]),
            host: stringValue(settings[kCFNetworkProxiesHTTPSProxy]),
            port: intValue(settings[kCFNetworkProxiesHTTPSPort]),
            scheme: "http"
        )
        setProxy(
            environment: &environment,
            variable: "ALL_PROXY",
            enabled: boolValue(settings[kCFNetworkProxiesSOCKSEnable]),
            host: stringValue(settings[kCFNetworkProxiesSOCKSProxy]),
            port: intValue(settings[kCFNetworkProxiesSOCKSPort]),
            scheme: "socks5h"
        )

        if environment["NO_PROXY"] == nil,
           let exceptions = settings[kCFNetworkProxiesExceptionsList] as? [String] {
            let values = (["127.0.0.1", "localhost"] + exceptions)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
                .reduce(into: [String]()) { result, value in
                    if !result.contains(value) { result.append(value) }
                }
            if !values.isEmpty { environment["NO_PROXY"] = values.joined(separator: ",") }
        }
        return environment
    }

    private static func setProxy(
        environment: inout [String: String],
        variable: String,
        enabled: Bool,
        host: String?,
        port: Int?,
        scheme: String
    ) {
        guard environment[variable] == nil, enabled,
              let host, !host.isEmpty, let port, (1...65_535).contains(port) else { return }
        let formattedHost = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
        environment[variable] = "\(scheme)://\(formattedHost):\(port)"
    }

    private static func boolValue(_ value: Any?) -> Bool {
        if let value = value as? Bool { return value }
        if let value = value as? NSNumber { return value.boolValue }
        return false
    }

    private static func intValue(_ value: Any?) -> Int? {
        if let value = value as? Int { return value }
        if let value = value as? NSNumber { return value.intValue }
        return nil
    }

    private static func stringValue(_ value: Any?) -> String? {
        value as? String
    }
}
