import SwiftUI

/// Every scene the app has, for the app target's `App` to return.
public struct PitboardScenes: Scene {
    let model: AppModel
    let updates: any Updates

    public init(model: AppModel, updates: any Updates) {
        self.model = model
        self.updates = updates
    }

    public var body: some Scene {
        MenuBarExtra("pitboard", systemImage: "speedometer") {
            Button("Quit pitboard") { NSApp.terminate(nil) }.keyboardShortcut("q")
        }
        .menuBarExtraStyle(.menu)
        Window("pitboard", id: "main") { Text("pitboard") }
        Settings { Text("Settings") }
    }
}
