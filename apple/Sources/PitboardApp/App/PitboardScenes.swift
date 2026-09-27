import SwiftUI

/// Every scene the app has, for the app target's `App` to return: the menu bar item, the
/// main window, and the settings.
///
/// The menu bar item comes first, which makes it the scene the app starts with: an app with
/// no Dock icon opens no window at launch.
public struct PitboardScenes: Scene {
    let model: AppModel
    let updates: any Updates

    public init(model: AppModel, updates: any Updates) {
        self.model = model
        self.updates = updates
    }

    public var body: some Scene {
        MenuBarExtra {
            MenuBarContent(model: model, updates: updates)
                .defaultAppStorage(model.defaults)
        } label: {
            MenuBarLabel(model: model)
                .defaultAppStorage(model.defaults)
        }
        .menuBarExtraStyle(.menu)

        Window("pitboard", id: MainWindow.id) {
            MainWindow(model: model)
                .defaultAppStorage(model.defaults)
        }
        .defaultSize(width: 760, height: 560)
        .commands {
            // Nothing here makes a document, and the window has its own Add Account button
            // and Command-N.
            CommandGroup(replacing: .newItem) {}
        }

        Settings {
            SettingsView(model: model, updates: updates)
                .defaultAppStorage(model.defaults)
        }
    }
}
