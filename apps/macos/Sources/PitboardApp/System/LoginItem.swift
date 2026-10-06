import Foundation
import Observation
import ServiceManagement

/// Whether Pitboard opens when its person logs in.
///
/// A protocol so a UI test can turn the setting on and off without registering the test
/// build as a login item of whoever runs the tests.
@MainActor
public protocol LoginItem: AnyObject {
    var state: LoginItemState { get }
    func register() throws
    func unregister() throws
    /// Login Items in System Settings, where a login item waiting for approval is approved.
    func openSystemSettings()
}

public enum LoginItemState: Equatable, Sendable {
    case enabled
    case disabled
    /// Registered, and waiting for its person to allow it in System Settings. macOS asks
    /// this of an app whose login item it has not seen before, and until then it does not
    /// open at login.
    case requiresApproval
}

/// This app, as macOS's own login items list has it.
@MainActor
public final class MainAppLoginItem: LoginItem {
    public init() {}

    public var state: LoginItemState {
        switch SMAppService.mainApp.status {
        case .enabled: .enabled
        case .requiresApproval: .requiresApproval
        default: .disabled
        }
    }

    public func register() throws { try SMAppService.mainApp.register() }
    public func unregister() throws { try SMAppService.mainApp.unregister() }
    public func openSystemSettings() { SMAppService.openSystemSettingsLoginItems() }
}

/// The settings' Open Pitboard at login: what macOS has, and why a change it refused did not
/// happen. macOS's own, never the model's: the login item belongs to this app's bundle.
@MainActor
@Observable
public final class OpenAtLogin {
    @ObservationIgnored private let item: any LoginItem
    /// Whether Pitboard opens at login, as macOS last said.
    private(set) var state: LoginItemState
    /// Why opening at login could not be changed.
    private(set) var failed: String?

    init(_ item: any LoginItem) {
        self.item = item
        state = item.state
    }

    /// Asks macOS again, since the person can change it in System Settings at any time.
    func read() {
        state = item.state
    }

    /// Registers or unregisters this app as a login item. A registration macOS wants
    /// approved stays waiting until the person allows it in System Settings, which the
    /// settings then offer to open.
    func set(_ wanted: Bool) {
        failed = nil
        do {
            if wanted {
                try item.register()
            } else {
                try item.unregister()
            }
        } catch {
            failed = error.localizedDescription
        }
        state = item.state
    }

    func openSystemSettings() {
        item.openSystemSettings()
    }
}
