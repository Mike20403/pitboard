import AppKit
import Foundation
import WebKit

/// What one page of an account window asks WebKit's delegates, answered by the window's
/// policy: where each navigation goes, what is downloaded, which windows open, and the
/// dialogs, file choosers and permissions a page asks for.
///
/// The same delegate serves an account window's own page and a sign-in window's, by the
/// page's role, so a sign-in window says and refuses what the account window does.
@MainActor
final class PageDelegate: NSObject, WKNavigationDelegate, WKUIDelegate {
    private weak var session: WebSession?
    private weak var page: Page?

    /// Makes this the delegate of `page`, a page of `session`'s window.
    func attach(to session: WebSession, page: Page) {
        self.session = session
        self.page = page
        page.webView.navigationDelegate = self
        page.webView.uiDelegate = self
    }

    private var role: PageRole { page?.role ?? .popup }

    // MARK: - Navigations

    func webView(
        _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
    ) async -> WKNavigationActionPolicy {
        guard let session, let page else { return .cancel }
        let request = session.request(for: navigationAction, of: page)
        // A link that asks for a new window comes here first, and is let through so WebKit
        // asks for the window it wants, where the window is decided: refused here, it never
        // would be, and a sign-in link would open nothing.
        if request.target == .newWindow && !request.download { return .allow }
        switch session.policy.decide(request, in: role) {
        case .load, .loadInPage, .popup:
            return .allow
        case .download:
            return .download
        case .openElsewhere(let url):
            session.handOver(url, clicked: request.clicked)
            return .cancel
        case .refuse(let kind):
            session.say(kind)
            return .cancel
        case .ignore:
            return .cancel
        }
    }

    func webView(
        _ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse
    ) async -> WKNavigationResponsePolicy {
        guard let session else { return .cancel }
        let disposition = (navigationResponse.response as? HTTPURLResponse)?
            .value(forHTTPHeaderField: "Content-Disposition")
        let decision = session.policy.decideResponse(
            canShow: navigationResponse.canShowMIMEType, disposition: disposition,
            fromSite: fromSite(navigationResponse))
        switch decision {
        case .show: return .allow
        case .download: return role == .window ? .download : .cancel
        }
    }

    func webView(
        _ webView: WKWebView, navigationAction: WKNavigationAction,
        didBecome download: WKDownload
    ) {
        guard let session, let page else { return download.cancel(nil) }
        let request = session.request(for: navigationAction, of: page)
        guard case .download(let ask) = session.policy.decide(request, in: role) else {
            return download.cancel(nil)
        }
        session.download(download, ask: ask)
    }

    func webView(
        _ webView: WKWebView, navigationResponse: WKNavigationResponse,
        didBecome download: WKDownload
    ) {
        guard let session, role == .window else { return download.cancel(nil) }
        session.download(download, ask: !fromSite(navigationResponse))
    }

    /// Whether a response is to the window's own page on the site.
    private func fromSite(_ response: WKNavigationResponse) -> Bool {
        guard role == .window, response.isForMainFrame, let url = response.response.url else {
            return false
        }
        return session?.policy.isSite(url) == true
    }

    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!)
    {
        page?.loadStarted()
    }

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        page?.loadCommitted()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        page?.loadFinished()
    }

    func webView(
        _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!,
        withError error: Error
    ) {
        failed(error)
    }

    func webView(
        _ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error
    ) {
        failed(error)
    }

    private func failed(_ error: Error) {
        page?.loadFinished()
        guard Page.isShown(error) else { return }
        let failing = (error as NSError).userInfo[NSURLErrorFailingURLErrorKey] as? URL
        page?.loadFailed(failing, reason: error.localizedDescription)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        page?.contentEnded()
    }

    // MARK: - Windows

    func webView(
        _ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
        for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures
    ) -> WKWebView? {
        guard let session, let page else { return nil }
        let request = session.request(for: navigationAction, of: page)
        switch session.policy.decide(request, in: role) {
        case .popup:
            return session.openPopup(
                configuration: configuration, features: windowFeatures, over: webView.window)
        case .loadInPage:
            page.load(navigationAction.request)
        case .download(let ask):
            webView.startDownload(using: navigationAction.request) { [weak session] download in
                session?.download(download, ask: ask)
            }
        case .openElsewhere(let url):
            session.handOver(url, clicked: request.clicked)
        case .refuse(let kind):
            session.say(kind)
        case .load, .ignore:
            break
        }
        return nil
    }

    func webViewDidClose(_ webView: WKWebView) {
        // A sign-in window's page closes it once it is done. An account window's page cannot
        // close the account's window.
        if role == .popup { session?.closePopup() }
    }

    // MARK: - Dialogs, files and permissions

    func webView(
        _ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
        initiatedByFrame frame: WKFrameInfo
    ) async {
        await PageDialogs.alert(message, from: frame, in: webView.window)
    }

    func webView(
        _ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
        initiatedByFrame frame: WKFrameInfo
    ) async -> Bool {
        await PageDialogs.confirm(message, from: frame, in: webView.window)
    }

    func webView(
        _ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String,
        defaultText: String?, initiatedByFrame frame: WKFrameInfo
    ) async -> String? {
        await PageDialogs.prompt(prompt, answer: defaultText, from: frame, in: webView.window)
    }

    func webView(
        _ webView: WKWebView, runOpenPanelWith parameters: WKOpenPanelParameters,
        initiatedByFrame frame: WKFrameInfo
    ) async -> [URL]? {
        await PageDialogs.chooseFiles(parameters, in: webView.window)
    }

    /// The camera and the microphone are never given to a page: pitboard asks macOS for
    /// neither, and a voice conversation belongs in the site's own app.
    func webView(
        _ webView: WKWebView, decideMediaCapturePermissionsFor origin: WKSecurityOrigin,
        initiatedBy frame: WKFrameInfo, type: WKMediaCaptureType
    ) async -> WKPermissionDecision {
        .deny
    }
}
