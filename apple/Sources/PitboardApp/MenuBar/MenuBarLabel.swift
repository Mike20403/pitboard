import AppKit
import SwiftUI

/// What sits in the menu bar: pitboard's mark, and the account in use with its tightest
/// limit unless the settings say to show less.
///
/// The one view alive from launch to quit, so it is also what opens the main window when
/// the model asks for it: from the menu, from a notification's button, or on the first
/// launch ever.
struct MenuBarLabel: View {
    let model: AppModel
    @Environment(\.openWindow) private var openWindow
    /// Whether this app has ever shown anyone anything.
    ///
    /// An app with no Dock icon that launches straight into a menu bar item shows a person
    /// who has just installed it nothing at all: no window, and nothing to explain the mark
    /// that appeared in their menu bar. Once, and never again.
    @AppStorage(DefaultsKey.hasBeenSeen) private var seen = false
    @AppStorage(DefaultsKey.menuBarShows) private var shows = MenuBarShows.nameAndUsage

    var body: some View {
        let title = menuTitle(for: model.status, order: model.tools, showing: shows)
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
        .accessibilityLabel(model.spokenTitle)
        .onChange(of: model.windowRequests) {
            // An app with no Dock icon has nothing to bring forward but itself, and the
            // window opens behind whatever is in front otherwise.
            NSApp.activate()
            openWindow(id: MainWindow.id)
        }
        .task {
            guard !seen else { return }
            seen = true
            model.showWindow()
        }
    }
}
