/// The panes of the main window, named in the model so it can ask for one.
enum WindowPane: String, CaseIterable, Identifiable {
    case accounts
    case activity
    case machine

    var id: String { rawValue }

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
