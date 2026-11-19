import SwiftUI

/// Ponte SwiftUI → `WebViewController` (UIKit), onde ficam o WKWebView e seus delegates.
struct WebScreen: UIViewControllerRepresentable {
    let origin: URL
    let onChangeServer: () -> Void

    func makeUIViewController(context: Context) -> WebViewController {
        WebViewController(origin: origin, onChangeServer: onChangeServer)
    }

    func updateUIViewController(_ controller: WebViewController, context: Context) {
        controller.onChangeServer = onChangeServer
    }
}
