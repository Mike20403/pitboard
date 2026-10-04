import SwiftUI

/// What View > Refresh does in the window in front: read the accounts again, read the log
/// again, check this Mac again, or load an account window's page again.
///
/// One command for every window, so Command-R means one item in the menu bar and the key
/// window's own action, and a toolbar's Refresh is in the menu bar as well.
struct RefreshCommand {
    /// The menu item's title in that window.
    let title: String
    let disabled: Bool
    let perform: @MainActor () -> Void
}

extension FocusedValues {
    @Entry var refresh: RefreshCommand?
}

/// View > Refresh, or Reload Page in an account window.
struct RefreshCommands: Commands {
    @FocusedValue(\.refresh) private var refresh

    var body: some Commands {
        CommandGroup(before: .toolbar) {
            Button(refresh?.title ?? "Refresh") { refresh?.perform() }
                .keyboardShortcut("r")
                .disabled(refresh == nil || refresh?.disabled == true)
            Divider()
        }
    }
}
