import AppKit
import Foundation
import WebKit

/// One web page in an account's store: an account window's own page, or a sign-in window
/// one of its pages opened. It owns one `WKWebView` and says what the page is doing in
/// observable properties, as SwiftUI's `WebPage` does.
///
/// `WebPage` itself cannot host an account window: measured on macOS 27, it opens no window
/// for `window.open`, so a sign-in that opens one never starts, and the downloads it is asked
/// for never reach disk. Keeping this page's shape close to `WebPage`'s is what makes moving
/// to it a matter of conformances once it can.
@MainActor
@Observable
final class Page {
    /// Whose page this is.
    let role: PageRole
    @ObservationIgnored let webView: WKWebView

    private(set) var title = ""
    private(set) var url: URL?
    private(set) var isLoading = false
    private(set) var canGoBack = false
    private(set) var canGoForward = false
    /// Why the page is not shown, after its last load failed. WebKit shows no error page of
    /// its own, so without this the window would stay blank.
    private(set) var failure: String?
    /// The page zoom, in Safari's steps.
    private(set) var zoom = 1.0

    /// What to load again after a failure: the page that failed.
    @ObservationIgnored private var failedURL: URL?
    /// When the page's content process last ended, so a page that keeps ending it says so
    /// rather than loading forever.
    @ObservationIgnored private var lastEnded: Date?
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private var observations: [NSKeyValueObservation] = []
    /// Waiting for the page's load to end, to leave it.
    @ObservationIgnored private var leaving: CheckedContinuation<Void, Never>?

    /// A page for `configuration`, which says which store it keeps its data in. A sign-in
    /// window's page must be made from the configuration WebKit hands over, which is a copy of
    /// its opener's: that is what shares the account's store and keeps `window.opener`.
    init(
        role: PageRole, configuration: WKWebViewConfiguration,
        now: @escaping () -> Date = Date.init
    ) {
        self.role = role
        self.now = now
        webView = PageWebView(frame: .zero, configuration: configuration)
        // As Safari: two-finger swipes go back and forward, and a pinch zooms.
        webView.allowsBackForwardNavigationGestures = true
        webView.allowsMagnification = true
        // A force click previews a link through Quick Look, which loads it outside the
        // account's store, signed in as nobody.
        webView.allowsLinkPreview = false
        #if DEBUG
            webView.isInspectable = true
        #endif
        observe()
    }

    /// The configuration of an account window's own page, on `store`.
    static func configuration(
        store: WKWebsiteDataStore, configure: (WKWebViewConfiguration) -> Void
    ) -> WKWebViewConfiguration {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = store
        // Safari's popup blocker: a page opens a window only when a person clicked.
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        // Videos and other elements can fill the screen, as in Safari.
        configuration.preferences.isElementFullscreenEnabled = true
        configure(configuration)
        return configuration
    }

    private func observe() {
        func track<Value>(
            _ path: KeyPath<WKWebView, Value>,
            _ update: @escaping @MainActor @Sendable (Page, WKWebView) -> Void
        ) -> NSKeyValueObservation {
            webView.observe(path, options: [.initial, .new]) { [weak self] webView, _ in
                // WebKit changes these on the main thread, where it lives.
                MainActor.assumeIsolated {
                    guard let self else { return }
                    update(self, webView)
                }
            }
        }
        observations = [
            track(\.title) { $0.title = $1.title ?? "" },
            track(\.url) { $0.url = $1.url },
            track(\.isLoading) { $0.isLoading = $1.isLoading },
            track(\.canGoBack) { $0.canGoBack = $1.canGoBack },
            track(\.canGoForward) { $0.canGoForward = $1.canGoForward },
        ]
    }

    // MARK: - Commands

    func load(_ url: URL) {
        failure = nil
        failedURL = nil
        webView.load(URLRequest(url: url))
    }

    /// Loads `request` as a page asked for it, keeping its method, body and referrer.
    func load(_ request: URLRequest) {
        failure = nil
        failedURL = nil
        webView.load(request)
    }

    func goBack() { webView.goBack() }
    func goForward() { webView.goForward() }

    /// Loads the page again, or after a failure the page that failed.
    func reload() {
        guard failure != nil else {
            webView.reload()
            return
        }
        failure = nil
        if let failedURL {
            webView.load(URLRequest(url: failedURL))
        } else {
            webView.reload()
        }
    }

    func stopLoading() { webView.stopLoading() }

    func zoom(_ change: ZoomChange) {
        zoom = Self.zoom(from: zoom, toward: change)
        webView.pageZoom = zoom
        // A pinch zooms on its own, and Actual Size undoes both, as in Safari: measured on
        // macOS 27, setting the page zoom leaves a pinch's magnification as it was.
        if change == .actualSize { webView.magnification = 1 }
    }

