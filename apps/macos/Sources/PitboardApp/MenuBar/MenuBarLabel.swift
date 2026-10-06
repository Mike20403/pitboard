import AppKit
import PitboardKit
import SwiftUI

/// What sits in the menu bar: Pitboard's mark, and the account in use with its tightest
/// limit unless the settings say to show less.
///
/// The one view alive from launch to quit, so it is also what opens a window asked for away
/// from any view: the main window whenever the model asks for it, from the menu, from a
/// notification's button, or on the first launch ever, and an account's window from the
/// Dock icon's menu.
struct MenuBarLabel: View {
    let model: AppModel
    let windows: AccountWindows
    let requests: WindowRequests
    @Environment(\.openWindow) private var openWindow
    @AppStorage(DefaultsKey.menuBarShows) private var shows = MenuBarShows.nameAndUsage

    var body: some View {
        let title = shows.text(of: model.menuBar)
        // A mark as well as words: on a crowded menu bar macOS drops the widest items
        // first, and an item that is only text is the widest thing up there. A Label would
        // render as the icon alone, so both are placed by hand.
        HStack(spacing: 4) {
            Image(systemName: Symbol.menuBar)
            if !title.isEmpty {
                Text(title).monospacedDigit()
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(model.menuBar.spoken)
        // As it first appears too: the first launch's request can be in before it is drawn.
        .onChange(of: model.windowRequest.serial, initial: true) {
            guard requests.opens(model.windowRequest) else { return }
            // An app with no Dock icon has nothing to bring forward but itself, and the
            // window opens behind whatever is in front otherwise.
            windows.presence.activate()
            openWindow(id: MainWindow.id)
        }
        .onChange(of: windows.requested) {
            for store in windows.takeRequests() {
                windows.presence.activate()
                openWindow(id: AccountWindowScene.id, value: store)
            }
        }
    }
}
