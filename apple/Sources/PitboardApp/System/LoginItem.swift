import Foundation
import ServiceManagement

/// Whether pitboard opens when its person logs in.
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
