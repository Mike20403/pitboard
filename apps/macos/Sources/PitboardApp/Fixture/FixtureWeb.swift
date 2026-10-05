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
        /// host, stores that live in memory, downloads in the fixture's temporary folder, and
        /// links to anywhere else opened nowhere. Nothing reaches a site, and nothing is
        /// written under `~/Library/WebKit` of whoever runs the tests.
        static func fixture(
            defaults: UserDefaults, openElsewhere: @escaping @MainActor (URL) -> Void = { _ in }
        ) -> WebEnvironment {
            let downloads = FileManager.default.temporaryDirectory
                .appendingPathComponent("pitboard-fixture/Downloads")
            try? FileManager.default.createDirectory(
                at: downloads, withIntermediateDirectories: true)
            return WebEnvironment(
                scheme: fixtureScheme,
                stores: FixtureDataStores(),
                record: StoreRecord(defaults: defaults, directory: "fixture"),
                pages: PageRecord(defaults: defaults, directory: "fixture"),
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

    /// Answers every request on the fixture's scheme with a page for its host: each site's
    /// stand-in, a sign-in stand-in for the hosts a site's sign-in goes to, which closes
    /// itself as a real one does once it is done, and an artifact's frame.
    final class FixturePages: NSObject, WKURLSchemeHandler {
        /// The host an artifact's frame is served from: not the site's, as a real artifact's
        /// is not.
        static let artifactHost = "artifact.fixture"

        /// The page served for `url`.
        static func page(for url: URL) -> String {
            let host = url.host?.lowercased() ?? ""
            if let site = sites().first(where: { $0.host == host }) {
                return sitePage(site, path: url.path)
            }
            if sites().contains(where: { $0.signInHosts.contains(host) }) {
                return signInPage(host)
            }
            if host == artifactHost {
                // A message from the page clicks the link, as a person would inside the frame,
                // since a page cannot reach into a frame of another origin.
                return document(
                    title: "Artifact",
                    body: "<a id=\"artifact-download\" download=\"artifact.txt\" "
                        + "href=\"data:text/plain,artifact\">Download the artifact</a>"
                        + "<script>addEventListener('message', () => "
                        + "document.getElementById('artifact-download').click())</script>")
            }
            return document(title: "Stand-in", body: "<p>Nothing is here.</p>")
        }

        private static func sitePage(_ site: Site, path: String) -> String {
            let scheme = WebEnvironment.fixtureScheme
            let signIn = site.signInHosts.sorted().first.map { host in
                let address = "\(scheme)://\(host)/sign-in"
                return
                    "<p><a id=\"sign-in-link\" href=\"\(address)\" target=\"_blank\" "
                    + "rel=\"opener\">Continue with \(host)</a></p>"
                    + "<p><button id=\"sign-in-button\" onclick=\"window.open('\(address)', "
                    + "'sign-in', 'width=480,height=600')\">Sign in in a window</button></p>"
                    + "<p><button id=\"blank-popup\" onclick=\"const w = window.open(''); "
                    + "w.location = '\(address)'\">Sign in through a blank window</button></p>"
            }
            return document(
                title: "\(site.name) stand-in",
                body:
                    "<p>A Pitboard fixture page at \(path). Nothing here reaches the network. "
                    + "Find the word needle here, and the needle there.</p>"
                    + "<p><a id=\"outside\" href=\"https://example.com/\">A link outside "
                    + "\(site.name)</a></p>"
                    + "<p><a id=\"google\" href=\"https://accounts.google.com/o/oauth2/v2/auth\">"
                    + "Continue with Google</a></p>"
                    + "<p><a id=\"other-app\" href=\"vscode://file/x\">Open in an editor</a></p>"
                    + "<p><a id=\"chat\" href=\"\(scheme)://\(site.host)/chat/fixture\">A chat"
                    + "</a></p>"
                    + "<p><a id=\"download\" download=\"notes.txt\" "
                    + "href=\"data:text/plain,notes\">Download notes</a></p>"
                    + "<p><button id=\"alert\" onclick=\"alert('Saved.')\">Alert</button>"
                    + "<button id=\"confirm\" onclick=\"document.title = confirm('Delete?') ? "
                    + "'confirmed' : 'declined'\">Confirm</button></p>"
                    + (signIn ?? "")
                    + "<iframe title=\"artifact\" src=\"\(scheme)://\(artifactHost)/\" "
                    + "width=\"320\" height=\"80\"></iframe>")
        }

        private static func signInPage(_ host: String) -> String {
            document(
                title: "\(host) sign-in stand-in",
                body: "<p>A Pitboard fixture page for signing in.</p>"
                    + "<p><button id=\"done\" onclick=\"window.close()\">Done</button></p>")
        }

        private static func document(title: String, body: String) -> String {
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>\(title)</title>"
                + "</head><body><h1>\(title)</h1>\(body)</body></html>"
        }

        func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
            let url =
                urlSchemeTask.request.url
                ?? URL(string: "\(WebEnvironment.fixtureScheme):")!
            let body = Data(Self.page(for: url).utf8)
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
