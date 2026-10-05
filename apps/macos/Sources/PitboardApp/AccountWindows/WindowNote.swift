import Foundation
import PitboardKit

/// Something an account window has to say, in the bar above its page, until it is
/// dismissed or the next one replaces it. What it says is the core's, `windowNote`.
struct WindowNote: Equatable, Identifiable {
    let kind: WindowNoteKind
    let text: String
    /// Tells two notes of the same kind apart, so saying one again shows it again.
    let id = UUID()

    init(_ kind: WindowNoteKind, for account: WindowAccount) {
        self.kind = kind
        text = windowNote(kind: kind, account: account)
    }

    static func == (lhs: WindowNote, rhs: WindowNote) -> Bool { lhs.id == rhs.id }
}
