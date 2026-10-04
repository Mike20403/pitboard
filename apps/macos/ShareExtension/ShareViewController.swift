import AppKit
import PitboardSites
import SwiftUI
import UniformTypeIdentifiers

/// **Pitboard** in the system Share menu: hands the page being shared to the app it came in,
/// whose account picker asks which account's window opens it.
///
/// Sandboxed, as an app extension must be, with no network, no files and no group shared with
/// the app. It reads the one web address the host shares, checks it is a link of one of the
/// sites, so sharing any other page never starts Pitboard, and opens a Pitboard link with the
/// app it is inside. The app checks the link again all the same, since anything on the Mac
/// can open a Pitboard link.
final class ShareViewController: NSViewController {
    private let state = ShareState()
    private var started = false

    override func loadView() {
        let host = NSHostingController(
            rootView: ShareView(state: state) { [weak self] in self?.cancel() })
        // The host's size follows the view's, and the share sheet follows the host's.
        host.sizingOptions = .preferredContentSize
        addChild(host)
        view = host.view
        preferredContentSize = host.preferredContentSize
    }

    override func preferredContentSizeDidChange(for viewController: NSViewController) {
        super.preferredContentSizeDidChange(for: viewController)
        preferredContentSize = viewController.preferredContentSize
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        guard !started else { return }
        started = true
        Task { await share() }
    }

    private func share() async {
        let link: SiteLink
        do {
            link = try SiteLink(await sharedURL()?.absoluteString ?? "")
        } catch {
            state.phase = .refused(error.localizedDescription)
            return
        }
        // The app this extension came in, not whichever copy Launch Services picks: a debug
        // extension reaches the debug app.
        guard let app = Handoff.containingApp(of: Bundle.main.bundleURL),
            let scheme = Handoff.scheme(in: .main)
        else {
            state.phase = .failed("Pitboard couldn’t find the app this extension came with.")
            return
        }
        do {
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.activates = true
            try await NSWorkspace.shared.open(
                [Handoff.url(opening: link, scheme: scheme)], withApplicationAt: app,
                configuration: configuration)
            extensionContext?.completeRequest(returningItems: [], completionHandler: nil)
        } catch {
            state.phase = .failed(error.localizedDescription)
        }
    }

    /// The first web address among what the host shares.
    private func sharedURL() async -> URL? {
        let items = extensionContext?.inputItems.compactMap { $0 as? NSExtensionItem } ?? []
        let providers = items.flatMap { $0.attachments ?? [] }
        guard
            let provider = providers.first(where: {
                $0.hasItemConformingToTypeIdentifier(UTType.url.identifier)
            })
        else { return nil }
        // It returns its progress, so it comes into Swift with no async form.
        return await withCheckedContinuation { done in
            _ = provider.loadTransferable(type: URL.self) {
                done.resume(returning: try? $0.get())
            }
        }
    }

    private func cancel() {
        extensionContext?.cancelRequest(
            withError: NSError(domain: NSCocoaErrorDomain, code: NSUserCancelledError))
    }
}
