import Foundation
import PitboardApp
import PitboardKit

/// Where this program starts, before any of the app does. A renewal schedule an app up to
/// 0.3.0 wrote starts this executable with `renew`, and `Launch` says why that has to reach
/// the command line inside the bundle instead of the menu bar app.
@main
enum Main {
    @MainActor
    static func main() {
        let helper = Settings.bundledCommandLine(in: Bundle.main.bundleURL)
        switch Launch.action(for: CommandLine.arguments, helper: helper) {
        case .app:
            Pitboard.main()
        case .renew(let helper):
            Launch.renew(with: helper)
        case .fail:
            Launch.fail("this copy of Pitboard has no command line inside it to renew with.")
        }
    }
}
