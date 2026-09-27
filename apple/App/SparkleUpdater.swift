import AppKit
import Foundation
import Observation
import PitboardApp
import Sparkle

/// Updates, when the build was made to receive them. A build from a clone carries no update
/// key, and Sparkle will not run without one, so such a build has no updater and says
/// nothing about updates.
///
/// An app with no Dock icon has nowhere to put a window nobody asked for, so Sparkle is told
/// this one handles scheduled updates gently: the panel says a version is ready, and the
/// person decides when to stop what they are doing.
@MainActor
@Observable
final class SparkleUpdater: NSObject, Updates, SPUStandardUserDriverDelegate {
    /// The version waiting, once one is, for the panel to offer.
    private(set) var waiting: String?

    @ObservationIgnored private var controller: SPUStandardUpdaterController?

    override init() {
        super.init()
        guard Bundle.main.object(forInfoDictionaryKey: "SUPublicEDKey") != nil else { return }
        controller = SPUStandardUpdaterController(
            startingUpdater: true, updaterDelegate: nil, userDriverDelegate: self)
    }

    var available: Bool { controller != nil }

    /// Sparkle's own settings, for a Settings pane to show. An app with no Dock icon has no
    /// menu bar to put Sparkle's own checkbox in, so this is the only place they can be.
    /// Sparkle keeps them, so a view is told of a change here by hand.
    var checksAutomatically: Bool {
        get {
            access(keyPath: \.checksAutomatically)
            return controller?.updater.automaticallyChecksForUpdates ?? false
        }
        set {
            withMutation(keyPath: \.checksAutomatically) {
                controller?.updater.automaticallyChecksForUpdates = newValue
            }
        }
    }

    var installsAutomatically: Bool {
        get {
            access(keyPath: \.installsAutomatically)
            return controller?.updater.automaticallyDownloadsUpdates ?? false
        }
        set {
            withMutation(keyPath: \.installsAutomatically) {
                controller?.updater.automaticallyDownloadsUpdates = newValue
            }
        }
    }

    /// Shows Sparkle's own window: what it finds, what changed, and the install button.
    func check() {
        waiting = nil
        controller?.updater.checkForUpdates()
    }

    nonisolated var supportsGentleScheduledUpdateReminders: Bool { true }

    nonisolated func standardUserDriver(
        _ driver: SPUStandardUserDriver,
        willShowModalAlert alert: NSAlert
    ) {
        // A modal alert from an app with no Dock icon arrives behind everything otherwise.
        Task { @MainActor in NSApp.activate(ignoringOtherApps: true) }
    }

    nonisolated func standardUserDriverWillHandleShowingUpdate(
        _ handleShowingUpdate: Bool,
        forUpdate update: SUAppcastItem,
        state: SPUUserUpdateState
    ) {
        guard !state.userInitiated else { return }
        let version = update.displayVersionString
        Task { @MainActor in self.waiting = version }
    }
}
