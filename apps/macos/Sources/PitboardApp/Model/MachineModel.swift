import Foundation
import PitboardKit

/// Everything about this Mac rather than its accounts: the daily renewal schedule, the
/// `pitboard` a terminal runs, opening at login, what doctor finds, and what Pitboard has
/// changed. The settings and the window's other panes read it; the menu never does.
@MainActor
@Observable
final class MachineModel {
    private let service: any Core
    /// This app's own command line, and where a terminal would find it.
    let commandLineTool: CommandLineTool
    private let loginItem: any LoginItem

    /// Called after a renewal, so the accounts are read again with what it renewed.
    @ObservationIgnored var renewed: (() async -> Void)?

    /// Whether anything keeps parked logins alive without a command being run.
    private(set) var schedule: Schedule = .absent
    /// Why the schedule could not be changed, said beside the switch that tried.
    private(set) var scheduleFailed: String?
    /// The state being written, while it is: the switch shows what was asked for rather than
    /// snapping back until the scheduler answers, and cannot be pressed again meanwhile.
    private(set) var scheduling: Bool?
    /// What the last renewal came to, for the settings pane that started it.
    private(set) var renewals: [Renewed]?
    private(set) var renewing = false

    /// The `pitboard` a terminal runs, once the settings have looked.
    private(set) var commandLine: CommandLineTool.Found?
    /// Why the command line could not be linked, said beside the button that tried.
    private(set) var linkFailed: String?
    private(set) var linking = false

    /// Whether Pitboard opens at login, as macOS has it.
    private(set) var openAtLogin: LoginItemState
    /// Why opening at login could not be changed.
    private(set) var loginItemFailed: String?

    /// Every check Pitboard makes about this machine, once someone asks for them.
    private(set) var checks: [Check] = []
    private(set) var checking = false
    private(set) var checkedAt: Date?

    /// What Pitboard has changed, newest first, once someone asks for it.
    private(set) var changes: [Change] = []

    init(service: any Core, commandLineTool: CommandLineTool, loginItem: any LoginItem) {
        self.service = service
        self.commandLineTool = commandLineTool
        self.loginItem = loginItem
        openAtLogin = loginItem.state
    }

    // MARK: - Opening at login

    /// Asks macOS again, since the person can change it in System Settings at any time.
    func readLoginItem() {
        openAtLogin = loginItem.state
    }

    /// Registers or unregisters this app as a login item. A registration macOS wants
    /// approved stays waiting until the person allows it in System Settings, which the
    /// settings then offer to open.
    func setOpenAtLogin(_ wanted: Bool) {
        loginItemFailed = nil
        do {
            if wanted {
                try loginItem.register()
            } else {
                try loginItem.unregister()
            }
        } catch {
            loginItemFailed = error.localizedDescription
        }
        openAtLogin = loginItem.state
    }

    func openLoginItemSettings() {
        loginItem.openSystemSettings()
    }

    // MARK: - Renewing parked logins

    /// Whether anything keeps parked logins alive without a command being run.
    func readSchedule() async {
        schedule = await service.schedule()
    }

    /// Points a renewal schedule an app up to 0.3.0 wrote, which runs that app and renews
    /// nothing, at the command line inside this one, and shows the schedule again when it did.
    /// A failure is not said here: the schedule is as it was, and doctor still reports it.
    func repairSchedule() async {
        guard (try? await service.scheduleRepair()) == true else { return }
        await readSchedule()
    }

    /// Why daily renewal cannot be turned on from this copy of the app, or nil when it can.
    ///
    /// The schedule runs the command line inside the app long after the app has quit, so it
    /// needs one a link would keep reaching. A copy macOS runs from a temporary place is gone
    /// by then, and without one inside the app there is only the app itself to schedule,
    /// which renews nothing.
    var cannotSchedule: String? {
        if commandLineTool.linkable { return nil }
        if commandLineTool.translocated {
            return "Move Pitboard to your Applications folder first. Until then macOS runs it "
                + "from a temporary copy, which is gone once Pitboard quits."
        }
        return "This copy of Pitboard has no command line inside it to run on a schedule."
    }

    /// Whether daily renewal is on.
    var renewsDaily: Bool {
        if case .installed = schedule { return true }
        return false
    }

    /// Hand the renewal of parked logins to this computer's own scheduler, or take it back.
    ///
    /// Opt-in, and the settings say what it does before offering it: a background process
    /// that talks to a service on a schedule is the shape most likely to be read as
    /// automation, so it is something a person turns on knowing what it is.
    ///
    /// Turning it on is refused where `cannotSchedule` says why, and not only left out of the
    /// settings, so nothing that calls this can write a schedule that fails every day without
    /// telling anyone. Turning it off never is: that is how such a schedule is taken away.
    func setSchedule(on: Bool) async {
        guard scheduling == nil else { return }
        scheduling = on
        defer { scheduling = nil }
        scheduleFailed = nil
        if on, let why = cannotSchedule {
            scheduleFailed = why
            return
        }
        do {
            if on {
                _ = try await service.scheduleInstall()
            } else {
                _ = try await service.scheduleUninstall()
            }
        } catch {
            scheduleFailed = AppModel.saying(error)
        }
        await readSchedule()
    }

    /// Renew every parked login that is due, now. Never switches and never asks for usage.
    func renewNow() async {
        renewing = true
        defer { renewing = false }
        renewals = await service.renew()
        await renewed?()
    }

    // MARK: - The command line

    /// Looks for the `pitboard` a terminal runs: on the login shell's `PATH`, then where
    /// each way of installing it puts it.
    func findCommandLine() async {
        let path = await service.searchPath()
        let tool = commandLineTool
        commandLine = await Task.detached(priority: .utility) {
            tool.find(onPath: path)
        }.value
    }

    /// Links this app's command line onto the `PATH`, once macOS has asked for an
    /// administrator's password.
    func installCommandLine() async {
        linking = true
        defer { linking = false }
        linkFailed = nil
        if case .failed(let why) = await commandLineTool.install() {
            linkFailed = why
        }
        await findCommandLine()
    }

    // MARK: - Checks and changes

    /// What `pitboard doctor` reports, for when something is wrong at machine level and
    /// the one line a failed call carries is not enough to act on.
    func diagnose() async {
        checking = true
        defer { checking = false }
        checks = await service.doctor().checks
        checkedAt = Date()
    }

    /// What Pitboard has changed, newest first. Read when something asks to see it.
    func readChanges(_ limit: UInt32 = 500) async {
        changes = await service.log(limit: limit).reversed()
    }
}