    /// Leaves the page for a blank one and waits, up to two seconds, until it has: the page's
    /// own leaving, its hide and unload handlers, runs then, and not after what comes next.
    func leave() async {
        webView.stopLoading()
        webView.load(URLRequest(url: URL(string: "about:blank")!))
        await withCheckedContinuation { done in
            leaving = done
            Task { [weak self] in
                try? await Task.sleep(for: .seconds(2))
                self?.loadFinished()
            }
        }
    }

    /// Stops everything the page is doing, for good: its window has closed.
    func close() {
        for observation in observations { observation.invalidate() }
        observations = []
        webView.stopLoading()
        webView.navigationDelegate = nil
        webView.uiDelegate = nil
        webView.removeFromSuperview()
    }

    // MARK: - What happened, as the page's delegate says

    func loadStarted() {
        failure = nil
    }

    func loadCommitted() {
        failure = nil
    }

    /// The page's load ended, finished or failed.
    func loadFinished() {
        leaving?.resume()
        leaving = nil
    }

    /// The page at `url` did not load, for `reason`.
    func loadFailed(_ url: URL?, reason: String) {
        failedURL = url ?? self.url
        failure = reason
    }

    /// The page's content process ended: it is loaded again, and a page that ends it again
    /// within `crashWindow` of the last time says so instead. A heavy page ends it after it
    /// has loaded, so a page loading again is no sign it is working.
    func contentEnded() {
        let ended = now()
        defer { lastEnded = ended }
        if let last = lastEnded, ended.timeIntervalSince(last) < Self.crashWindow {
            loadFailed(url, reason: "The page stopped working.")
            return
        }
        webView.reload()
    }

    /// How soon a second end of the content process counts as the page not working.
    nonisolated static let crashWindow: TimeInterval = 60

    // MARK: - Zoom

    enum ZoomChange: Equatable {
        case actualSize
        case zoomIn
        case zoomOut
    }

    /// Safari's zoom steps, as fractions of the page's size. WebKit has no list of its own.
    nonisolated static let zoomSteps: [Double] = [
        0.5, 0.75, 0.85, 1, 1.15, 1.25, 1.5, 1.75, 2, 2.5, 3,
    ]

    /// The page zoom after `change` from `current`: the next of Safari's steps either way,
    /// staying at the ends, or back to 100%.
    nonisolated static func zoom(from current: Double, toward change: ZoomChange) -> Double {
        let near = 0.001
        switch change {
        case .actualSize: return 1
        case .zoomIn: return zoomSteps.first { $0 > current + near } ?? zoomSteps.last!
        case .zoomOut: return zoomSteps.last { $0 < current - near } ?? zoomSteps.first!
        }
    }

    // MARK: - Failures

    /// Whether a failed load is worth saying. A load stopped for another,
    /// `NSURLErrorCancelled`, is not, and neither is one a policy decision ended, which every
    /// download and every stopped navigation gives: WebKit's frame load interrupted by a
    /// policy change, code 102 in `WebKitErrorDomain`.
    nonisolated static func isShown(_ error: Error) -> Bool {
        let error = error as NSError
        switch (error.domain, error.code) {
        case (NSURLErrorDomain, NSURLErrorCancelled): return false
        case (webKitErrorDomain, frameLoadInterruptedByPolicyChange): return false
        default: return true
        }
    }

    /// `WebKitErrorDomain`, which only the deprecated legacy WebKit header names.
    nonisolated static let webKitErrorDomain = "WebKitErrorDomain"
    /// `WebKitErrorFrameLoadInterruptedByPolicyChange`, from the same header.
    nonisolated static let frameLoadInterruptedByPolicyChange = 102
}

/// A page's web view, whose shortcut menu leaves out WebKit's own download items.
///
/// WebKit hands a download started from Download Linked File, Download Image or Download
/// Video to the app only through a private delegate call, and with no delegate it cancels the
/// download without a word: those items would do nothing. The site's own download buttons
/// work. The items are told apart by identifiers WebKit has used since macOS 10.12
/// (`WKMenuItemIdentifiersPrivate.h` in WebKit's source); macOS has no public way to adapt
/// them, or to learn which link the menu was opened on.
final class PageWebView: WKWebView {
    override func willOpenMenu(_ menu: NSMenu, with event: NSEvent) {
        super.willOpenMenu(menu, with: event)
        menu.items.removeAll { !Self.keeps($0.identifier?.rawValue ?? "") }
    }

    /// Whether the shortcut menu keeps the item WebKit identifies as `identifier`.
    nonisolated static func keeps(_ identifier: String) -> Bool {
        ![
            "WKMenuItemIdentifierDownloadLinkedFile", "WKMenuItemIdentifierDownloadImage",
            "WKMenuItemIdentifierDownloadMedia",
        ].contains(identifier)
    }
}
