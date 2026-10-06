import Foundation
import PitboardKit

/// The preferences this app kept in UserDefaults before the model kept them in `app.json` in
/// Pitboard's directory: handed to the model once, and taken out of UserDefaults once that
/// file is there.
///
/// Which of the two wins, and what an old key means, is the model's: where `app.json` is
/// there it reads that and nothing else. So the keys are kept until the file is there, and
/// a launch that stops before the model wrote it hands them over again.
struct EarlierStore {
    let defaults: UserDefaults
    /// `app.json` in the Pitboard directory this launch serves.
    let appFile: URL

    /// The store of this launch: the app's own defaults, and `app.json` in the Pitboard
    /// directory `environment` names, as the core reads it.
    init(defaults: UserDefaults, environment: [String: String]) {
        self.init(
            defaults: defaults,
            appFile: URL(fileURLWithPath: pitboardDirectory(environment: environment))
                .appendingPathComponent("app.json"))
    }

    init(defaults: UserDefaults, appFile: URL) {
        self.defaults = defaults
        self.appFile = appFile
    }

    /// What UserDefaults held, for `AppLaunch.earlierPreferences`: nil once `app.json` is
    /// there, when the keys are taken out, and nil where none was ever set.
    func handOver() -> EarlierPreferences? {
        guard !forgetOnceKept() else { return nil }
        guard DefaultsKey.Earlier.all.contains(where: { defaults.object(forKey: $0) != nil })
        else { return nil }
        return EarlierPreferences(
            secondAccountDeclined: defaults.stringArray(
                forKey: DefaultsKey.Earlier.secondAccountDeclined) ?? [],
            hasBeenSeen: defaults.bool(forKey: DefaultsKey.Earlier.hasBeenSeen),
            secondAccountNudgeHidden: defaults.bool(
                forKey: DefaultsKey.Earlier.hideSecondAccountNudge))
    }

    /// Takes the keys out of UserDefaults where the model has kept what they held in
    /// `app.json`, and says whether it had: at launch, and again as the app quits, once the
    /// model has stopped.
    @discardableResult
    func forgetOnceKept() -> Bool {
        guard FileManager.default.fileExists(atPath: appFile.path) else { return false }
        for key in DefaultsKey.Earlier.all { defaults.removeObject(forKey: key) }
        return true
    }
}
