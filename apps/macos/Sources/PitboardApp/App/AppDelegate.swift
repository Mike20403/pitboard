import AppKit
import SwiftUI

/// What only an app delegate can do for Pitboard: the Dock icon's menu, a click on the Dock
/// icon with no window open, and asking before quitting stops a download.
///
/// It owns the app's models, since AppKit asks it these at any time and it has to answer
/// from the same models the scenes show. Links from the Share extension are not its: the
/// account picker's scene receives them.
@MainActor
public final class AppDelegate: NSObject, NSApplicationDelegate {
    public let model: AppModel
    public let windows: AccountWindows

    override public init() {
        let dependencies = Dependencies.forLaunch()
        model = AppModel(dependencies: dependencies)
        windows = AccountWindows(
            model: model, environment: dependencies.web, presence: .live(),
            scheme: dependencies.linkScheme)
        super.init()
    }

    /// The Dock icon's menu, while Pitboard has one: each account's window, as the File menu
    /// offers them, and the Pitboard window. macOS lists the windows open above it.
    public func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        let menu = NSMenu()
        for entry in windows.menus {
            switch entry {
            case .one(let account):
                menu.addItem(item(entry.title, opening: account))
            case .several(_, let accounts):
                let item = NSMenuItem(title: entry.title, action: nil, keyEquivalent: "")
                let submenu = NSMenu()
                for account in accounts {
                    submenu.addItem(self.item(account.title, opening: account))
                }
                item.submenu = submenu
                menu.addItem(item)
            }
        }
        if !menu.items.isEmpty { menu.addItem(.separator()) }
        let main = NSMenuItem(
            title: "Open Pitboard", action: #selector(openMainWindow), keyEquivalent: "")
        main.target = self
        menu.addItem(main)
        return menu
    }

    private func item(_ title: String, opening account: WindowAccount) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(openAccount), keyEquivalent: "")
        item.target = self
        item.representedObject = account.store
        return item
    }

    @objc private func openAccount(_ sender: NSMenuItem) {
        guard let store = sender.representedObject as? UUID,
            let account = windows.account(store)
        else { return }
        windows.request(account)
    }

    @objc private func openMainWindow() {
        model.showWindow()
    }

    /// Opening Pitboard again from Finder, Spotlight or Launchpad with no window open shows
    /// the Pitboard window. A Dock click with every window minimised brings one back instead,
    /// as AppKit does by itself.
    public func applicationShouldHandleReopen(
        _ sender: NSApplication, hasVisibleWindows flag: Bool
    ) -> Bool {
        if !flag, !sender.windows.contains(where: \.isMiniaturized) { model.showWindow() }
        return true
    }

    /// Quitting stops every download still running, so it asks first, as Safari does.
    public func applicationShouldTerminate(_ sender: NSApplication)
        -> NSApplication.TerminateReply
    {
        let running = windows.downloads.running.count
        guard running > 0 else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText =
            running == 1
            ? "A download is in progress. Quit anyway?"
            : "\(running) downloads are in progress. Quit anyway?"
        alert.informativeText = "Quitting Pitboard stops them, and they will not resume."
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        // Quit is often chosen from the menu bar item's menu, which leaves the app behind
        // whatever is in front, and an alert from it would come up behind as well.
        NSApp.activate()
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        windows.downloads.stopAll()
        return .terminateNow
    }
}
