import AppKit
import Foundation
import WebKit

/// The dialogs a page asks for, as sheets on its window, titled with the site that asks, and
/// as an embedded page's when a frame inside the page asks, so a frame cannot pass its words
/// off as the site's own. A page with no window gets WebKit's own answer: an alert is
/// dismissed, a question and a file chooser are cancelled.
@MainActor
enum PageDialogs {
    static func alert(_ message: String, from frame: WKFrameInfo, in window: NSWindow?) async {
        guard let window else { return }
        let alert = dialog(message, from: frame)
        alert.addButton(withTitle: "OK")
        _ = await alert.beginSheetModal(for: window)
    }

    static func confirm(_ message: String, from frame: WKFrameInfo, in window: NSWindow?) async
        -> Bool
    {
        guard let window else { return false }
        let alert = dialog(message, from: frame)
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        return await alert.beginSheetModal(for: window) == .alertFirstButtonReturn
    }

    static func prompt(
        _ message: String, answer: String?, from frame: WKFrameInfo, in window: NSWindow?
    ) async -> String? {
        guard let window else { return nil }
        let alert = dialog(message, from: frame)
        let field = NSTextField(string: answer ?? "")
        field.frame = NSRect(x: 0, y: 0, width: 260, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        let answered = await alert.beginSheetModal(for: window) == .alertFirstButtonReturn
        return answered ? field.stringValue : nil
    }

    static func chooseFiles(_ parameters: WKOpenPanelParameters, in window: NSWindow?) async
        -> [URL]?
    {
        guard let window else { return nil }
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = parameters.allowsDirectories
        panel.allowsMultipleSelection = parameters.allowsMultipleSelection
        return await panel.beginSheetModal(for: window) == .OK ? panel.urls : nil
    }

    /// Whether a download of `name` from `host` goes ahead, asked on `window`. With no
    /// window to ask on, it does not.
    static func allowDownload(_ name: String, from host: String?, in window: NSWindow?) async
        -> Bool
    {
        guard let window else { return false }
        let alert = NSAlert()
        if let host, !host.isEmpty {
            alert.messageText = "Download “\(name)” from \(host)?"
        } else {
            alert.messageText = "Download “\(name)”?"
        }
        alert.informativeText =
            "A page inside the site, such as an artifact, asked to save this file in your "
            + "Downloads folder."
        alert.addButton(withTitle: "Download")
        alert.addButton(withTitle: "Cancel")
        return await alert.beginSheetModal(for: window) == .alertFirstButtonReturn
    }

    private static func dialog(_ message: String, from frame: WKFrameInfo) -> NSAlert {
        let alert = NSAlert()
        alert.messageText = title(
            host: frame.securityOrigin.host, isMainFrame: frame.isMainFrame)
        alert.informativeText = message
        return alert
    }

    /// Who a dialog says asks: the page's own host, or a page embedded in it, by its host when
    /// it has one. A frame of an origin of its own, such as a sandboxed one, has none.
    nonisolated static func title(host: String, isMainFrame: Bool) -> String {
        switch (isMainFrame, host.isEmpty) {
        case (true, false): "\(host) says"
        case (false, false): "An embedded page at \(host) says"
        case (_, true): "An embedded page says"
        }
    }
}
