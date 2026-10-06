import AppKit
import PitboardKit
import SwiftUI

/// What only an app delegate can do for Pitboard: start the model once the app has
/// launched and stop it as the app quits, tell it when the Mac wakes and when somebody opens
/// a menu, the Dock icon's menu, a click on the Dock icon with no window open, and asking
/// before quitting stops a download.
///
/// It owns the app's models, since AppKit asks it these at any time and it has to answer
/// from the same models the scenes show. Links from the Share extension are not its: the
/// account picker's scene receives them.
@MainActor
public final class AppDelegate: NSObject, NSApplicationDelegate {
    public let model: AppModel
    public let windows: AccountWindows
    public let openAtLogin: OpenAtLogin
    public let commandLineLink: CommandLineLink
    /// Which of the model's requests for the main window the menu bar item and the window
    /// have answered.
    let windowRequests = WindowRequests()
    /// Where the app keeps its view preferences, which views read through `@AppStorage`.
    public let defaults: UserDefaults
    private let quitting: @MainActor () -> Void
    private var watching: [NSObjectProtocol] = []

    override public init() {
        let dependencies = Dependencies.forLaunch()
        model = dependencies.model
        defaults = dependencies.defaults
        windows = AccountWindows(
            model: model, environment: dependencies.web, presence: .live())
        openAtLogin = OpenAtLogin(dependencies.loginItem)
        commandLineLink = CommandLineLink(dependencies.commandLineTool, model: model)
        quitting = dependencies.quitting
        super.init()
    }

    /// Starts what the model runs by itself, and tells it what only the app hears of: the Mac
    /// waking, after which numbers read before it slept say nothing about now, and a menu of
    /// this app opening. The menu bar item's menu is the glance the whole app exists for, and
    /// SwiftUI says nothing when it opens; AppKit says it of every menu, so a top-level one is
    /// taken to be it.
    public func applicationDidFinishLaunching(_ notification: Notification) {
        let model = model
        watching = [
            NSWorkspace.shared.notificationCenter.addObserver(
                forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
            ) { _ in
                MainActor.assumeIsolated { model.send(.woke) }
            },
            NotificationCenter.default.addObserver(
                forName: NSMenu.didBeginTrackingNotification, object: nil, queue: .main
            ) { note in
                let topLevel = (note.object as? NSMenu)?.supermenu == nil
                MainActor.assumeIsolated {
                    if topLevel { model.send(.glanced) }
                }
            },
        ]
        model.send(.start)
    }

    /// Stops the model, a sign-in under way with it, before the app goes.
    public func applicationWillTerminate(_ notification: Notification) {
        model.shutdown()
        quitting()
    }

    /// The Dock icon's menu, while Pitboard has one: each account's window, as the File menu
    /// offers them, and the Pitboard window. macOS lists the windows open above it.
    public func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        let menu = NSMenu()
        for entry in windows.menus {
            switch entry {
            case .one(let title, let account):
                menu.addItem(item(title, opening: account))
            case .several(let title, _, let accounts):
                let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
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
        item.representedObject = account.id
        return item
    }

    @objc private func openAccount(_ sender: NSMenuItem) {
        guard let store = sender.representedObject as? UUID,
            let account = windows.account(store)
        else { return }
        windows.request(account)
    }

    @objc private func openMainWindow() {
        model.send(.showWindow(pane: nil))
    }

    /// Opening Pitboard again from Finder, Spotlight or Launchpad with no window open shows
    /// the Pitboard window. A Dock click with every window minimised brings one back instead,
    /// as AppKit does by itself.
    public func applicationShouldHandleReopen(
        _ sender: NSApplication, hasVisibleWindows flag: Bool
    ) -> Bool {
        if !flag, !sender.windows.contains(where: \.isMiniaturized) {
            model.send(.showWindow(pane: nil))
        }
        return true
    }

    /// Quitting stops every download still running, so it asks first, as Safari does, in the
    /// model's words: of a download started a moment before Quit too, which no snapshot may
    /// list yet.
    public func applicationShouldTerminate(_ sender: NSApplication)
        -> NSApplication.TerminateReply
    {
        guard let question = windows.downloads.quitQuestion else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = question.title
        alert.informativeText = question.message
        alert.addButton(withTitle: question.confirm)
        alert.addButton(withTitle: "Cancel")
        // Quit is often chosen from the menu bar item's menu, which leaves the app behind
        // whatever is in front, and an alert from it would come up behind as well.
        NSApp.activate()
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        windows.downloads.stopAll()
        return .terminateNow
    }
}
