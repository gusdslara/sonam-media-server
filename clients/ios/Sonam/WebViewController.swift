import UIKit
import WebKit

/// Mostra a interface Svelte servida pelo próprio servidor (fetch same-origin em /api, cookie de sessão).
///
/// Regras, iguais às do launcher Android TV:
/// - só navega dentro da origem do servidor; links para outras origens abrem no Safari;
/// - nenhuma ponte nativa é exposta ao JavaScript (sem WKScriptMessageHandler);
/// - cookies ficam no `WKWebsiteDataStore.default()`, persistidos entre aberturas do app.
final class WebViewController: UIViewController, WKNavigationDelegate, WKUIDelegate, WKDownloadDelegate, UIGestureRecognizerDelegate {
    var onChangeServer: () -> Void

    private let origin: URL
    private let originScheme: String
    private let originHost: String
    private let originPort: Int
    private let webView: WKWebView
    private let refreshControl = UIRefreshControl()
    private let spinner = UIActivityIndicatorView(style: .large)
    private let errorView = ConnectionErrorView()
    private var hasLoadedPage = false
    private var downloadDestinations: [WKDownload: URL] = [:]

    init(origin: URL, onChangeServer: @escaping () -> Void) {
        self.origin = origin
        self.onChangeServer = onChangeServer
        let scheme = origin.scheme?.lowercased() ?? "http"
        originScheme = scheme
        originHost = origin.host?.lowercased() ?? ""
        originPort = WebViewController.effectivePort(origin.port, scheme: scheme)

        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = WKWebsiteDataStore.default()
        configuration.allowsInlineMediaPlayback = true
        configuration.mediaTypesRequiringUserActionForPlayback = []
        configuration.allowsPictureInPictureMediaPlayback = true
        configuration.allowsAirPlayForMediaPlayback = true
        if #available(iOS 16.4, *) {
            // Fullscreen de elementos (player/visualizador de fotos) no iPad.
            configuration.preferences.isElementFullscreenEnabled = true
        }
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) não é usado")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = Theme.pageBackgroundUI
        overrideUserInterfaceStyle = .light

        webView.translatesAutoresizingMaskIntoConstraints = false
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.allowsBackForwardNavigationGestures = true
        webView.allowsLinkPreview = false
        webView.isOpaque = false
        webView.backgroundColor = Theme.pageBackgroundUI
        webView.scrollView.backgroundColor = Theme.pageBackgroundUI
        // O WKWebView já fica dentro da área segura (constraints abaixo).
        webView.scrollView.contentInsetAdjustmentBehavior = .never
        // Permite puxar para atualizar mesmo quando a página não rola.
        webView.scrollView.alwaysBounceVertical = true
        refreshControl.addTarget(self, action: #selector(pullToRefresh), for: .valueChanged)
        webView.scrollView.refreshControl = refreshControl
        view.addSubview(webView)

        // Equivalente a "segurar Voltar" do Android TV: tocar e segurar com dois dedos abre o menu do servidor.
        let serverGesture = UILongPressGestureRecognizer(target: self, action: #selector(handleServerGesture(_:)))
        serverGesture.numberOfTouchesRequired = 2
        serverGesture.minimumPressDuration = 0.7
        serverGesture.cancelsTouchesInView = false
        serverGesture.delegate = self
        webView.addGestureRecognizer(serverGesture)

        spinner.translatesAutoresizingMaskIntoConstraints = false
        spinner.color = Theme.accentUI
        spinner.hidesWhenStopped = true
        view.addSubview(spinner)

        errorView.translatesAutoresizingMaskIntoConstraints = false
        errorView.isHidden = true
        errorView.onRetry = { [weak self] in
            self?.loadHome()
        }
        errorView.onChangeServer = { [weak self] in
            self?.onChangeServer()
        }
        view.addSubview(errorView)

        let guide = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            webView.topAnchor.constraint(equalTo: guide.topAnchor),
            webView.bottomAnchor.constraint(equalTo: guide.bottomAnchor),
            webView.leadingAnchor.constraint(equalTo: guide.leadingAnchor),
            webView.trailingAnchor.constraint(equalTo: guide.trailingAnchor),
            spinner.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            spinner.centerYAnchor.constraint(equalTo: view.centerYAnchor),
            errorView.topAnchor.constraint(equalTo: view.topAnchor),
            errorView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            errorView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            errorView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])

        loadHome()
    }

    // MARK: - Carregamento

    private var displayAddress: String {
        origin.absoluteString
    }

    private func loadHome() {
        errorView.isHidden = true
        spinner.startAnimating()
        let request = URLRequest(url: origin, cachePolicy: .useProtocolCachePolicy, timeoutInterval: 15)
        webView.load(request)
    }

    @objc private func pullToRefresh() {
        if hasLoadedPage && webView.url != nil {
            webView.reload()
        } else {
            loadHome()
        }
    }

    private func handleFailure(_ error: Error, provisional: Bool) {
        refreshControl.endRefreshing()
        spinner.stopAnimating()
        let nsError = error as NSError
        // Cancelamentos (nova navegação, política .cancel/.download) não são falhas de conexão.
        if nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled {
            return
        }
        if nsError.domain == "WebKitErrorDomain" && nsError.code == 102 {
            return // "Frame load interrupted": acontece quando a resposta vira download.
        }
        guard provisional else {
            return
        }
        errorView.show(address: displayAddress, detail: nsError.localizedDescription)
    }

    // MARK: - Menu do servidor

    @objc private func handleServerGesture(_ gesture: UILongPressGestureRecognizer) {
        if gesture.state == .began {
            showServerMenu()
        }
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer) -> Bool {
        true
    }

    private func showServerMenu() {
        guard presentedViewController == nil else {
            return
        }
        let sheet = UIAlertController(title: "Sonam", message: "Conectado a \(displayAddress)", preferredStyle: .actionSheet)
        sheet.addAction(UIAlertAction(title: "Recarregar", style: .default) { [weak self] _ in
            self?.pullToRefresh()
        })
        sheet.addAction(UIAlertAction(title: "Trocar servidor", style: .default) { [weak self] _ in
            self?.onChangeServer()
        })
        sheet.addAction(UIAlertAction(title: "Cancelar", style: .cancel, handler: nil))
        presentCentered(sheet)
    }

    /// Action sheet e share sheet precisam de âncora de popover no iPad.
    private func presentCentered(_ controller: UIViewController) {
        if let popover = controller.popoverPresentationController {
            popover.sourceView = view
            popover.sourceRect = CGRect(x: view.bounds.midX, y: view.bounds.midY, width: 1, height: 1)
            popover.permittedArrowDirections = []
        }
        var presenter: UIViewController = self
        while let presented = presenter.presentedViewController {
            presenter = presented
        }
        presenter.present(controller, animated: true)
    }

    // MARK: - Origem

    private static func effectivePort(_ port: Int?, scheme: String) -> Int {
        if let port = port {
            return port
        }
        return scheme == "https" ? 443 : 80
    }

    private func isSameOrigin(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased() else {
            return false
        }
        return scheme == originScheme
            && host == originHost
            && WebViewController.effectivePort(url.port, scheme: scheme) == originPort
    }

    private func openOutside(_ url: URL) {
        guard let scheme = url.scheme?.lowercased(), ["http", "https", "mailto", "tel"].contains(scheme) else {
            return
        }
        UIApplication.shared.open(url, options: [:], completionHandler: nil)
    }

    // MARK: - WKNavigationDelegate

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        guard let url = navigationAction.request.url else {
            decisionHandler(.cancel)
            return
        }
        // target="_blank" / window.open: mesma origem abre aqui; outra origem vai para o Safari.
        if navigationAction.targetFrame == nil {
            decisionHandler(.cancel)
            if isSameOrigin(url) {
                webView.load(navigationAction.request)
            } else {
                openOutside(url)
            }
            return
        }
        let scheme = url.scheme?.lowercased() ?? ""
        if scheme == "about" || scheme == "blob" || isSameOrigin(url) {
            decisionHandler(navigationAction.shouldPerformDownload ? .download : .allow)
            return
        }
        decisionHandler(.cancel)
        let isMainFrame = navigationAction.targetFrame?.isMainFrame ?? false
        if isMainFrame && navigationAction.navigationType == .linkActivated {
            openOutside(url)
        }
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse, decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        if !navigationResponse.canShowMIMEType || WebViewController.isAttachment(navigationResponse.response) {
            decisionHandler(.download)
        } else {
            decisionHandler(.allow)
        }
    }

    private static func isAttachment(_ response: URLResponse) -> Bool {
        guard let http = response as? HTTPURLResponse,
              let disposition = http.value(forHTTPHeaderField: "Content-Disposition")
        else {
            return false
        }
        return disposition.lowercased().hasPrefix("attachment")
    }

    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
        if !hasLoadedPage && !refreshControl.isRefreshing {
            spinner.startAnimating()
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        hasLoadedPage = true
        spinner.stopAnimating()
        refreshControl.endRefreshing()
        errorView.isHidden = true
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        handleFailure(error, provisional: true)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        handleFailure(error, provisional: false)
    }

    /// O iOS pode encerrar o processo da página (memória, app em segundo plano); recarrega em vez de ficar em branco.
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        if webView.url != nil {
            webView.reload()
        } else {
            loadHome()
        }
    }

    func webView(_ webView: WKWebView, navigationAction: WKNavigationAction, didBecome download: WKDownload) {
        download.delegate = self
    }

    func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
        download.delegate = self
    }

    // MARK: - WKUIDelegate

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if let url = navigationAction.request.url {
            if isSameOrigin(url) {
                webView.load(navigationAction.request)
            } else {
                openOutside(url)
            }
        }
        return nil
    }

    // MARK: - WKDownloadDelegate (botões "Baixar" da interface)

    func download(_ download: WKDownload, decideDestinationUsing response: URLResponse, suggestedFilename: String, completionHandler: @escaping (URL?) -> Void) {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("Downloads", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: nil)
        } catch {
            completionHandler(nil)
            return
        }
        var name = suggestedFilename.replacingOccurrences(of: "/", with: "_")
        if name.isEmpty || name == "." || name == ".." {
            name = "download"
        }
        let destination = folder.appendingPathComponent(name, isDirectory: false)
        downloadDestinations[download] = destination
        completionHandler(destination)
    }

    func downloadDidFinish(_ download: WKDownload) {
        guard let file = downloadDestinations.removeValue(forKey: download) else {
            return
        }
        // Folha de compartilhamento: "Salvar em Arquivos", "Salvar Imagem", AirDrop etc.
        let share = UIActivityViewController(activityItems: [file], applicationActivities: nil)
        presentCentered(share)
    }

    func download(_ download: WKDownload, didFailWithError error: Error, resumeData: Data?) {
        downloadDestinations.removeValue(forKey: download)
        let alert = UIAlertController(title: "Não foi possível baixar", message: error.localizedDescription, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "OK", style: .default, handler: nil))
        presentCentered(alert)
    }
}
