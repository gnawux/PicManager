import AppKit
import SwiftUI
import WebKit

struct WebLibraryView: NSViewRepresentable {
    let serviceURL: URL
    let serviceAvailable: Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(serviceURL: serviceURL)
    }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .default()
        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = context.coordinator
        webView.allowsMagnification = true
        return webView
    }

    func updateNSView(_ webView: WKWebView, context: Context) {
        context.coordinator.serviceURL = serviceURL
        if serviceAvailable,
           (!context.coordinator.wasAvailable || webView.url?.absoluteString != serviceURL.absoluteString) {
            webView.load(URLRequest(url: serviceURL))
        }
        context.coordinator.wasAvailable = serviceAvailable
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var serviceURL: URL
        var wasAvailable = false

        init(serviceURL: URL) {
            self.serviceURL = serviceURL
        }

        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void
        ) {
            guard let destination = navigationAction.request.url else {
                decisionHandler(.cancel)
                return
            }
            if destination.host == serviceURL.host && destination.port == serviceURL.port {
                decisionHandler(.allow)
            } else {
                NSWorkspace.shared.open(destination)
                decisionHandler(.cancel)
            }
        }
    }
}
