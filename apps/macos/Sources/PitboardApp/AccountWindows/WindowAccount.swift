import Foundation
import PitboardKit

// Which accounts have a window, the store that keeps each one's data, what each window is
// titled, the menus' entries, the windows an account's own menu opens and what forgetting an
// account deletes are the core's rules, in the snapshot's `accountWindows` and each
// `AccountItem`'s `windows` and `forget`. What is left here is the store as WebKit and the
// window's scene take it.

extension WindowAccount: Identifiable {
    /// The window's store, as WebKit names a store and as the account windows' scene keeps a
    /// window's value: one window per account. The core derives it as a UUID, so it always
    /// reads as one.
    public var id: UUID { UUID(uuidString: store)! }
}

extension OpenWindow: Identifiable {
    /// The window's store, as its account's.
    public var id: UUID { account.id }
}

extension PickerAccount: Identifiable {
    /// The store of the account's window.
    public var id: UUID { window.id }
}

extension DownloadShown: Identifiable {}
