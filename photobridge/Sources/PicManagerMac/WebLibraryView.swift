import AppKit
import PhotoBridgeLib
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
        context.coordinator.webView = webView
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
        weak var webView: WKWebView?
        var wasAvailable = false

        init(serviceURL: URL, model: AppModel) {
            self.serviceURL = serviceURL
            self.model = model
        }

        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard message.name == "picmanager", let body = message.body as? NSDictionary,
                  let action = body["action"] as? String else { return }
            if action == "syncApple" { Task { @MainActor in await model.synchronizeAppleInventory() } }
            if action == "configureGarmin" {
                Task { @MainActor in
                    NSApplication.shared.activate(ignoringOtherApps: true)
                    let result = await model.presentGarminCredentials()
                    await self.dispatch(event: "picmanager:garmin-credentials", detail: result)
                }
            }
            if action == "prepareGarmin",
               let request = GarminPreparationRequest(wireObject: body as? [String: Any] ?? [:]) {
                Task { @MainActor in
                    NSApplication.shared.activate(ignoringOtherApps: true)
                    let result = await model.prepareGarminForExplicitRequest(requestID: request.requestID)
                    await self.dispatch(event: "picmanager:garmin-prepared", detail: result)
                }
            }
        }

        @MainActor
        private func dispatch<T: Encodable>(event: String, detail: T) async {
            do {
                let data = try JSONEncoder().encode(detail)
                let object = try JSONSerialization.jsonObject(with: data)
                guard let webView else {
                    model.lastError = "The embedded Web view closed before the native response was delivered."
                    return
                }
                _ = try await webView.callAsyncJavaScript(
                    "window.dispatchEvent(new CustomEvent(eventName, { detail: detail }));",
                    arguments: ["eventName": event, "detail": object],
                    in: nil,
                    contentWorld: .page
                )
            } catch {
                model.lastError = "The native Garmin response could not be delivered: \(error.localizedDescription)"
            }
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
