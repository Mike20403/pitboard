#if DEBUG
    import Foundation
    import PitboardKit
    import WebKit

    extension WebEnvironment {
        /// The scheme a fixture's stand-in sites are served under. WebKit does not let an app
        /// serve `https` itself, so each stand-in keeps its site's host on a scheme of its
        /// own, and the navigation policy is the same code in a fixture as in a live run.
        static let fixtureScheme = "pitboard-fixture"

        /// A fixture's windows: a stand-in page for every address of every site and sign-in
        /// host, stores that live in memory, downloads in the fixture's folder, and links to
        /// anywhere else opened nowhere. Nothing reaches a site, and nothing is written under
        /// `~/Library/WebKit` of whoever runs the tests. Their records are the fixture's
        /// model's, in the fixture's folder.
        static func fixture(
            folder: URL, openElsewhere: @escaping @MainActor (URL) -> Void = { _ in }
        ) -> WebEnvironment {
            let downloads = folder.appendingPathComponent("Downloads")
            try? FileManager.default.createDirectory(
                at: downloads, withIntermediateDirectories: true)
            return WebEnvironment(
                scheme: fixtureScheme,
                stores: FixtureDataStores(),
                downloads: downloads,
                openElsewhere: openElsewhere,
                configure: { configuration in
                    configuration.setURLSchemeHandler(
                        FixturePages(), forURLScheme: fixtureScheme)
                },
                pause: { try? await Task.sleep(for: $0) })
        }
    }

    /// Stores that live in memory, one per identifier, gone when the app quits.
    @MainActor
    final class FixtureDataStores: WebsiteDataStores {
        private var stores: [UUID: WKWebsiteDataStore] = [:]

        func store(for id: UUID) -> WKWebsiteDataStore {
            if let store = stores[id] { return store }
            let store = WKWebsiteDataStore.nonPersistent()
            stores[id] = store
            return store
        }

        func remove(_ id: UUID) async throws {
            stores[id] = nil
        }
    }

    /// Answers every request on the fixture's scheme with the page `fixture_page` makes for
    /// it from the site table, the same page the Windows app serves: each site's stand-in, a
    /// sign-in stand-in for the hosts a site's sign-in goes to, which closes itself as a real
    /// one does once it is done, and an artifact's frame.
    final class FixturePages: NSObject, WKURLSchemeHandler {
        func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
            let url =
                urlSchemeTask.request.url
                ?? URL(string: "\(WebEnvironment.fixtureScheme):")!
            // Only a library with fixtures launches into one, and that one has the pages.
            let page = (try? fixturePage(url: url.absoluteString)) ?? ""
            let body = Data(page.utf8)
            urlSchemeTask.didReceive(
                URLResponse(
                    url: url, mimeType: "text/html", expectedContentLength: body.count,
                    textEncodingName: "utf-8"))
            urlSchemeTask.didReceive(body)
            urlSchemeTask.didFinish()
        }

        func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {}
    }
#endif
