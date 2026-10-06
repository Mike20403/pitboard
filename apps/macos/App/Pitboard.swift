import PitboardApp
import SwiftUI

/// The menu bar app. Everything it shows and does is PitboardApp's; what this target adds is
/// Sparkle, which the library leaves out so its tests need no framework only a bundle can
/// load. `Main` starts it.
struct Pitboard: App {
    /// Owns the app's models, since AppKit asks it for the Dock icon's menu and before
    /// quitting, and it answers from the models the scenes show.
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var updater = SparkleUpdater()

    var body: some Scene {
        PitboardScenes(delegate: delegate, updates: updater)
    }
}
