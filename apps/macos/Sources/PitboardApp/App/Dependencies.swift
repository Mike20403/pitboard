import Foundation
import PitboardKit
import PitboardLinkTarget

/// Everything the app reaches outside itself through: the Rust model over this machine, the
/// defaults it keeps its view preferences in, the login item, the command line link, and
/// the sites and WebKit stores of the account windows, whose records the model keeps.
///
/// Gathered in one value so a launch decides once which world the app runs in. A UI test
/// launches the debug build into a fixture, where the model is a fixture's, over the real
/// core on a machine of its own, and nothing reaches the keychain, the network, launchd, the
/// login items or an administrator prompt of whoever runs the tests.
@MainActor
public struct Dependencies {
    let model: AppModel
    let defaults: UserDefaults
    let loginItem: any LoginItem
    let commandLineTool: CommandLineTool
    /// The account windows' world: the sites, WebKit's stores and the Downloads folder, or a
    /// fixture's stand-ins.
    let web: WebEnvironment
    /// What is done as the app quits, once the model has stopped.
    let quitting: @MainActor () -> Void

    /// This machine, as the person running the app has it: the model over the core this
    /// app's environment and bundle make, as the command line reads its own, with macOS's
    /// apps, Notification Center and the person's clock, keeping the account windows'
    /// records in the app's own folder in Application Support, under the Pitboard directory
    /// this launch serves, for the Pitboard links of this build's scheme: `pitboard`, or
    /// `pitboard-debug` in a debug build.
    public static func live(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        bundle: URL = Bundle.main.bundleURL
    ) -> Dependencies {
        let defaults = UserDefaults.standard
        let records = WebEnvironment.recordsDirectory()
        let earlier = EarlierStore(
            defaults: defaults, environment: environment, windowsDirectory: records)
        let notifications = MacNotifications(delivering: bundle.pathExtension == "app")
        let launch = AppLaunch(
            environment: environment, appLocation: bundle.path,
            earlierPreferences: earlier.handOver(),
            windows: WindowsLaunch(
                directory: records.path,
                key: WebEnvironment.recordKey(environment: environment),
                linkScheme: LinkTarget.scheme(in: .main) ?? "pitboard",
                earlier: earlier.handOverWindows()))
        let model = AppModel.listening { listener in
            PitboardModel(
                launch: launch, listener: listener, apps: MacAppControl.live(),
                notifications: notifications, localTime: MacLocalTime())
        }
        notifications.onSwitch = { [weak model] qualified in
            model?.send(.switchTo(qualified: qualified))
        }
        return Dependencies(
            model: model,
            defaults: defaults,
            loginItem: MainAppLoginItem(),
            commandLineTool: CommandLineTool(bundle: bundle),
            web: .live(),
            quitting: {
                earlier.forgetOnceKept()
                earlier.forgetWindowsOnceKept()
            })
    }

    /// The world this launch runs in: `live()`, unless this is a debug build started with
    /// `PITBOARD_FIXTURE` naming one of the fixtures UI tests use.
    public static func forLaunch(
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> Dependencies {
        #if DEBUG
            if let name = environment[Fixture.variable] {
                return Fixture.dependencies(named: name, environment: environment)
            }
        #endif
        return live(environment: environment)
    }
}
