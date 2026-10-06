import Foundation
import PitboardKit

/// What this app kept in UserDefaults before the model kept it in files of its own: the
/// preferences, now in `app.json` in Pitboard's directory, and the account windows' records,
/// now in `windows.json` in the app's own folder. Each is handed to the model once, and taken
/// out of UserDefaults once its file is there.
///
/// Which of the two wins, and what an old key means, is the model's: where the file is there
/// it reads that and nothing else. So the keys are kept until the file is there, and a launch
/// that stops before the model wrote it hands them over again.
struct EarlierStore {
    let defaults: UserDefaults
    /// `app.json` in the Pitboard directory this launch serves.
    let appFile: URL
    /// `windows.json` in the app's own folder.
    let windowsFile: URL

    /// The store of this launch: the app's own defaults, `app.json` in the Pitboard directory
    /// `environment` names, as the core reads it, and `windows.json` in `windowsDirectory`.
    init(defaults: UserDefaults, environment: [String: String], windowsDirectory: URL) {
        self.init(
            defaults: defaults,
            appFile: URL(fileURLWithPath: pitboardDirectory(environment: environment))
                .appendingPathComponent("app.json"),
            windowsFile: windowsDirectory.appendingPathComponent("windows.json"))
    }

    init(defaults: UserDefaults, appFile: URL, windowsFile: URL) {
        self.defaults = defaults
        self.appFile = appFile
        self.windowsFile = windowsFile
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

    /// The account windows' records UserDefaults held, for `WindowsLaunch.earlier`, as the
    /// app's `StoreRecord` and `PageRecord` wrote them: every Pitboard directory's, by its
    /// standardised path, each store as `UUID.uuidString` writes it. Nil once `windows.json`
    /// is there, when the keys are taken out, and nil where neither was ever set.
    func handOverWindows() -> EarlierWindowRecords? {
        guard !forgetWindowsOnceKept() else { return nil }
        let stores =
            defaults.object(forKey: DefaultsKey.Earlier.webStores) as? [String: [String]]
        let pages =
            defaults.object(forKey: DefaultsKey.Earlier.windowPages)
            as? [String: [String: String]]
        guard stores != nil || pages != nil else { return nil }
        return EarlierWindowRecords(stores: stores ?? [:], pages: pages ?? [:])
    }

    /// Takes the preferences' keys out of UserDefaults where the model has kept what they
    /// held in `app.json`, and says whether it had: at launch, and again as the app quits,
    /// once the model has stopped.
    @discardableResult
    func forgetOnceKept() -> Bool {
        forget(DefaultsKey.Earlier.all, onceThere: appFile)
    }

    /// Takes the account windows' keys out of UserDefaults where the model has kept what
    /// they held in `windows.json`, and says whether it had, as `forgetOnceKept` does.
    @discardableResult
    func forgetWindowsOnceKept() -> Bool {
        forget(DefaultsKey.Earlier.windows, onceThere: windowsFile)
    }

    private func forget(_ keys: [String], onceThere file: URL) -> Bool {
        guard FileManager.default.fileExists(atPath: file.path) else { return false }
        for key in keys { defaults.removeObject(forKey: key) }
        return true
    }
}
