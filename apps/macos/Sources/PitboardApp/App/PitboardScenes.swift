import PitboardKit
import SwiftUI

/// Every scene the app has, for the app target's `App` to return: the menu bar item, the
/// main window, each account's window, the account picker, and the settings.
///
/// The menu bar item comes first, which makes it the scene the app starts with: an app with
/// no Dock icon opens no window at launch.
public struct PitboardScenes: Scene {
    let model: AppModel
    let windows: AccountWindows
    let requests: WindowRequests
    let openAtLogin: OpenAtLogin
    let commandLineLink: CommandLineLink
    let defaults: UserDefaults
    let updates: any Updates

    public init(delegate: AppDelegate, updates: any Updates) {
        model = delegate.model
        windows = delegate.windows
        requests = delegate.windowRequests
        openAtLogin = delegate.openAtLogin
        commandLineLink = delegate.commandLineLink
        defaults = delegate.defaults
        self.updates = updates
    }

    public var body: some Scene {
        MenuBarExtra {
            MenuBarContent(model: model, windows: windows, updates: updates)
                .defaultAppStorage(defaults)
        } label: {
            MenuBarLabel(model: model, windows: windows, requests: requests)
                .defaultAppStorage(defaults)
        }
        .menuBarExtraStyle(.menu)

        Window("Pitboard", id: MainWindow.id) {
            MainWindow(model: model, windows: windows, requests: requests)
                .defaultAppStorage(defaults)
        }
        .defaultSize(width: 760, height: 560)
        .commands {
            // Nothing here makes a document: what is new here is an account, from any pane.
            CommandGroup(replacing: .newItem) {
                Button("Add Account…") { model.send(.presentSheet(sheet: .add(provider: nil))) }
                    .keyboardShortcut("n")
            }
            AccountWindowCommands(windows: windows)
            RefreshCommands()
            // Pitboard's help is its documentation site; the app has no help book.
            CommandGroup(replacing: .help) {
                Link("Pitboard Help", destination: Links.documentation)
            }
        }

        AccountWindowScene(windows: windows, defaults: defaults)

        // A link the Share extension hands over, as a Pitboard link, comes here whatever
        // else is open, and only here: no other scene makes a window for one.
        Window("Open Link", id: AccountPicker.id) {
            AccountPicker(windows: windows)
                .onOpenURL { windows.inbox.receive($0) }
                .defaultAppStorage(defaults)
        }
        .handlesExternalEvents(matching: ["*"])
        .windowResizability(.contentSize)
        .defaultPosition(.center)
        .commandsRemoved()

        Settings {
            SettingsView(
                model: model, openAtLogin: openAtLogin, commandLineLink: commandLineLink,
                presence: windows.presence, updates: updates
            )
            .defaultAppStorage(defaults)
        }
    }
}
