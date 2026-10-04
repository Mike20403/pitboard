import AppKit
import SwiftUI
import WebKit

/// A sign-in window a page of an account's window opened, sharing the account's store.
///
/// An AppKit window rather than a SwiftUI scene: WebKit asks for the web view of a new
/// window and must have it back before it returns, and a scene opens later.
@MainActor
final class PopupWindow: NSObject, NSWindowDelegate {
    let page: Page
    /// Called once the window has closed, however it closed.
    var onClose: (() -> Void)?

    private let window: NSWindow
    private let delegate = PageDelegate()

    init(page: Page, session: WebSession, size: CGSize, over parent: NSWindow?) {
        self.page = page
        let host = NSHostingController(rootView: PopupView(page: page))
        // The view's title and subtitle become the window's.
        host.sceneBridgingOptions = [.title]
        host.sizingOptions = []
        window = NSWindow(contentViewController: host)
        window.styleMask = [.titled, .closable, .resizable, .miniaturizable]
        window.setContentSize(size)
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        window.identifier = NSUserInterfaceItemIdentifier(Self.identifier)
        super.init()
        delegate.attach(to: session, page: page)
        window.delegate = self
        if let parent {
            let frame = parent.frame
            window.setFrameOrigin(
                NSPoint(
                    x: frame.midX - window.frame.width / 2,
                    y: frame.midY - window.frame.height / 2))
        } else {
            window.center()
        }
        window.makeKeyAndOrderFront(nil)
    }

    /// The window's identifier, which UI tests find a sign-in window by.
    nonisolated static let identifier = "sign-in"

    /// The size of a sign-in window: what the page asked for, no smaller than 320 by 400
    /// points, and 500 by 640 for a side it did not name.
    nonisolated static func size(width: Double?, height: Double?) -> CGSize {
        CGSize(width: max(320, width ?? 500), height: max(400, height ?? 640))
    }

    func close() {
        window.close()
    }

    func windowWillClose(_ notification: Notification) {
        page.close()
        window.delegate = nil
        onClose?()
        onClose = nil
    }
}

/// A sign-in window's page, titled with its page's title and the host it is on, so a person
/// asked for a password can see whose page asks.
private struct PopupView: View {
    let page: Page

    var body: some View {
        let place = page.url.map { $0.host() ?? $0.absoluteString } ?? ""
        PageContent(page: page)
            .frame(minWidth: 320, minHeight: 400)
            .navigationTitle(page.title.isEmpty ? place : page.title)
            .navigationSubtitle(page.title.isEmpty ? "" : place)
    }
}
