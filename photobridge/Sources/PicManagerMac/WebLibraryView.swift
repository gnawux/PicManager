import AppKit
import SwiftUI
import WebKit

struct WebLibraryView: NSViewRepresentable {
    let serviceURL: URL
    let serviceAvailable: Bool
    let model: AppModel

    func makeCoordinator() -> Coordinator {
        Coordinator(serviceURL: serviceURL, model: model)
    }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.userContentController.add(context.coordinator, name: "picmanager")
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

    final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        var serviceURL: URL
        let model: AppModel
        var wasAvailable = false

        init(serviceURL: URL, model: AppModel) {
            self.serviceURL = serviceURL
            self.model = model
        }

        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard message.name == "picmanager", let body = message.body as? [String: Any],
                  let action = body["action"] as? String else { return }
            if action == "syncApple" { Task { await model.synchronizeAppleInventory() } }
            if action == "configureGarmin" { model.presentGarminCredentials() }
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
