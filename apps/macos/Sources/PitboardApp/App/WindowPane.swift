import PitboardKit

/// The panes of the main window, as the window keeps the one it shows and the model names
/// the one it asks for.
enum WindowPane: String, CaseIterable, Identifiable {
    case accounts
    case activity
    case machine

    var id: String { rawValue }

    init(_ pane: Pane) {
        switch pane {
        case .accounts: self = .accounts
        case .activity: self = .activity
        case .machine: self = .machine
        }
    }

    /// The model's name for it.
    var pane: Pane {
        switch self {
        case .accounts: .accounts
        case .activity: .activity
        case .machine: .machine
        }
    }

    /// The pane to show once the window has been asked for by `request`: the one it wants,
    /// or this one where it wants none.
    func after(_ request: WindowRequest) -> WindowPane {
        request.pane.map(WindowPane.init) ?? self
    }

    var title: String {
        switch self {
        case .accounts: "Accounts"
        case .activity: "Activity"
        case .machine: "This Mac"
        }
    }

    var symbol: String {
        switch self {
        case .accounts: Symbol.accounts
        case .activity: Symbol.activity
        case .machine: Symbol.machine
        }
    }
}

/// Which of the model's requests for the main window have been answered, so that each is
/// answered once: the window opened for it, and the pane it wants shown.
///
/// The model asks by moving `WindowRequest.serial`, from 0, which asks for nothing. The menu
/// bar item opens the window as it first appears and each time the serial moves, since the
/// first launch's request can come before it is first drawn; the window shows the pane as it
/// appears and each time the serial moves while it is open. Answered by serial, a request
/// seen both ways is answered once, and one answered already is not answered again when the
/// window is opened some other way, from the Window menu or the Dock icon's.
@MainActor
final class WindowRequests {
    /// The serial of the last request the window was opened for.
    private var opened: UInt64 = 0
    /// The serial of the last request whose pane was shown.
    private var shown: UInt64 = 0

    /// Whether to open the window for `request`: once for each request.
    func opens(_ request: WindowRequest) -> Bool {
        guard request.serial > opened else { return false }
        opened = request.serial
        return true
    }

    /// The pane to show for `request`, from `pane`: the one it wants, once, and `pane` for a
    /// request that wants none or was answered already.
    func pane(for request: WindowRequest, from pane: WindowPane) -> WindowPane {
        guard request.serial > shown else { return pane }
        shown = request.serial
        return pane.after(request)
    }
}
