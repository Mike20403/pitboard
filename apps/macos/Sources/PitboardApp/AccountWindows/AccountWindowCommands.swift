import PitboardKit
import SwiftUI

/// The account windows' scene: one window per enrolled account, keyed by its store, so
/// opening an account's window again brings the one open forward.
struct AccountWindowScene: Scene {
    /// The scene's id, named once so the scene and the code that opens it cannot drift apart.
    static let id = "account"

    let windows: AccountWindows
    let defaults: UserDefaults

    var body: some Scene {
        WindowGroup("Account", id: Self.id, for: UUID.self) { $store in
            AccountWindowView(windows: windows, store: store)
                .defaultAppStorage(defaults)
        }
        .defaultSize(width: 1100, height: 800)
        // A window opens only for an account somebody chose, never empty from File > New.
        .commandsRemoved()
    }
}

/// The menu bar's commands for account windows: opening one in the File menu, going back and
/// forward, zooming, finding, and removing what one keeps.
struct AccountWindowCommands: Commands {
    let windows: AccountWindows
    @FocusedValue(WebSession.self) private var session

    var body: some Commands {
        CommandGroup(after: .newItem) {
            Divider()
            SiteMenuItems(windows: windows)
            Divider()
            Button("Remove Website Data…") { session?.removalAsked = true }
                .disabled(session == nil)
        }
        // Edit > Find, which a window's find bar answers.
        TextEditingCommands()
        CommandGroup(after: .toolbar) {
            Button("Actual Size") { session?.page.zoom(.actualSize) }
                .keyboardShortcut("0")
                .disabled(session == nil)
            Button("Zoom In") { session?.page.zoom(.zoomIn) }
                .keyboardShortcut("+")
                .disabled(session == nil)
            Button("Zoom Out") { session?.page.zoom(.zoomOut) }
                .keyboardShortcut("-")
                .disabled(session == nil)
            Divider()
        }
        // Finder's Go menu, with Safari's shortcuts for going back and forward.
        CommandMenu("Go") {
            Button("Back") { session?.page.goBack() }
                .keyboardShortcut("[")
                .disabled(session?.page.canGoBack != true)
            Button("Forward") { session?.page.goForward() }
                .keyboardShortcut("]")
                .disabled(session?.page.canGoForward != true)
        }
    }
}

/// "Open claude.ai as work" for a site with one account, and an "Open claude.ai" submenu of
/// titles for a site with several: in the File menu and the menu bar item's menu alike.
struct SiteMenuItems: View {
    let windows: AccountWindows
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        ForEach(windows.menus, id: \.self) { menu in
            switch menu {
            case .one(let title, let account):
                Button(title) { open(account) }
            case .several(let title, _, let accounts):
                Menu(title) {
                    ForEach(accounts) { account in
                        Button(account.title) { open(account) }
                    }
                }
            }
        }
    }

    private func open(_ account: WindowAccount) {
        windows.presence.activate()
        openWindow(id: AccountWindowScene.id, value: account.id)
    }
}
