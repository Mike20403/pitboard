import PitboardApp
import SwiftUI

/// The menu bar app. Everything it shows and does is PitboardApp's; what this target adds is
/// Sparkle, which the library leaves out so its tests need no framework only a bundle can
/// load. `Main` starts it.
struct Pitboard: App {
    @State private var model = AppModel(dependencies: .forLaunch())
    @State private var updater = SparkleUpdater()

    var body: some Scene {
        PitboardScenes(model: model, updates: updater)
    }
}
