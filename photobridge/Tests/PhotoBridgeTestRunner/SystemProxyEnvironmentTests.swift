import CFNetwork
import PhotoBridgeLib

func runSystemProxyEnvironmentTests() {
    suite("Mac system proxy environment") {
        test("maps enabled HTTP, HTTPS, SOCKS and exclusions") {
            let settings: [AnyHashable: Any] = [
                kCFNetworkProxiesHTTPEnable: 1,
                kCFNetworkProxiesHTTPProxy: "127.0.0.1",
                kCFNetworkProxiesHTTPPort: 7897,
                kCFNetworkProxiesHTTPSEnable: 1,
                kCFNetworkProxiesHTTPSProxy: "127.0.0.1",
                kCFNetworkProxiesHTTPSPort: 7897,
                kCFNetworkProxiesSOCKSEnable: 1,
                kCFNetworkProxiesSOCKSProxy: "127.0.0.1",
                kCFNetworkProxiesSOCKSPort: 7897,
                kCFNetworkProxiesExceptionsList: ["*.local", "10.0.0.0/8"],
            ]
            let environment = SystemProxyEnvironment.applyingMacSystemProxy(
                to: [:], settings: settings
            )
            try expect(environment["HTTP_PROXY"], equals: "http://127.0.0.1:7897")
            try expect(environment["HTTPS_PROXY"], equals: "http://127.0.0.1:7897")
            try expect(environment["ALL_PROXY"], equals: "socks5h://127.0.0.1:7897")
            try expect(
                environment["NO_PROXY"],
                equals: "127.0.0.1,localhost,*.local,10.0.0.0/8"
            )
        }

        test("preserves explicit environment overrides") {
            let settings: [AnyHashable: Any] = [
                kCFNetworkProxiesHTTPEnable: true,
                kCFNetworkProxiesHTTPProxy: "system.proxy",
                kCFNetworkProxiesHTTPPort: 8080,
                kCFNetworkProxiesExceptionsList: ["*.local"],
            ]
            let environment = SystemProxyEnvironment.applyingMacSystemProxy(
                to: [
                    "HTTP_PROXY": "http://explicit.proxy:9000",
                    "NO_PROXY": "example.test",
                ],
                settings: settings
            )
            try expect(environment["HTTP_PROXY"], equals: "http://explicit.proxy:9000")
            try expect(environment["NO_PROXY"], equals: "example.test")
        }

        test("ignores disabled and incomplete proxy records") {
            let settings: [AnyHashable: Any] = [
                kCFNetworkProxiesHTTPEnable: 0,
                kCFNetworkProxiesHTTPProxy: "disabled.proxy",
                kCFNetworkProxiesHTTPPort: 8080,
                kCFNetworkProxiesHTTPSEnable: 1,
                kCFNetworkProxiesHTTPSProxy: "",
                kCFNetworkProxiesHTTPSPort: 8080,
            ]
            let environment = SystemProxyEnvironment.applyingMacSystemProxy(
                to: ["UNCHANGED": "yes"], settings: settings
            )
            try expect(environment, equals: ["UNCHANGED": "yes"])
        }
    }
}
