import PitboardKit

/// What the menu bar item shows beside its mark: the app's own setting.
///
/// macOS hides menu bar items to make room, the widest first, and a notched display has
/// little room to begin with. The name is what gets dropped first, and the mark alone is
/// for somebody who only ever opens the menu.
enum MenuBarShows: String, CaseIterable, Identifiable {
    case nameAndUsage
    case usage
    case icon

    var id: String { rawValue }

    var title: String {
        switch self {
        case .nameAndUsage: "Account and usage"
        case .usage: "Usage only"
        case .icon: "Icon only"
        }
    }

    /// The words beside the mark, of the forms the model says the bar in.
    func text(of bar: MenuBarText) -> String {
        switch self {
        case .nameAndUsage: bar.nameAndUsage
        case .usage: bar.usage
        case .icon: ""
        }
    }
}
